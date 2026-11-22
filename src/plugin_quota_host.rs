//! 本地 JS 额度插件的安装、授权和后台执行。
#[path = "plugin_quota_source.rs"]
mod source;

use crate::quota_plugin::PluginQuota;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
pub(crate) use source::{SourceGrant, SourceKind};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Cursor, Read},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const COVER_FILE: &str = "cover.png";
const COVER_LIMIT: usize = 256 * 1024;
const RECORD_LIMIT: usize = 2 * 1024 * 1024;
const SCRIPT_LIMIT: usize = 256 * 1024;
const SOURCE_PERMISSION: &str = "source.read";

type Values = BTreeMap<String, String>;
type PackageResult<T> = Result<T, Box<dyn std::error::Error>>;

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
}

impl Field {
    fn valid(&self, value: &str) -> bool {
        if self.min.is_some() || self.max.is_some() || self.step.is_some() {
            return false;
        }
        match self.kind.as_str() {
            "text" => !value.trim().is_empty() && value.chars().count() <= 80,
            "date" => chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .is_ok_and(|date| date.to_string() == value),
            _ => false,
        }
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cover: Option<String>,
    owner: String,
    capabilities: Vec<String>,
    data_version: u32,
    permissions: BTreeMap<String, Value>,
    settings: Vec<Field>,
    #[serde(default = "default_refresh")]
    refresh_seconds: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material: Option<Value>,
}

fn default_refresh() -> u32 {
    60
}

fn valid_id(id: &str) -> bool {
    id.len() <= 64
        && id.contains('.')
        && id.split('.').all(|part| {
            part.starts_with(|c: char| c.is_ascii_lowercase())
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

impl Manifest {
    fn source_kind(&self) -> Option<SourceKind> {
        if !cfg!(windows) || self.permissions.len() != 1 {
            return None;
        }
        match self.permissions.get(SOURCE_PERMISSION)?.as_str()? {
            "file" => Some(SourceKind::File),
            "loopback" => Some(SourceKind::Loopback),
            _ => None,
        }
    }

    fn has(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|value| value == capability)
    }

    fn validate(&self) -> PackageResult<()> {
        if self.api > 1 {
            return Err(format!("需要更新岛仔。此插件需要 API v{}，当前支持 v1", self.api).into());
        }
        if self.api != 1
            || self.data_version != 1
            || !valid_id(&self.id)
            || self.name.trim().is_empty()
            || self.name.chars().count() > 80
            || self.author.trim().is_empty()
            || self.author.chars().count() > 80
            || self.version.split('.').count() != 3
            || !self
                .version
                .split('.')
                .all(|v| v.parse::<u32>().is_ok_and(|n| n.to_string() == v))
            || !(60..=86400).contains(&self.refresh_seconds)
        {
            return Err("请核对插件 ID、名称、作者、版本和刷新间隔".into());
        }
        if self.owner != "quota"
            || self.runtime != "js"
            || self.material.is_some()
            || (!self.permissions.is_empty() && self.source_kind().is_none())
            || self
                .capabilities
                .iter()
                .filter(|cap| cap.as_str() == "quota.source")
                .count()
                != 1
            || !self.capabilities.iter().all(|cap| {
                matches!(
                    cap.as_str(),
                    "quota.source" | "settings" | "timer" | "storage"
                )
            })
        {
            return Err("AI 额度插件需使用 quota 归属、JS 入口与 quota.source 能力".into());
        }
        let mut keys = BTreeSet::new();
        if self.settings.len() > 32
            || (!self.settings.is_empty() && !self.has("settings"))
            || !self.settings.iter().all(|field| {
                field.valid(&field.default)
                    && !field.label.trim().is_empty()
                    && field.label.chars().count() <= 80
                    && field.key.starts_with(|c: char| c.is_ascii_lowercase())
                    && field.key.len() <= 40
                    && field.key.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'-' | b'_')
                    })
                    && keys.insert(&field.key)
            })
        {
            return Err("设置项名称、类型或默认值无效".into());
        }
        if !self.entry.strip_suffix(".js").is_some_and(|entry| {
            entry.split('/').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
        }) || !Path::new(&self.entry)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err("脚本入口必须位于插件目录内".into());
        }
        if self
            .cover
            .as_deref()
            .is_some_and(|cover| cover != COVER_FILE)
        {
            return Err("清单 cover 必须是根目录文件 cover.png".into());
        }
        Ok(())
    }
}

fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    }
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(io::Error::other("需要普通文件"));
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(io::Error::other("需要普通文件"));
        }
    }
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(io::Error::other("需要普通文件，且文件大小必须符合限制"));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::other("文件超过大小限制"));
    }
    Ok(bytes)
}

fn read_cover(base: &Path) -> PackageResult<String> {
    let path = base.join(COVER_FILE);
    if path.canonicalize()?.parent() != Some(base) {
        return Err("cover.png 必须位于插件根目录".into());
    }
    let bytes = read_bounded(&path, COVER_LIMIT)?;
    if !bytes.ends_with(b"\0\0\0\0IEND\xaeB\x60\x82") {
        return Err("封面 cover.png 必须是完整的 PNG 图片".into());
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(&bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1920);
    limits.max_image_height = Some(1080);
    limits.max_alloc = Some(24 * 1024 * 1024);
    reader.limits(limits);
    reader.decode()?;
    Ok(STANDARD.encode(bytes))
}

#[derive(Clone, Serialize, Deserialize)]
struct Package {
    manifest: Manifest,
    source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cover: Option<String>,
}

impl Package {
    fn read(path: &Path) -> PackageResult<Self> {
        let manifest: Manifest = serde_json::from_slice(&read_bounded(path, 32 * 1024)?)?;
        manifest.validate()?;
        if manifest.cover.is_none() {
            return Err("请在插件清单中声明 cover: \"cover.png\"".into());
        }
        let base = path.parent().ok_or("缺少插件根目录")?;
        let entry = base.join(&manifest.entry).canonicalize()?;
        if !entry.starts_with(base) {
            return Err("脚本入口超出插件目录".into());
        }
        let source = String::from_utf8(read_bounded(&entry, SCRIPT_LIMIT)?)?;
        if source.trim().is_empty() {
            return Err("插件入口文件为空".into());
        }
        Ok(Self {
            manifest,
            source,
            cover: Some(read_cover(base)?),
        })
    }

    fn render(
        &self,
        values: &Values,
        storage: &Value,
        source: SourceInput,
    ) -> PackageResult<Output> {
        let mut input = json!({
            "settings": values,
            "storage": storage,
            "today": chrono::Local::now().date_naive().to_string()
        });
        let requires_source = self.manifest.source_kind().is_some();
        let ready = !requires_source || matches!(&source, SourceInput::Ready { .. });
        if requires_source {
            input["source"] = serde_json::to_value(source)?;
        }
        let input = input.to_string();

        let runtime = rquickjs::Runtime::new()?;
        runtime.set_memory_limit(4 * 1024 * 1024);
        runtime.set_max_stack_size(128 * 1024);
        let context = rquickjs::Context::full(&runtime)?;
        let end = Instant::now() + Duration::from_millis(30);
        runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= end)));
        let raw = context.with(|ctx| -> rquickjs::Result<String> {
            ctx.eval::<(), _>(self.source.as_bytes())?;
            ctx.globals().set("__input", input)?;
            ctx.eval("JSON.stringify(refresh(JSON.parse(__input)))")
        })?;
        if raw.len() > 16 * 1024 {
            return Err("插件输出超过 16 KiB".into());
        }
        let mut output: Output = serde_json::from_str(&raw)?;
        if !output.card.is_empty() || output.quota.as_ref().is_some_and(|quota| !quota.valid()) {
            return Err("额度来源需要返回格式有效的 quota 或 null".into());
        }
        if !output.storage.is_null() && !self.manifest.has("storage") {
            return Err("未声明私有存储".into());
        }
        if !ready {
            output.quota = None;
        }
        Ok(output)
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Record {
    origin: PathBuf,
    package: Package,
    enabled: bool,
    values: Values,
    storage: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_grant: Option<SourceGrant>,
    #[serde(flatten)]
    metadata: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedRecord {
    origin: PathBuf,
    package: Value,
    enabled: bool,
    values: Values,
    storage: Value,
    #[serde(default)]
    source_grant: Option<SourceGrant>,
    #[serde(default)]
    material_granted: bool,
    #[serde(default)]
    from_market: bool,
    #[serde(default)]
    official_market: bool,
    #[serde(default)]
    presentation: Option<Value>,
}

impl RetainedRecord {
    fn matches(&self, origin: &Path) -> bool {
        self.package.is_null()
            && !self.enabled
            && self.source_grant.is_none()
            && !self.material_granted
            && !self.from_market
            && !self.official_market
            && self.presentation.is_none()
            && self.origin.as_path() == origin
    }
}

impl Record {
    fn valid(&self, id: &str) -> bool {
        let package = &self.package;
        package.manifest.id == id
            && package.manifest.validate().is_ok()
            && !package.source.trim().is_empty()
            && package.source.len() <= SCRIPT_LIMIT
            && package.manifest.cover.is_some() == package.cover.is_some()
            && package
                .cover
                .as_ref()
                .is_none_or(|cover| cover.len() <= COVER_LIMIT.div_ceil(3) * 4)
            && package.manifest.settings.iter().all(|field| {
                self.values
                    .get(&field.key)
                    .is_some_and(|value| field.valid(value))
            })
    }

    fn source_grant(&self) -> Option<&SourceGrant> {
        self.source_grant
            .as_ref()
            .filter(|grant| Some(grant.kind()) == self.package.manifest.source_kind())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Output {
    #[serde(default)]
    card: String,
    #[serde(default)]
    quota: Option<PluginQuota>,
    #[serde(default)]
    storage: Value,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum SourceInput {
    Unconfigured,
    Ready { text: String },
    Offline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PluginQuotaStatus {
    Disabled,
    Waiting,
    Unconfigured,
    Offline,
    Ready,
    Error,
}

pub(crate) struct PluginQuotaRecord<'a> {
    pub(crate) id: &'a str,
    pub(crate) name: &'a str,
    pub(crate) enabled: bool,
    pub(crate) source_kind: Option<SourceKind>,
    pub(crate) source_grant: Option<&'a SourceGrant>,
    pub(crate) status: PluginQuotaStatus,
}

// 后台准备私有存储快照。对于已取消的任务，只移除临时文件。
struct Staged(PathBuf);

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

struct Rendered {
    output: Output,
    status: PluginQuotaStatus,
    staged: Option<Staged>,
}

struct Job {
    cancelled: Arc<AtomicBool>,
    reading_source: bool,
    result: JoinHandle<Result<Rendered, PluginQuotaStatus>>,
}

pub(crate) struct PluginQuotaHost {
    dir: PathBuf,
    records: BTreeMap<String, Record>,
    jobs: BTreeMap<String, Job>,
    quota: BTreeMap<String, PluginQuota>,
    next: BTreeMap<String, Instant>,
    status: BTreeMap<String, PluginQuotaStatus>,
}

impl PluginQuotaHost {
    pub(crate) fn load() -> Self {
        Self::open(crate::model::config_dir().join("local-plugins"))
    }

    pub(crate) fn open(dir: PathBuf) -> Self {
        let mut host = Self {
            dir,
            records: BTreeMap::new(),
            jobs: BTreeMap::new(),
            quota: BTreeMap::new(),
            next: BTreeMap::new(),
            status: BTreeMap::new(),
        };
        if let Ok(entries) = fs::read_dir(&host.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }
                let Some(id) = path
                    .file_stem()
                    .and_then(|stem| stem.to_str()?.strip_prefix("local-"))
                    .filter(|id| valid_id(id))
                else {
                    continue;
                };
                if let Ok(bytes) = read_bounded(&path, RECORD_LIMIT) {
                    if let Ok(record) = serde_json::from_slice::<Record>(&bytes) {
                        if record.valid(id) {
                            host.records.insert(id.into(), record);
                        }
                    }
                }
            }
        }
        host
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("local-{id}.json"))
    }

    fn save(&mut self, id: &str, record: Record) -> Result<(), String> {
        let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
        if bytes.len() > RECORD_LIMIT {
            return Err("插件记录超过 2 MiB".into());
        }
        fs::create_dir_all(&self.dir).map_err(|error| error.to_string())?;
        crate::model::atomic_write(&self.path(id), &bytes).map_err(|error| error.to_string())?;
        self.records.insert(id.to_owned(), record);
        Ok(())
    }

    pub(crate) fn install(&mut self, path: &Path) -> Result<String, String> {
        let origin = path.canonicalize().map_err(|error| error.to_string())?;
        let package = Package::read(&origin).map_err(|error| error.to_string())?;
        let id = package.manifest.id.clone();
        if self.records.contains_key(&id) {
            return Err("此插件 ID 已安装或已被其他插件使用".into());
        }
        let retained = self.retained_record(&id, &origin)?;
        let values = package
            .manifest
            .settings
            .iter()
            .map(|field| {
                let value = retained
                    .as_ref()
                    .and_then(|record| record.values.get(&field.key))
                    .filter(|value| field.valid(value))
                    .unwrap_or(&field.default);
                (field.key.clone(), value.clone())
            })
            .collect();
        let storage = retained
            .filter(|_| package.manifest.has("storage"))
            .map_or(Value::Null, |record| record.storage);
        self.save(
            &id,
            Record {
                origin,
                package,
                enabled: false,
                values,
                storage,
                source_grant: None,
                metadata: BTreeMap::new(),
            },
        )?;
        Ok(id)
    }

    fn retained_record(&self, id: &str, origin: &Path) -> Result<Option<RetainedRecord>, String> {
        let path = self.path(id);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            _ => {}
        }
        // 仅认领验收版从同一清单位置卸载后留下的无授权本地记录。
        read_bounded(&path, RECORD_LIMIT)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<RetainedRecord>(&bytes).ok())
            .filter(|record| record.matches(origin))
            .map(Some)
            .ok_or_else(|| "此插件 ID 已安装，或保留数据属于其他清单".into())
    }

    fn installed(&self, id: &str) -> Result<Record, String> {
        self.records
            .get(id)
            .cloned()
            .ok_or_else(|| "插件未安装".into())
    }

    pub(crate) fn enable(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        let mut record = self.installed(id)?;
        if record.enabled != enabled {
            record.enabled = enabled;
            self.save(id, record)?;
            self.clear_view(id);
        }
        Ok(())
    }

    pub(crate) fn forget(&mut self, id: &str) -> Result<(), String> {
        if !self.records.contains_key(id) {
            return Err("插件未安装".into());
        }
        match fs::remove_file(self.path(id)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        self.records.remove(id);
        self.clear_view(id);
        Ok(())
    }

    pub(crate) fn grant_file(&mut self, id: &str, path: &Path) -> Result<(), String> {
        self.set_source_grant(id, Some(source::grant_file(path)?))
    }

    pub(crate) fn grant_url(&mut self, id: &str, url: &str) -> Result<(), String> {
        self.set_source_grant(id, Some(source::grant_url(url)?))
    }

    pub(crate) fn revoke(&mut self, id: &str) -> Result<(), String> {
        self.set_source_grant(id, None)
    }

    fn set_source_grant(&mut self, id: &str, grant: Option<SourceGrant>) -> Result<(), String> {
        let mut record = self.installed(id)?;
        let kind = record
            .package
            .manifest
            .source_kind()
            .ok_or("此插件未声明数据源")?;
        if grant.as_ref().is_some_and(|grant| grant.kind() != kind) {
            return Err("数据源类型与插件声明不一致".into());
        }
        if record.source_grant != grant {
            record.source_grant = grant;
            self.save(id, record)?;
            self.clear_view(id);
        }
        Ok(())
    }

    fn clear_view(&mut self, id: &str) {
        if let Some(job) = self.jobs.get(id) {
            job.cancelled.store(true, Ordering::Release);
        }
        self.quota.remove(id);
        self.next.remove(id);
        self.status.remove(id);
    }

    pub(crate) fn pause(&mut self) {
        for job in self.jobs.values() {
            job.cancelled.store(true, Ordering::Release);
        }
        self.quota.clear();
        self.next.clear();
        self.status.clear();
    }

    pub(crate) fn refresh(&mut self) {
        self.next.clear();
    }

    pub(crate) fn tick(&mut self, enabled: bool) {
        if self.records.is_empty() && self.jobs.is_empty() {
            return;
        }
        if !enabled {
            self.pause();
        }
        self.finish();
        if !enabled || self.jobs.len() >= 2 {
            return;
        }
        let now = Instant::now();
        let mut due: Vec<_> = self
            .records
            .iter()
            .filter(|(id, record)| {
                record.enabled
                    && !self.jobs.contains_key(*id)
                    && self
                        .next
                        .get(*id)
                        .is_none_or(|next| *next <= now && record.package.manifest.has("timer"))
            })
            .map(|(id, record)| (id.clone(), record.source_grant().is_some()))
            .collect();
        due.sort_by_key(|(id, _)| self.next.get(id).copied());
        for (id, reading_source) in due {
            if self.jobs.len() >= 2 {
                break;
            }
            // 一个较慢的数据源只占一个后台工作位。
            if reading_source && self.jobs.values().any(|job| job.reading_source) {
                continue;
            }
            self.start(id, reading_source, now);
        }
    }

    fn start(&mut self, id: String, reading_source: bool, now: Instant) {
        let record = self.records[&id].clone();
        self.next.insert(
            id.clone(),
            now + Duration::from_secs(record.package.manifest.refresh_seconds.into()),
        );
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let stage = self
            .path(&id)
            .with_extension(format!("{}-{serial}.pending", std::process::id()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        match thread::Builder::new()
            .name("daozai-plugin-quota".into())
            .spawn(move || run(record, stage, &worker_cancelled))
        {
            Ok(result) => {
                self.jobs.insert(
                    id,
                    Job {
                        cancelled,
                        reading_source,
                        result,
                    },
                );
            }
            Err(_) => {
                self.quota.remove(&id);
                self.status.insert(id, PluginQuotaStatus::Error);
            }
        }
    }

    fn finish(&mut self) {
        let finished: Vec<_> = self
            .jobs
            .iter()
            .filter(|(_, job)| job.result.is_finished())
            .map(|(id, _)| id.clone())
            .collect();
        for id in finished {
            let job = self.jobs.remove(&id).unwrap();
            let result = job.result.join().unwrap_or(Err(PluginQuotaStatus::Error));
            if job.cancelled.load(Ordering::Acquire) {
                continue;
            }
            match result.and_then(|rendered| self.apply_result(&id, rendered)) {
                Ok(()) => {}
                Err(status) => {
                    self.quota.remove(&id);
                    self.status.insert(id, status);
                }
            }
        }
    }

    fn apply_result(&mut self, id: &str, rendered: Rendered) -> Result<(), PluginQuotaStatus> {
        if let Some(staged) = rendered.staged {
            crate::model::replace_file(&staged.0, &self.path(id))
                .map_err(|_| PluginQuotaStatus::Error)?;
            self.records.get_mut(id).unwrap().storage = rendered.output.storage;
        }
        self.status.insert(id.to_owned(), rendered.status);
        match rendered.output.quota {
            Some(quota) => {
                self.quota.insert(id.to_owned(), quota);
            }
            None => {
                self.quota.remove(id);
            }
        }
        Ok(())
    }

    pub(crate) fn quota_sources(&self) -> impl Iterator<Item = (&str, &str, &PluginQuota)> {
        self.quota.iter().filter_map(|(id, quota)| {
            let record = self.records.get(id).filter(|record| record.enabled)?;
            Some((id.as_str(), record.package.manifest.name.as_str(), quota))
        })
    }

    pub(crate) fn records(&self) -> impl Iterator<Item = PluginQuotaRecord<'_>> {
        self.records.iter().map(|(id, record)| {
            let source_kind = record.package.manifest.source_kind();
            let source_grant = record.source_grant();
            let status = if !record.enabled {
                PluginQuotaStatus::Disabled
            } else if source_kind.is_some() && source_grant.is_none() {
                PluginQuotaStatus::Unconfigured
            } else {
                self.status
                    .get(id)
                    .copied()
                    .unwrap_or(PluginQuotaStatus::Waiting)
            };
            PluginQuotaRecord {
                id,
                name: &record.package.manifest.name,
                enabled: record.enabled,
                source_kind,
                source_grant,
                status,
            }
        })
    }
}

impl Drop for PluginQuotaHost {
    fn drop(&mut self) {
        for job in self.jobs.values() {
            job.cancelled.store(true, Ordering::Release);
        }
    }
}

fn run(
    mut record: Record,
    path: PathBuf,
    cancelled: &AtomicBool,
) -> Result<Rendered, PluginQuotaStatus> {
    let source = match record.source_grant() {
        Some(grant) => match grant.read(cancelled) {
            Ok(text) => SourceInput::Ready { text },
            Err(_) => SourceInput::Offline,
        },
        None => SourceInput::Unconfigured,
    };
    if cancelled.load(Ordering::Acquire) {
        return Err(PluginQuotaStatus::Error);
    }
    let unavailable = match &source {
        SourceInput::Offline => Some(PluginQuotaStatus::Offline),
        SourceInput::Unconfigured if record.package.manifest.source_kind().is_some() => {
            Some(PluginQuotaStatus::Unconfigured)
        }
        _ => None,
    };
    let output = record
        .package
        .render(&record.values, &record.storage, source)
        .map_err(|_| unavailable.unwrap_or(PluginQuotaStatus::Error))?;
    if cancelled.load(Ordering::Acquire) {
        return Err(PluginQuotaStatus::Error);
    }
    let status = unavailable.unwrap_or(if output.quota.is_some() {
        PluginQuotaStatus::Ready
    } else {
        PluginQuotaStatus::Waiting
    });
    let staged = if record.storage != output.storage {
        record.storage = output.storage.clone();
        let bytes = serde_json::to_vec(&record).map_err(|_| PluginQuotaStatus::Error)?;
        if bytes.len() > RECORD_LIMIT {
            return Err(PluginQuotaStatus::Error);
        }
        crate::model::atomic_write(&path, &bytes).map_err(|_| PluginQuotaStatus::Error)?;
        Some(Staged(path))
    } else {
        None
    };
    Ok(Rendered {
        output,
        status,
        staged,
    })
}

#[cfg(test)]
#[path = "plugin_quota_host_tests.rs"]
mod tests;
