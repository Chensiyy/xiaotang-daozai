//! 本地开发包的安装、状态及卡片执行。每次操作原子保存一个插件的快照。
#[path = "local_ui.rs"]
mod ui;
use super::{fail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    key: String,
    label: String,
    #[serde(rename = "type")]
    kind: String,
    default: String,
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
    owner: String,
    capabilities: Vec<String>,
    data_version: u32,
    permissions: BTreeMap<String, Value>,
    settings: Vec<Field>,
}
type Values = BTreeMap<String, String>;
#[derive(Clone, Serialize, Deserialize)]
struct Package {
    manifest: Manifest,
    source: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Record {
    origin: PathBuf,
    package: Option<Package>,
    enabled: bool,
    values: Values,
    storage: Value,
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
    match f.kind.as_str() {
        "text" => !v.trim().is_empty() && v.chars().count() <= 80,
        "date" => {
            chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").is_ok_and(|d| d.to_string() == v)
        }
        _ => false,
    }
}
impl Package {
    fn read(path: &Path) -> Result<Self> {
        let manifest: Manifest = serde_json::from_str(&bounded(path, 32 * 1024)?)?;
        let m = &manifest;
        if !valid_id(&m.id)
            || m.api != 1
            || m.data_version != 1
            || m.runtime != "js"
            || m.owner != "tasks"
            || m.name.trim().is_empty()
            || m.name.chars().count() > 80
            || m.author.trim().is_empty()
            || m.version.split('.').count() != 3
            || !m
                .version
                .split('.')
                .all(|v| v.parse::<u32>().is_ok_and(|n| n.to_string() == v))
            || !m.permissions.is_empty()
            || !m.capabilities.iter().any(|c| c == "card")
            || !m
                .capabilities
                .iter()
                .all(|c| matches!(c.as_str(), "card" | "settings" | "timer" | "storage"))
        {
            return Err(fail(
                "仅支持本地 v1 待办卡片、无外部权限的 JS 包，请核对清单",
            ));
        }
        let mut keys = std::collections::BTreeSet::new();
        if m.settings.len() > 32
            || (!m.settings.is_empty() && !m.capabilities.iter().any(|c| c == "settings"))
            || !m.settings.iter().all(|f| {
                valid_field(f, &f.default)
                    && !f.label.trim().is_empty()
                    && f.key.starts_with(|c: char| c.is_ascii_lowercase())
                    && f.key.len() <= 40
                    && f.key.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
                    })
                    && keys.insert(&f.key)
            })
        {
            return Err(fail("设置项名称、类型或默认值无效"));
        }
        let entry = Path::new(&m.entry);
        if !m.entry.ends_with(".js")
            || !entry
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        {
            return Err(fail("脚本入口必须在插件目录内"));
        }
        let base = path
            .parent()
            .ok_or_else(|| fail("缺少包目录"))?
            .canonicalize()?;
        let resolved = base.join(entry).canonicalize()?;
        if !resolved.starts_with(base) || !resolved.is_file() {
            return Err(fail("入口越出插件目录"));
        }
        Ok(Self {
            source: bounded(&resolved, 256 * 1024)?,
            manifest,
        })
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

pub struct LocalPlugins {
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
}
impl LocalPlugins {
    pub fn open(dir: PathBuf) -> Self {
        let mut s = Self {
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
                match fs::read(&p)
                    .ok()
                    .and_then(|v| serde_json::from_slice::<Record>(&v).ok())
                {
                    Some(r)
                        if valid_id(id)
                            && r.package.as_ref().is_none_or(|p| p.manifest.id == id) =>
                    {
                        s.records.insert(id.into(), r);
                    }
                    _ => s.notice = format!("无法读取插件记录 {id}"),
                }
            }
            Ok(())
        })();
        if let Err(e) = read {
            s.notice = e.to_string()
        }
        s
    }
    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("local-{id}.json"))
    }
    fn put(&mut self, id: &str, r: Record) -> Result<()> {
        crate::model::atomic_write(&self.path(id), &serde_json::to_vec(&r)?)?;
        self.records.insert(id.into(), r);
        Ok(())
    }
    fn forget_view(&mut self, id: &str) {
        self.cards.remove(id);
        self.next.remove(id);
        self.errors.remove(id);
        self.drafts.remove(id);
    }
    pub(crate) fn install(&mut self, path: &Path) -> Result<()> {
        let path = path.canonicalize()?;
        let pkg = Package::read(&path)?;
        let id = pkg.manifest.id.clone();
        let mut record = Record {
            origin: path.clone(),
            values: Values::new(),
            enabled: false,
            package: None,
            storage: Value::Null,
        };
        if let Some(old) = self.records.get(&id) {
            if old.package.is_some() {
                return Err(fail("此插件 ID 已安装"));
            }
            if old.origin != path {
                return Err(fail("保留数据属于原本地清单，请从原位置重装"));
            }
            record.storage = old.storage.clone();
            record.values = old.values.clone();
        } else if self.path(&id).exists() {
            return Err(fail("已有无法读取的插件记录"));
        }
        let mut values = Values::new();
        for f in &pkg.manifest.settings {
            values.insert(
                f.key.clone(),
                record
                    .values
                    .get(&f.key)
                    .filter(|v| valid_field(f, v))
                    .unwrap_or(&f.default)
                    .clone(),
            );
        }
        record.values = values;
        record.package = Some(pkg);
        self.put(&id, record)
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
        if keep {
            r.package = None;
            r.enabled = false;
            self.put(id, r)?
        } else {
            fs::remove_file(self.path(id))?;
            self.records.remove(id);
        }
        self.forget_view(id);
        Ok(())
    }
    pub fn tick(&mut self, owner: bool, today: &str) {
        if !owner {
            self.cards.clear();
            self.next.clear();
            return;
        }
        let now = Instant::now();
        let due: Vec<_> = self
            .records
            .iter()
            .filter(|(id, r)| {
                r.enabled
                    && r.package.as_ref().is_some_and(|pkg| {
                        self.next.get(*id).is_none_or(|t| {
                            *t <= now && pkg.manifest.capabilities.iter().any(|c| c == "timer")
                        })
                    })
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            self.next.insert(id.clone(), now + Duration::from_secs(60));
            let result = (|| -> Result<String> {
                let r = &self.records[&id];
                let output = r
                    .package
                    .as_ref()
                    .unwrap()
                    .render(&r.values, &r.storage, today)?;
                if output.storage != r.storage {
                    let mut updated = r.clone();
                    updated.storage = output.storage;
                    self.put(&id, updated)?
                }
                Ok(output.card)
            })();
            match result {
                Ok(card) => {
                    self.errors.remove(&id);
                    self.cards.insert(id, card);
                }
                Err(e) => {
                    self.cards.remove(&id);
                    self.errors.insert(id, e.to_string());
                }
            }
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
