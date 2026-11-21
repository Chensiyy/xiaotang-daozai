//! 本地开发包的安装、状态及卡片执行。每次操作原子保存一个插件的快照。
#[path = "cover.rs"]
mod cover;
#[cfg(windows)]
#[path = "market.rs"]
pub(super) mod market;
#[cfg(windows)]
#[path = "market_state.rs"]
mod market_state;
#[path = "local_ui.rs"]
mod ui;
use super::{fail, Result};
use cover::{CoverPng, COVER_FILE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

const MATERIAL_SOURCE_LIMIT: u64 = 64 * 1024;
const SCRIPT_SOURCE_LIMIT: u64 = 256 * 1024;
const BACKDROP_PERMISSION: &str = "screen.backdrop";
const API_VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    key: String,
    label: String,
    #[serde(rename = "type")]
    kind: String,
    default: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    step: Option<f32>,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    unit: Option<String>,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    precision: Option<u8>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Material {
    controls: bool,
    spring: MaterialSpring,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    shadow: Option<f32>,
    #[serde(default)]
    foreground: MaterialForeground,
}

fn present_value<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    input: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(input).map(Some)
}

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum MaterialForeground {
    #[default]
    Skin,
    Glass,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterialSpring {
    stiffness: f32,
    damping: f32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Manifest {
    id: String,
    name: String,
    author: String,
    version: String,
    api: u32,
    runtime: String,
    entry: String,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    cover: Option<String>,
    owner: String,
    capabilities: Vec<String>,
    data_version: u32,
    permissions: BTreeMap<String, Value>,
    settings: Vec<Field>,
    #[serde(default = "default_refresh")]
    refresh_seconds: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material: Option<Material>,
}
fn default_refresh() -> u32 {
    60
}
type Values = BTreeMap<String, String>;
#[derive(Clone, Serialize, Deserialize)]
struct Package {
    manifest: Manifest,
    source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cover: Option<CoverPng>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Record {
    origin: PathBuf,
    #[serde(default)]
    from_market: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    official_market: bool,
    package: Option<Package>,
    enabled: bool,
    values: Values,
    storage: Value,
    #[serde(default)]
    material_granted: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl Record {
    fn valid(&self, id: &str) -> bool {
        self.package.as_ref().is_none_or(|package| {
            let manifest = &package.manifest;
            manifest.id == id
                && manifest.validate().is_ok()
                && manifest.cover.is_some() == package.cover.is_some()
                && !package.source.trim().is_empty()
                && package.source.len() as u64 <= manifest.source_limit()
                && manifest.settings.iter().all(|field| {
                    self.values
                        .get(&field.key)
                        .is_some_and(|value| valid_field(field, value))
                })
        })
    }

    #[cfg(feature = "sdk-material")]
    fn material_package(&self) -> Option<&Package> {
        self.package
            .as_ref()
            .filter(|package| package.manifest.is_material())
    }
}

#[cfg(feature = "sdk-material")]
pub struct MaterialSpec<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub controls: bool,
    pub stiffness: f32,
    pub damping: f32,
    pub shadow: Option<f32>,
    pub glass_foreground: bool,
    pub params: [f32; 32],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Output {
    card: String,
    #[serde(default)]
    storage: Value,
}

fn bounded(path: &Path, max: u64) -> Result<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(max + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(fail("文件过大"));
    }
    Ok(String::from_utf8(bytes)?)
}
fn valid_id(id: &str) -> bool {
    id.len() <= 64
        && id.contains('.')
        && id.split('.').all(|p| {
            p.starts_with(|c: char| c.is_ascii_lowercase())
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}
fn valid_field(f: &Field, v: &str) -> bool {
    if f.kind != "number" && (f.unit.is_some() || f.precision.is_some()) {
        return false;
    }
    match f.kind.as_str() {
        "text" if f.min.is_none() && f.max.is_none() && f.step.is_none() => {
            !v.trim().is_empty() && v.chars().count() <= 80
        }
        "date" if f.min.is_none() && f.max.is_none() && f.step.is_none() => {
            chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").is_ok_and(|d| d.to_string() == v)
        }
        "number" => {
            let (Some(min), Some(max), Some(step)) = (f.min, f.max, f.step) else {
                return false;
            };
            let width = max - min;
            width.is_finite()
                && step > 0.0
                && step <= width
                && f.unit.as_ref().is_none_or(|unit| unit.chars().count() <= 4)
                && f.precision.is_none_or(|precision| precision <= 3)
                && v.parse::<f32>()
                    .is_ok_and(|number| number.is_finite() && (min..=max).contains(&number))
        }
        _ => false,
    }
}
impl Manifest {
    fn decode(contents: &str) -> Result<Self> {
        #[derive(Deserialize)]
        struct ApiHeader {
            api: u64,
        }
        // 先检查版本，使未来清单新增的字段能够得到明确的升级提示。
        let ApiHeader { api } = serde_json::from_str(contents)?;
        if api > u64::from(API_VERSION) {
            return Err(fail(&format!(
                "需要更新岛仔。此插件需要 API v{api}，当前支持 v{API_VERSION}"
            )));
        }
        let manifest: Self = serde_json::from_str(contents)?;
        manifest.validate()?;
        if manifest.cover.is_none() {
            return Err(fail("请在插件清单中声明 cover: \"cover.png\""));
        }
        Ok(manifest)
    }

    fn is_material(&self) -> bool {
        self.runtime == "material"
    }

    fn source_limit(&self) -> u64 {
        if self.is_material() {
            MATERIAL_SOURCE_LIMIT
        } else {
            SCRIPT_SOURCE_LIMIT
        }
    }

    fn validate(&self) -> Result<()> {
        let m = self;
        if !valid_id(&m.id)
            || !(60..=86400).contains(&m.refresh_seconds)
            || m.api != API_VERSION
            || m.data_version != 1
            || m.name.trim().is_empty()
            || m.name.chars().count() > 80
            || m.author.trim().is_empty()
            || m.author.chars().count() > 80
            || m.version.split('.').count() != 3
            || !m
                .version
                .split('.')
                .all(|v| v.parse::<u32>().is_ok_and(|n| n.to_string() == v))
        {
            return Err(fail("请核对插件 ID、作者、版本和 API 版本"));
        }
        let material = m.is_material();
        if material {
            if m.owner != "appearance"
                || m.permissions.len() != 1
                || m.permissions
                    .get(BACKDROP_PERMISSION)
                    .and_then(Value::as_bool)
                    != Some(true)
                || m.capabilities
                    .iter()
                    .filter(|c| c.as_str() == "appearance.material")
                    .count()
                    != 1
                || m.capabilities.len() > 2
                || !m
                    .capabilities
                    .iter()
                    .all(|c| matches!(c.as_str(), "appearance.material" | "settings"))
                || !m.material.as_ref().is_some_and(|material| {
                    (0.2..=3.0).contains(&material.spring.stiffness)
                        && (0.1..=1.0).contains(&material.spring.damping)
                        && material
                            .shadow
                            .is_none_or(|shadow| (0.0..=1.5).contains(&shadow))
                })
            {
                return Err(fail(
                    "请核对材质归属、屏幕底片权限、能力、弹簧参数和投影强度",
                ));
            }
        } else if m.runtime != "js"
            || m.owner != "tasks"
            || m.material.is_some()
            || !m.permissions.is_empty()
            || !m.capabilities.iter().any(|c| c == "card")
            || !m
                .capabilities
                .iter()
                .all(|c| matches!(c.as_str(), "card" | "settings" | "timer" | "storage"))
        {
            return Err(fail("仅支持本地 v1 待办卡片和材质插件，请核对清单"));
        }
        let mut keys = std::collections::BTreeSet::new();
        if m.settings.len() > 32
            || (!m.settings.is_empty() && !m.capabilities.iter().any(|c| c == "settings"))
            || !m.settings.iter().all(|f| {
                (f.kind == "number") == material
                    && valid_field(f, &f.default)
                    && !f.label.trim().is_empty()
                    && f.label.chars().count() <= 80
                    && f.key.starts_with(|c: char| c.is_ascii_lowercase())
                    && f.key.len() <= 40
                    && f.key.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
                    })
                    && keys.insert(&f.key)
            })
        {
            return Err(fail("设置项名称、类型、默认值或显示格式无效"));
        }
        let entry = Path::new(&m.entry);
        let allowed_name = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        };
        if !m
            .entry
            .strip_suffix(if material { ".wgsl" } else { ".js" })
            .is_some_and(|name| name.split('/').all(allowed_name))
            || !entry
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        {
            return Err(fail("插件入口文件的类型或路径不符合清单要求"));
        }
        if m.cover.as_deref().is_some_and(|cover| cover != COVER_FILE) {
            return Err(fail("清单 cover 必须是根目录文件 cover.png"));
        }
        Ok(())
    }
}

fn validate_material_files(dir: &Path, manifest: &Path, shader: &Path) -> Result<()> {
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for item in fs::read_dir(folder)? {
            let item = item?;
            let path = item.path();
            let kind = item.file_type()?;
            if kind.is_symlink() || path.canonicalize()? != path {
                return Err(fail("材质包内不能包含链接或重定向路径"));
            }
            if kind.is_dir() {
                folders.push(path);
                continue;
            }
            let description = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    matches!(ext.to_ascii_lowercase().as_str(), "md" | "txt" | "rst")
                })
                || path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        matches!(
                            name.to_ascii_uppercase().as_str(),
                            "README" | "LICENSE" | "COPYING" | "NOTICE" | "CHANGELOG"
                        )
                    });
            if !kind.is_file()
                || (path != shader
                    && path != manifest
                    && path != dir.join(COVER_FILE)
                    && !description)
            {
                return Err(fail(
                    "材质包只能包含 plugin.json、一个 WGSL 入口、cover.png 和说明文件",
                ));
            }
        }
    }
    Ok(())
}

impl Package {
    fn read(path: &Path) -> Result<Self> {
        let manifest = Manifest::decode(&bounded(path, 32 * 1024)?)?;
        let entry = Path::new(&manifest.entry);
        let base = path
            .parent()
            .ok_or_else(|| fail("缺少包目录"))?
            .canonicalize()?;
        let cover = CoverPng::read(&base.join(COVER_FILE))?;
        let resolved = base.join(entry).canonicalize()?;
        if !resolved.starts_with(&base) || !resolved.is_file() {
            return Err(fail("入口越出插件目录"));
        }
        if manifest.is_material() {
            if path.file_name().and_then(|name| name.to_str()) != Some("plugin.json") {
                return Err(fail("材质清单必须命名为 plugin.json"));
            }
            validate_material_files(&base, &base.join("plugin.json"), &resolved)?;
        }
        let source = bounded(&resolved, manifest.source_limit())?;
        if source.trim().is_empty() {
            return Err(fail("插件入口文件为空"));
        }
        Ok(Self {
            source,
            manifest,
            cover: Some(cover),
        })
    }

    fn cover_png(&self) -> Option<Vec<u8>> {
        self.cover.as_ref().map(|cover| cover.bytes().to_vec())
    }

    fn settings_values(&self, previous: &Values) -> Values {
        self.manifest
            .settings
            .iter()
            .map(|field| {
                let value = previous
                    .get(&field.key)
                    .filter(|value| valid_field(field, value))
                    .unwrap_or(&field.default);
                (field.key.clone(), value.clone())
            })
            .collect()
    }

    fn render(&self, values: &Values, storage: &Value, today: &str) -> Result<Output> {
        let runtime = rquickjs::Runtime::new()?;
        runtime.set_memory_limit(4 * 1024 * 1024);
        runtime.set_max_stack_size(128 * 1024);
        let end = Instant::now() + Duration::from_millis(30);
        runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= end)));
        let ctx = rquickjs::Context::full(&runtime)?;
        let input = json!({"settings":values,"storage":storage,"today":today}).to_string();
        let raw = ctx.with(|c| -> rquickjs::Result<String> {
            c.eval::<(), _>(self.source.as_bytes())?;
            c.globals().set("__input", input)?;
            c.eval("JSON.stringify(refresh(JSON.parse(__input)))")
        })?;
        if raw.len() > 16 * 1024 {
            return Err(fail("插件输出超过 16 KiB"));
        }
        let output: Output = serde_json::from_str(&raw)?;
        if output.card.trim().is_empty() || output.card.chars().count() > 160 {
            return Err(fail("卡片文字需要 1 至 160 字"));
        }
        if !output.storage.is_null() && !self.manifest.capabilities.iter().any(|s| s == "storage") {
            return Err(fail("未声明私有存储"));
        }
        Ok(output)
    }
}

// 后台先写好临时快照。只有界面仍接受这次结果，才替换正式记录。
struct Staged(PathBuf);
impl Drop for Staged {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
struct Rendered {
    output: Output,
    staged: Option<Staged>,
}
struct Job {
    valid: bool,
    result: std::thread::JoinHandle<std::result::Result<Rendered, String>>,
}
pub struct LocalPlugins {
    jobs: BTreeMap<String, Job>,
    #[cfg(windows)]
    market_catalog: Option<std::sync::Arc<market::Catalog>>,
    #[cfg(windows)]
    official_market: market_state::OfficialMarket,
    #[cfg(windows)]
    market_job: Option<market_state::MarketJob>,
    invalid: std::collections::BTreeSet<String>,
    dir: PathBuf,
    records: BTreeMap<String, Record>,
    cards: BTreeMap<String, String>,
    next: BTreeMap<String, Instant>,
    errors: BTreeMap<String, String>,
    notice: String,
    pending: Option<String>,
    focus: Option<String>,
    drafts: BTreeMap<String, Values>,
    card_index: usize,
    #[cfg(feature = "sdk-material")]
    selected_material: Option<String>,
}
impl LocalPlugins {
    pub fn open(dir: PathBuf) -> Self {
        let mut s = Self {
            jobs: BTreeMap::new(),
            #[cfg(windows)]
            market_catalog: None,
            #[cfg(windows)]
            official_market: Default::default(),
            #[cfg(windows)]
            market_job: None,
            invalid: Default::default(),
            #[cfg(feature = "sdk-material")]
            selected_material: Self::read_material_id(&dir),
            dir,
            records: BTreeMap::new(),
            cards: BTreeMap::new(),
            next: BTreeMap::new(),
            errors: BTreeMap::new(),
            notice: String::new(),
            pending: None,
            focus: None,
            drafts: BTreeMap::new(),
            card_index: 0,
        };
        let read = (|| -> Result<()> {
            fs::create_dir_all(&s.dir)?;
            for item in fs::read_dir(&s.dir)? {
                let p = item?.path();
                if p.extension().and_then(|v| v.to_str()) != Some("json") {
                    continue;
                }
                let id = p
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .and_then(|s| s.strip_prefix("local-"))
                    .unwrap_or_default();
                if !valid_id(id) {
                    continue;
                }
                let parsed = bounded(&p, 2 * 1024 * 1024)
                    .ok()
                    .and_then(|v| serde_json::from_str::<Record>(&v).ok());
                let valid = parsed.as_ref().is_some_and(|r| r.valid(id));
                if valid {
                    s.records.insert(id.into(), parsed.unwrap());
                } else {
                    s.invalid.insert(id.into());
                }
            }
            Ok(())
        })();
        if let Err(e) = read {
            s.notice = e.to_string()
        }
        #[cfg(windows)]
        s.restore_market_catalog();
        s
    }
    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("local-{id}.json"))
    }
    fn put(&mut self, id: &str, r: Record) -> Result<()> {
        crate::model::atomic_write(&self.path(id), &serde_json::to_vec(&r)?)?;
        self.records.insert(id.into(), r);
        #[cfg(windows)]
        self.cancel_market(id);
        Ok(())
    }

    #[cfg(feature = "sdk-material")]
    fn read_material_id(dir: &Path) -> Option<String> {
        let text = bounded(&dir.join("active-material.json"), 128).ok()?;
        serde_json::from_str::<Option<String>>(&text)
            .ok()?
            .filter(|id| valid_id(id))
    }

    #[cfg(feature = "sdk-material")]
    pub fn startup_material(dir: &Path) -> bool {
        Self::read_material_id(dir).is_some_and(|id| {
            bounded(&dir.join(format!("local-{id}.json")), 2 * 1024 * 1024)
                .ok()
                .and_then(|text| serde_json::from_str::<Record>(&text).ok())
                .is_some_and(|record| {
                    record.material_granted
                        && record.material_package().is_some()
                        && record.valid(&id)
                })
        })
    }

    #[cfg(feature = "sdk-material")]
    pub fn selected_material_id(&self) -> Option<&str> {
        self.selected_material
            .as_deref()
            .filter(|id| self.material_permission_granted(id))
    }

    #[cfg(feature = "sdk-material")]
    pub fn material_permission_granted(&self, id: &str) -> bool {
        self.records
            .get(id)
            .is_some_and(|record| record.material_granted && record.material_package().is_some())
    }

    #[cfg(feature = "sdk-material")]
    pub fn activate_material(&mut self, id: Option<&str>, grant: bool) -> Result<()> {
        if let Some(id) = id {
            let mut record = self.installed(id)?;
            if record.material_package().is_none() {
                return Err(fail("所选插件不提供 GPU 材质"));
            }
            if !record.material_granted {
                if !grant {
                    return Err(fail("请先确认读取岛体背后屏幕画面的权限"));
                }
                record.material_granted = true;
                self.put(id, record)?;
            }
        }
        if self.selected_material.as_deref() != id {
            crate::model::atomic_write(
                &self.dir.join("active-material.json"),
                &serde_json::to_vec(&id)?,
            )?;
            self.selected_material = id.map(str::to_owned);
        }
        Ok(())
    }

    #[cfg(feature = "sdk-material")]
    pub fn active_material(&self) -> Option<MaterialSpec<'_>> {
        let id = self.selected_material_id()?;
        let record = self.records.get(id)?;
        let package = record.material_package()?;
        let material = package.manifest.material.as_ref()?;
        let mut params = [0.0; 32];
        for (param, field) in params.iter_mut().zip(&package.manifest.settings) {
            *param = record.values.get(&field.key)?.parse().ok()?;
        }
        Some(MaterialSpec {
            id,
            source: &package.source,
            controls: material.controls,
            stiffness: material.spring.stiffness,
            damping: material.spring.damping,
            shadow: material.shadow,
            glass_foreground: matches!(material.foreground, MaterialForeground::Glass),
            params,
        })
    }

    #[cfg(feature = "sdk-material")]
    pub fn set_material_parameter(&mut self, id: &str, key: &str, value: f32) -> Result<()> {
        let mut record = self.installed(id)?;
        let field = record
            .material_package()
            .and_then(|package| {
                package
                    .manifest
                    .settings
                    .iter()
                    .find(|field| field.key == key)
            })
            .ok_or_else(|| fail("找不到该材质参数"))?;
        let value = value.to_string();
        if !valid_field(field, &value) {
            return Err(fail("材质参数超出有效范围"));
        }
        if record.values.get(key) != Some(&value) {
            record.values.insert(key.to_owned(), value);
            self.put(id, record)?;
        }
        Ok(())
    }
    fn forget_view(&mut self, id: &str) {
        if let Some(job) = self.jobs.get_mut(id) {
            job.valid = false;
        }
        self.cards.remove(id);
        self.next.remove(id);
        self.errors.remove(id);
        self.drafts.remove(id);
    }
    pub(crate) fn install(&mut self, path: &Path) -> Result<()> {
        let path = path.canonicalize()?;
        let pkg = Package::read(&path)?;
        if !cfg!(all(windows, feature = "sdk-material")) && pkg.manifest.is_material() {
            return Err(fail("当前版本未启用 Windows GPU 材质支持"));
        }
        let id = pkg.manifest.id.clone();
        let mut record = Record {
            origin: path.clone(),
            from_market: false,
            official_market: false,
            values: Values::new(),
            enabled: false,
            package: None,
            storage: Value::Null,
            material_granted: false,
        };
        if let Some(old) = self.records.get(&id) {
            if old.package.is_some() {
                return Err(fail("此插件 ID 已安装"));
            }
            if old.origin != path || old.from_market {
                return Err(fail("保留数据属于原本地清单，请从原位置重装"));
            }
            record.storage = old.storage.clone();
            record.values = old.values.clone();
        } else if self.path(&id).exists() && !self.invalid.contains(&id) {
            return Err(fail("已有无法读取的插件记录"));
        }
        record.values = pkg.settings_values(&record.values);
        record.package = Some(pkg);
        self.put(&id, record)?;
        self.invalid.remove(&id);
        Ok(())
    }
    fn installed(&self, id: &str) -> Result<Record> {
        self.records
            .get(id)
            .filter(|r| r.package.is_some())
            .cloned()
            .ok_or_else(|| fail("插件未安装"))
    }
    pub(crate) fn enable(&mut self, id: &str, on: bool) -> Result<()> {
        let mut r = self.installed(id)?;
        if r.package.as_ref().unwrap().manifest.is_material() {
            #[cfg(feature = "sdk-material")]
            if on || self.selected_material_id() == Some(id) {
                return self.activate_material(on.then_some(id), false);
            } else {
                return Ok(());
            }
            #[cfg(not(feature = "sdk-material"))]
            return Err(fail("当前版本未启用 GPU 材质支持"));
        }
        r.enabled = on;
        self.put(id, r)?;
        self.forget_view(id);
        Ok(())
    }
    fn save(&mut self, id: &str, v: Values) -> Result<()> {
        let mut r = self.installed(id)?;
        if !r
            .package
            .as_ref()
            .unwrap()
            .manifest
            .settings
            .iter()
            .all(|f| v.get(&f.key).is_some_and(|s| valid_field(f, s)))
        {
            return Err(fail("请检查文本和日期格式 YYYY-MM-DD"));
        }
        r.values = v;
        self.put(id, r)?;
        self.forget_view(id);
        Ok(())
    }
    fn uninstall(&mut self, id: &str, keep: bool) -> Result<()> {
        let mut r = self.installed(id)?;
        #[cfg(feature = "sdk-material")]
        if self.selected_material.as_deref() == Some(id) {
            self.activate_material(None, false)?;
        }
        if keep {
            r.package = None;
            r.enabled = false;
            r.material_granted = false;
            self.put(id, r)?
        } else {
            fs::remove_file(self.path(id))?;
            self.records.remove(id);
            #[cfg(windows)]
            self.cancel_market(id);
        }
        self.forget_view(id);
        Ok(())
    }
    pub fn tick(&mut self, owner: bool, today: &str) {
        #[cfg(windows)]
        self.poll_market();
        if !owner {
            self.cards.clear();
            self.next.clear();
            for job in self.jobs.values_mut() {
                job.valid = false;
            }
        }
        let finished: Vec<_> = self
            .jobs
            .iter()
            .filter(|(_, j)| j.result.is_finished())
            .map(|(id, _)| id.clone())
            .collect();
        for id in finished {
            let job = self.jobs.remove(&id).unwrap();
            if !job.valid {
                continue;
            }
            let result = job
                .result
                .join()
                .unwrap_or_else(|_| Err("插件线程退出".into()))
                .and_then(|rendered| {
                    if let Some(staged) = rendered.staged {
                        crate::model::replace_file(&staged.0, &self.path(&id))
                            .map_err(|e| e.to_string())?;
                        self.records.get_mut(&id).unwrap().storage = rendered.output.storage;
                    }
                    Ok(rendered.output.card)
                });
            match result {
                Ok(card) => {
                    self.errors.remove(&id);
                    self.cards.insert(id, card);
                }
                Err(e) => {
                    self.cards.remove(&id);
                    self.errors.insert(id, e);
                }
            }
        }
        if !owner || self.jobs.len() >= 2 {
            return;
        }
        let now = Instant::now();
        let due: Vec<_> = self
            .records
            .iter()
            .filter(|(id, r)| {
                !self.jobs.contains_key(*id)
                    && r.enabled
                    && r.package.as_ref().is_some_and(|pkg| {
                        !pkg.manifest.is_material()
                            && self.next.get(*id).is_none_or(|t| {
                                *t <= now && pkg.manifest.capabilities.iter().any(|c| c == "timer")
                            })
                    })
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in due.into_iter().take(2 - self.jobs.len()) {
            let mut r = self.records[&id].clone();
            let today = today.to_owned();
            let pkg = r.package.as_ref().unwrap();
            self.next.insert(
                id.clone(),
                now + Duration::from_secs(pkg.manifest.refresh_seconds.into()),
            );
            static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let stage = self
                .path(&id)
                .with_extension(format!("{}-{seq}.pending", std::process::id()));
            let result = std::thread::spawn(move || {
                let output = r
                    .package
                    .as_ref()
                    .unwrap()
                    .render(&r.values, &r.storage, &today)
                    .map_err(|e| e.to_string())?;
                let staged = if r.storage != output.storage {
                    r.storage = output.storage.clone();
                    let data = serde_json::to_vec(&r).map_err(|e| e.to_string())?;
                    crate::model::atomic_write(&stage, &data).map_err(|e| e.to_string())?;
                    Some(Staged(stage))
                } else {
                    None
                };
                Ok(Rendered { output, staged })
            });
            self.jobs.insert(
                id,
                Job {
                    valid: true,
                    result,
                },
            );
        }
    }
    pub fn has_card(&self) -> bool {
        !self.cards.is_empty()
    }
}

#[cfg(test)]
#[path = "local_management_test.rs"]
mod management_test;
#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
