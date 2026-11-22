//! 插件集市的目录读取、作者工具与包校验。

use super::{
    bounded, cover::COVER_LIMIT, fail, valid_id, CoverPng, Manifest, Package, Result, Values,
    COVER_FILE, SCRIPT_SOURCE_LIMIT,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    cmp::Ordering,
    collections::BTreeSet,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};
use url::Url;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MANIFEST_LIMIT: u64 = 32 * 1024;
const ZIP_LIMIT: u64 = 576 * 1024;
const CATALOG_LIMIT: u64 = 64 * 1024;
const AVATAR_LIMIT: u64 = 64 * 1024;
const CATALOG_FORMAT: u32 = 1;
const GITEE_REPOSITORY: &str = "xiaotangtoken/xiaotang-daozai-plugins";

#[cfg(not(test))]
pub(super) fn official_index_url() -> Result<Url> {
    let channel = if option_env!("DAOZAI_TEST_BUILD").is_some() {
        "preview"
    } else {
        "stable"
    };
    Ok(Url::parse(&format!(
        "https://gitee.com/api/v5/repos/{GITEE_REPOSITORY}/contents/{channel}/catalog.json?ref=master"
    ))?)
}

pub(super) fn official_cache_path(dir: &Path, index: &Url) -> PathBuf {
    dir.join(format!(
        "official-market-{}.json",
        sha256(index.as_str().as_bytes())
    ))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) author: String,
    pub(super) version: String,
    pub(super) size: u64,
    pub(super) sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) presentation: Option<Presentation>,
}

impl Entry {
    fn matches(&self, manifest: &Manifest) -> bool {
        self.id == manifest.id
            && self.version == manifest.version
            && self.name == manifest.name
            && self.author == manifest.author
    }

    fn zip_name(&self) -> String {
        format!("plugin-{}-{}.zip", self.id, self.version)
    }

    fn path_in(&self, catalog: &Path) -> PathBuf {
        catalog.with_file_name(self.zip_name())
    }

    pub(super) fn avatar_name(&self) -> Option<&str> {
        self.presentation.as_ref()?.author_avatar.as_deref()
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Presentation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) updated_on: Option<String>,
    #[serde(skip_serializing_if = "super::is_false")]
    pub(super) official: bool,
    #[serde(skip_serializing_if = "super::is_false")]
    pub(super) sample: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) maintainer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) author_avatar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) downloads: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) favorites: Option<u64>,
}

impl Presentation {
    fn valid(&self) -> bool {
        self.description.as_deref().is_none_or(|description| {
            !description.trim().is_empty() && description.chars().count() <= 1000
        }) && self.updated_on.as_deref().is_none_or(|date| {
            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .is_ok_and(|parsed| parsed.to_string() == date)
        }) && self.maintainer.as_deref().is_none_or(|login| {
            (1..=39).contains(&login.len())
                && login.starts_with(|c: char| c.is_ascii_alphanumeric())
                && login.ends_with(|c: char| c.is_ascii_alphanumeric())
                && !login.contains("--")
                && login
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }) && self.author_avatar.as_deref().is_none_or(valid_avatar_name)
    }
}

fn valid_avatar_name(name: &str) -> bool {
    name.strip_prefix("avatar-")
        .and_then(|name| name.strip_suffix(".png"))
        .is_some_and(|stem| {
            (1..=64).contains(&stem.len())
                && stem
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    format: u32,
    plugins: Vec<Entry>,
}

#[derive(Clone)]
pub(super) struct Catalog {
    pub(super) path: PathBuf,
    pub(super) entries: Vec<Entry>,
    official_index: Option<Url>,
}

impl Catalog {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let path = path.canonicalize()?;
        let bytes = bounded(&path, CATALOG_LIMIT)?;
        Self::parse(path, None, bytes.as_bytes())
    }

    pub(super) fn restore_official(path: PathBuf, index: Url) -> Result<Self> {
        let bytes = bounded(&path, CATALOG_LIMIT)?;
        Self::parse(path, Some(index), bytes.as_bytes())
    }

    pub(super) fn sync_official(path: PathBuf, index: Url) -> Result<Self> {
        let bytes = download(&index, CATALOG_LIMIT)?;
        let catalog = Self::parse(path, Some(index), &bytes)?;
        crate::model::atomic_write(&catalog.path, &bytes)?;
        Ok(catalog)
    }

    pub(super) fn is_official(&self) -> bool {
        self.official_index.is_some()
    }

    fn parse(path: PathBuf, official_index: Option<Url>, bytes: &[u8]) -> Result<Self> {
        let file: CatalogFile = serde_json::from_slice(bytes)
            .map_err(|error| fail(&format!("集市目录格式无效：{error}")))?;
        if file.format != CATALOG_FORMAT {
            return Err(fail("当前岛仔不支持此集市目录版本"));
        }
        let mut seen = BTreeSet::new();
        for entry in &file.plugins {
            if !valid_id(&entry.id)
                || version(&entry.version).is_none()
                || entry.name.trim().is_empty()
                || entry.name.chars().count() > 80
                || entry.author.trim().is_empty()
                || entry.author.chars().count() > 80
                || !(1..=ZIP_LIMIT).contains(&entry.size)
                || entry.sha256.len() != 64
                || !entry
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
                || entry
                    .presentation
                    .as_ref()
                    .is_some_and(|presentation| !presentation.valid())
                || !seen.insert(&entry.id)
            {
                return Err(fail("集市目录包含无效或重复的插件条目"));
            }
        }
        Ok(Self {
            path,
            entries: file.plugins,
            official_index,
        })
    }

    pub(super) fn load(&self, entry: &Entry) -> Result<Package> {
        let raw = match &self.official_index {
            Some(index) => download_catalog_asset(index, &entry.zip_name(), ZIP_LIMIT)?,
            None => read_catalog_zip(&entry.path_in(&self.path))?,
        };
        if raw.len() as u64 != entry.size || sha256(&raw) != entry.sha256 {
            return Err(fail("插件包大小或 SHA-256 与集市目录不一致"));
        }
        let package = decode_zip(&raw)?;
        if !entry.matches(&package.manifest) {
            return Err(fail("插件清单与集市目录不一致"));
        }
        Ok(package)
    }

    pub(super) fn cover_png(&self, entry: &Entry) -> Result<Vec<u8>> {
        self.load(entry)?
            .cover_png()
            .ok_or_else(|| fail("插件 ZIP 缺少封面"))
    }

    pub(super) fn avatar_png(&self, name: &str) -> Option<CoverPng> {
        let source = self.official_index.as_ref()?;
        if !valid_avatar_name(name) {
            return None;
        }
        let bytes = download_catalog_asset(source, name, AVATAR_LIMIT).ok()?;
        CoverPng::decode(bytes).ok()
    }

    pub(super) fn has_image(&self, key: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.sha256 == key || entry.avatar_name() == Some(key))
    }
}

fn download_catalog_asset(index: &Url, name: &str, limit: u64) -> Result<Vec<u8>> {
    let mut asset = index.join(name)?;
    asset.set_query(index.query());
    download(&asset, limit)
}

fn version(value: &str) -> Option<[u32; 3]> {
    let mut segments = value.split('.');
    let mut result = [0; 3];
    for part in &mut result {
        let raw = segments.next()?;
        *part = raw.parse().ok()?;
        if part.to_string() != raw {
            return None;
        }
    }
    segments.next().is_none().then_some(result)
}

pub(super) fn compare_versions(left: &str, right: &str) -> Option<Ordering> {
    Some(version(left)?.cmp(&version(right)?))
}

fn sha256(raw: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in digest(&SHA256, raw).as_ref() {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").expect("写入字符串不会失败");
    }
    hex
}

fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(fail("集市文件超过大小限制"));
    }
    Ok(bytes)
}

fn download(url: &Url, limit: u64) -> Result<Vec<u8>> {
    let (_, path) = url
        .path()
        .split_once("/contents/")
        .ok_or_else(|| fail("集市下载地址无效"))?;
    let agent = ureq::AgentBuilder::new()
        .try_proxy_from_env(url.scheme() == "https")
        .redirects(0)
        .timeout_connect(Duration::from_secs(6))
        .timeout(Duration::from_secs(15))
        .build();
    let response = match market_request(&agent, url, "application/json").call() {
        Ok(response) => response,
        Err(ureq::Error::Status(status, response)) => {
            if gitee_rate_limited(status, response.into_reader()) {
                if let Some(raw) = gitee_raw_url(url) {
                    return download_gitee_raw(&agent, &raw, limit);
                }
            }
            return Err(fail(&format!("集市服务返回 HTTP {status}")));
        }
        Err(_) => return Err(fail("无法连接集市，请检查网络后重试")),
    };
    if response.status() != 200 {
        return Err(fail("集市下载地址发生非预期跳转"));
    }
    let encoded_limit = limit.div_ceil(3) * 4;
    // 为 JSON 元数据和 Base64 换行留出空间，读取前仍有固定上限。
    let body = read_limited(
        response.into_reader(),
        encoded_limit + encoded_limit / 8 + 16 * 1024,
    )?;
    decode_gitee_file(&body, path, limit)
}

fn market_request(agent: &ureq::Agent, url: &Url, accept: &str) -> ureq::Request {
    agent
        .get(url.as_str())
        .set("User-Agent", concat!("Daozai/", env!("CARGO_PKG_VERSION")))
        .set("Accept", accept)
        .set("Cache-Control", "no-cache")
}

fn gitee_rate_limited(status: u16, body: impl Read) -> bool {
    const MESSAGE: &[u8] = b"Rate Limit Exceeded";
    status == 429
        || (status == 403
            && read_limited(body, 4096)
                .is_ok_and(|body| body.windows(MESSAGE.len()).any(|part| part == MESSAGE)))
}

fn is_https_host(url: &Url, host: &str) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some(host)
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

fn gitee_raw_url(source: &Url) -> Option<Url> {
    if !is_https_host(source, "gitee.com") || source.query() != Some("ref=master") {
        return None;
    }
    let prefix = format!("/api/v5/repos/{GITEE_REPOSITORY}/contents/");
    let (channel, name) = source
        .path()
        .strip_prefix(prefix.as_str())?
        .split_once('/')?;
    let plugin_zip = name
        .strip_prefix("plugin-")
        .and_then(|name| name.strip_suffix(".zip"))
        .and_then(|name| name.rsplit_once('-'))
        .is_some_and(|(id, version_name)| valid_id(id) && version(version_name).is_some());
    if !matches!(channel, "preview" | "stable")
        || !(name == "catalog.json" || valid_avatar_name(name) || plugin_zip)
    {
        return None;
    }
    Url::parse(&format!(
        "https://gitee.com/{GITEE_REPOSITORY}/raw/master/{channel}/{name}"
    ))
    .ok()
}

fn gitee_signed_location(raw: &Url, location: &str) -> Option<Url> {
    let url = Url::parse(location).ok()?;
    (is_https_host(&url, "raw.giteeusercontent.com")
        && url.path() == raw.path()
        && url
            .query_pairs()
            .any(|(key, value)| key == "signature" && !value.is_empty()))
    .then_some(url)
}

fn download_gitee_raw(agent: &ureq::Agent, raw: &Url, limit: u64) -> Result<Vec<u8>> {
    let request = |url: &Url| {
        market_request(agent, url, "*/*")
            .call()
            .map_err(|error| match error {
                ureq::Error::Status(status, _) => fail(&format!("集市镜像返回 HTTP {status}")),
                _ => fail("无法连接集市，请检查网络后重试"),
            })
    };
    let response = request(raw)?;
    let response = if response.status() == 302 {
        let target = response
            .header("Location")
            .and_then(|location| gitee_signed_location(raw, location))
            .ok_or_else(|| fail("集市下载地址发生非预期跳转"))?;
        request(&target)?
    } else {
        response
    };
    if response.status() != 200 {
        return Err(fail("集市下载地址发生非预期跳转"));
    }
    read_limited(response.into_reader(), limit)
}

fn decode_gitee_file(body: &[u8], expected_path: &str, limit: u64) -> Result<Vec<u8>> {
    #[derive(Deserialize)]
    struct Content {
        #[serde(rename = "type")]
        kind: String,
        encoding: String,
        path: String,
        size: u64,
        content: String,
    }
    let file: Content = serde_json::from_slice(body).map_err(|_| fail("集市接口未返回有效文件"))?;
    if file.kind != "file" || file.encoding != "base64" || file.path != expected_path {
        return Err(fail("集市返回的文件信息与请求不一致"));
    }
    if file.size > limit {
        return Err(fail("集市文件超过大小限制"));
    }
    let expected_encoded = (file.size.div_ceil(3) * 4) as usize;
    let encoded: Vec<_> = file
        .content
        .bytes()
        .filter(|byte| !matches!(byte, b'\r' | b'\n'))
        .take(expected_encoded + 1)
        .collect();
    if encoded.len() != expected_encoded {
        return Err(fail("集市返回的文件大小不一致"));
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| fail("集市返回的文件编码无效"))?;
    if bytes.len() as u64 != file.size {
        return Err(fail("集市返回的文件大小不一致"));
    }
    Ok(bytes)
}

fn read_catalog_zip(path: &Path) -> Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.is_file() || path.canonicalize()?.parent() != path.parent() {
        return Err(fail("测试集市插件包必须是同目录的普通文件"));
    }
    read_limited(File::open(path)?, ZIP_LIMIT)
}

fn supported(manifest: &Manifest) -> Result<()> {
    if manifest.runtime != "js" || manifest.entry.contains('/') {
        return Err(fail("集市仅接受根目录含一个 JavaScript 入口的卡片插件"));
    }
    Ok(())
}

fn decode_zip(raw: &[u8]) -> Result<Package> {
    if raw.len() as u64 > ZIP_LIMIT {
        return Err(fail("插件 ZIP 超过 576 KiB"));
    }
    let mut zip = ZipArchive::new(Cursor::new(raw))
        .map_err(|error| fail(&format!("插件 ZIP 格式无效：{error}")))?;
    if zip.len() > 3
        || zip.has_duplicate_names()
        || zip.offset() != 0
        || zip.has_overlapping_entries()?
    {
        return Err(fail("插件 ZIP 只能包含清单、入口和封面三个根目录文件"));
    }
    let manifest = Manifest::decode(&zip_text(&mut zip, "plugin.json", MANIFEST_LIMIT)?)?;
    supported(&manifest)?;
    let source = zip_text(&mut zip, &manifest.entry, SCRIPT_SOURCE_LIMIT)?;
    if source.trim().is_empty() {
        return Err(fail("插件入口文件为空"));
    }
    let cover = CoverPng::decode(zip_bytes(&mut zip, COVER_FILE, COVER_LIMIT)?)?;
    Ok(Package {
        manifest,
        source,
        cover: Some(cover),
    })
}

fn zip_text(zip: &mut ZipArchive<Cursor<&[u8]>>, name: &str, limit: u64) -> Result<String> {
    String::from_utf8(zip_bytes(zip, name, limit)?)
        .map_err(|_| fail(&format!("插件 ZIP 中的 {name} 必须使用 UTF-8")))
}

fn zip_bytes(zip: &mut ZipArchive<Cursor<&[u8]>>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let file = zip
        .by_name(name)
        .map_err(|_| fail(&format!("插件 ZIP 缺少 {name}")))?;
    if file.name_raw() != name.as_bytes()
        || !file.is_file()
        || file
            .unix_mode()
            .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000))
        || file.size() > limit
    {
        return Err(fail(&format!(
            "插件 ZIP 中的 {name} 必须是大小合规的普通文件"
        )));
    }
    read_limited(file, limit)
}

fn smoke(package: &Package) -> Result<()> {
    let values: Values = package
        .manifest
        .settings
        .iter()
        .map(|field| (field.key.clone(), field.default.clone()))
        .collect();
    let today = chrono::Local::now().date_naive().to_string();
    package
        .render(&values, &Value::Null, &today)
        .map_err(|error| {
            fail(&format!(
                "入口 {} 的 refresh 自检失败：{error}",
                package.manifest.entry
            ))
        })?;
    Ok(())
}

fn checked_project(path: &Path) -> Result<Package> {
    if path.file_name().and_then(|name| name.to_str()) != Some("plugin.json") {
        return Err(fail("请指定插件根目录的 plugin.json"));
    }
    let package =
        Package::read(path).map_err(|error| fail(&format!("{}：{error}", path.display())))?;
    supported(&package.manifest)?;
    smoke(&package)?;
    Ok(package)
}

pub(super) fn validate_project(path: &Path) -> Result<String> {
    let package = checked_project(path)?;
    Ok(format!(
        "{} v{} 校验通过",
        package.manifest.id, package.manifest.version
    ))
}

pub(super) fn create_template(
    parent: &Path,
    id: &str,
    name: &str,
    author: &str,
) -> Result<PathBuf> {
    let (id, name, author) = (id.trim(), name.trim(), author.trim());
    if !valid_id(id) {
        return Err(fail("插件 ID 格式应类似 xiaoming.payday，最多 64 个字符"));
    }
    if name.is_empty() || name.chars().count() > 80 {
        return Err(fail("插件名称需要 1 到 80 个字符"));
    }
    if author.is_empty() || author.chars().count() > 80 {
        return Err(fail("作者名称需要 1 到 80 个字符"));
    }

    let mut manifest = Manifest::decode(include_str!("examples/payday/plugin.json"))?;
    manifest.id = id.into();
    manifest.name = name.into();
    manifest.author = author.into();
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    let files: [(&str, &[u8]); 4] = [
        ("plugin.json", &bytes),
        ("main.js", include_bytes!("examples/payday/main.js")),
        (COVER_FILE, include_bytes!("examples/payday/cover.png")),
        ("LICENSE", include_bytes!("examples/payday/LICENSE")),
    ];

    let project = parent.join(id);
    let path = target_path(&project)?;
    if fs::symlink_metadata(&path).is_ok() {
        return Err(fail("同名文件或目录已存在，请更换插件 ID 或保存位置"));
    }
    let tmp = loop {
        let tmp = temporary_sibling(&path);
        match fs::create_dir(&tmp) {
            Ok(()) => break tmp,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };
    // 文件全部写好后再移入目标路径，已有目录始终保留。
    let result = files
        .iter()
        .try_for_each(|(name, contents)| fs::write(tmp.join(name), contents))
        .and_then(|_| move_new(&tmp, &path));
    if let Err(error) = result {
        for (name, _) in files {
            let _ = fs::remove_file(tmp.join(name));
        }
        let _ = fs::remove_dir(&tmp);
        return Err(fail(&format!(
            "无法创建插件目录 {}：{error}",
            project.display()
        )));
    }
    Ok(project.join("plugin.json"))
}

fn target_path(path: &Path) -> io::Result<PathBuf> {
    let filename = path
        .file_name()
        .ok_or_else(|| io::Error::other("缺少目标文件名"))?;
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()?;
    Ok(directory.join(filename))
}

fn temporary_sibling(path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let mut name = path.file_name().unwrap().to_os_string();
    name.push(format!(
        ".{}-{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, AtomicOrdering::Relaxed)
    ));
    path.with_file_name(name)
}

fn move_new(tmp: &Path, path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{HRESULT, PCWSTR};
    use windows::Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS},
        Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH},
    };

    let source: Vec<u16> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| {
        if error.code() == HRESULT::from_win32(ERROR_ALREADY_EXISTS.0)
            || error.code() == HRESULT::from_win32(ERROR_FILE_EXISTS.0)
        {
            io::Error::new(io::ErrorKind::AlreadyExists, error)
        } else {
            io::Error::other(error)
        }
    })
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let path = target_path(path)?;
    let (tmp, mut file) = loop {
        let tmp = temporary_sibling(&path);
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => break (tmp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };

    // 版本文件只在完整写盘后出现，移动时保留已存在的目标。
    let result = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    let result = result.and_then(|_| move_new(&tmp, &path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub(super) fn pack(manifest_path: &Path, output: &Path) -> Result<()> {
    let package = checked_project(manifest_path)?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("plugin.json", options)?;
    zip.write_all(&serde_json::to_vec(&package.manifest)?)?;
    zip.start_file(&package.manifest.entry, options)?;
    zip.write_all(package.source.as_bytes())?;
    zip.start_file(COVER_FILE, options)?;
    let cover = package.cover.as_ref().ok_or_else(|| fail("缺少插件封面"))?;
    zip.write_all(cover.bytes())?;
    let raw = zip.finish()?.into_inner();
    decode_zip(&raw)?;
    write_new(output, &raw)
        .map_err(|error| fail(&format!("无法创建 ZIP {}：{error}", output.display())))?;
    Ok(())
}

pub(super) fn submit(input: &Path, path: &Path) -> Result<()> {
    let raw = read_limited(File::open(input)?, ZIP_LIMIT)?;
    let package = decode_zip(&raw)?;
    smoke(&package)?;
    let manifest = package.manifest;
    let mut entry = Entry {
        id: manifest.id,
        name: manifest.name,
        author: manifest.author,
        version: manifest.version,
        size: raw.len() as u64,
        sha256: sha256(&raw),
        presentation: None,
    };
    let path = target_path(path)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_file_name(".daozai-test-market.lock"))?;
    // 操作系统在作者进程退出时释放锁，目录里的版本更新串行提交。
    lock.lock()?;
    let mut plugins = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => Catalog::open(&path)?.entries,
        Ok(_) => return Err(fail("测试集市目录必须是普通文件")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    let zip_path = entry.path_in(&path);
    if let Some(old) = plugins.iter_mut().find(|old| old.id == entry.id) {
        if compare_versions(&entry.version, &old.version) != Some(Ordering::Greater) {
            return Err(fail("提交版本必须高于测试集市已有版本"));
        }
        entry.presentation = old.presentation.take();
        *old = entry;
    } else {
        plugins.push(entry);
    }
    plugins.sort_by(|left, right| left.id.cmp(&right.id));
    let catalog = serde_json::to_vec_pretty(&CatalogFile {
        format: CATALOG_FORMAT,
        plugins,
    })?;
    if catalog.len() as u64 > CATALOG_LIMIT {
        return Err(fail("测试集市目录超过大小限制"));
    }
    let created = match write_new(&zip_path, &raw) {
        Ok(()) => true,
        Err(error)
            if error.kind() == io::ErrorKind::AlreadyExists
                && read_catalog_zip(&zip_path).is_ok_and(|existing| existing == raw) =>
        {
            // 上次在写入目录前退出，可继续提交字节完全相同的版本包。
            false
        }
        Err(error) => return Err(fail(&format!("无法保存不可覆盖的版本 ZIP：{error}"))),
    };
    if let Err(error) = crate::model::atomic_write(&path, &catalog) {
        if created {
            let _ = fs::remove_file(&zip_path);
        }
        return Err(fail(&format!("测试集市目录提交失败：{error}")));
    }
    Ok(())
}

pub(in crate::sdk_spike) fn author_command(args: &[OsString]) -> Option<ExitCode> {
    let operation = args.get(1)?.to_str()?;
    if !matches!(
        operation,
        "--plugin-test" | "--plugin-pack" | "--plugin-submit"
    ) {
        return None;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    // Windows 图形程序的显式作者命令与父终端共用控制台。
    unsafe { AttachConsole(u32::MAX) };
    let result = match (operation, &args[2..]) {
        ("--plugin-test", [manifest]) => validate_project(Path::new(manifest)),
        ("--plugin-pack", [manifest, output]) => pack(Path::new(manifest), Path::new(output))
            .map(|_| format!("已生成插件包 {}", Path::new(output).display())),
        ("--plugin-submit", [zip, catalog]) => submit(Path::new(zip), Path::new(catalog))
            .map(|_| format!("已提交到测试集市 {}", Path::new(catalog).display())),
        _ => Err(fail(match operation {
            "--plugin-test" => "用法：--plugin-test <plugin.json>",
            "--plugin-pack" => "用法：--plugin-pack <plugin.json> <输出 ZIP>",
            _ => "用法：--plugin-submit <插件 ZIP> <catalog.json>",
        })),
    };
    Some(match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    })
}

#[cfg(test)]
#[path = "market_tests.rs"]
mod tests;
