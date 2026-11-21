//! 本机测试集市的作者工具和包校验。

use super::{bounded, fail, valid_id, Manifest, Package, Result, Values, SCRIPT_SOURCE_LIMIT};
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
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MANIFEST_LIMIT: u64 = 32 * 1024;
const ZIP_LIMIT: u64 = 512 * 1024;
const CATALOG_LIMIT: u64 = 64 * 1024;
const CATALOG_FORMAT: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) author: String,
    pub(super) version: String,
    pub(super) size: u64,
    pub(super) sha256: String,
}

impl Entry {
    fn matches(&self, manifest: &Manifest) -> bool {
        self.id == manifest.id
            && self.version == manifest.version
            && self.name == manifest.name
            && self.author == manifest.author
    }

    fn path_in(&self, catalog: &Path) -> PathBuf {
        catalog.with_file_name(format!("plugin-{}-{}.zip", self.id, self.version))
    }
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
}

impl Catalog {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let path = path.canonicalize()?;
        let file: CatalogFile = serde_json::from_str(&bounded(&path, CATALOG_LIMIT)?)
            .map_err(|error| fail(&format!("测试集市目录格式无效：{error}")))?;
        if file.format != CATALOG_FORMAT {
            return Err(fail("当前岛仔不支持此测试集市目录版本"));
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
                || !seen.insert(&entry.id)
            {
                return Err(fail("测试集市目录包含无效或重复的插件条目"));
            }
        }
        Ok(Self {
            path,
            entries: file.plugins,
        })
    }

    pub(super) fn load(&self, entry: &Entry) -> Result<Package> {
        let path = entry.path_in(&self.path);
        let raw = read_catalog_zip(&path)?;
        if raw.len() as u64 != entry.size || sha256(&raw) != entry.sha256 {
            return Err(fail("插件包大小或 SHA-256 与测试集市目录不一致"));
        }
        let package = decode_zip(&raw)?;
        if !entry.matches(&package.manifest) {
            return Err(fail("插件清单与测试集市目录不一致"));
        }
        Ok(package)
    }
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
        return Err(fail("插件包文件超过大小限制"));
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
        return Err(fail("测试集市仅接受根目录含一个 JavaScript 入口的待办插件"));
    }
    Ok(())
}

fn decode_zip(raw: &[u8]) -> Result<Package> {
    let mut zip = ZipArchive::new(Cursor::new(raw))
        .map_err(|error| fail(&format!("插件 ZIP 格式无效：{error}")))?;
    if zip.len() != 2
        || zip.has_duplicate_names()
        || zip.offset() != 0
        || zip.has_overlapping_entries()?
    {
        return Err(fail("插件 ZIP 只能包含两个不重复的根目录文件"));
    }
    let manifest = Manifest::decode(&zip_text(&mut zip, "plugin.json", MANIFEST_LIMIT)?)?;
    supported(&manifest)?;
    let source = zip_text(&mut zip, &manifest.entry, SCRIPT_SOURCE_LIMIT)?;
    if source.trim().is_empty() {
        return Err(fail("插件入口文件为空"));
    }
    Ok(Package { manifest, source })
}

fn zip_text(zip: &mut ZipArchive<Cursor<&[u8]>>, name: &str, limit: u64) -> Result<String> {
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
    String::from_utf8(read_limited(file, limit)?)
        .map_err(|_| fail(&format!("插件 ZIP 中的 {name} 必须使用 UTF-8")))
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

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::{
        os::windows::ffi::OsStrExt,
        sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
    };
    use windows::core::{HRESULT, PCWSTR};
    use windows::Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS},
        Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH},
    };

    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let path = target_path(path)?;
    let (tmp, mut file) = loop {
        let mut name = path.file_name().unwrap().to_os_string();
        name.push(format!(
            ".{}-{}.tmp",
            std::process::id(),
            SERIAL.fetch_add(1, AtomicOrdering::Relaxed)
        ));
        let tmp = path.with_file_name(name);
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => break (tmp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };

    // 版本文件只在完整写盘后出现，移动时保留已存在的目标。
    let result = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    let result = result.and_then(|_| {
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
    });
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
    let entry = Entry {
        id: manifest.id,
        name: manifest.name,
        author: manifest.author,
        version: manifest.version,
        size: raw.len() as u64,
        sha256: sha256(&raw),
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
        ("--plugin-test", [manifest]) => checked_project(Path::new(manifest)).map(|package| {
            format!(
                "{} v{} 校验通过",
                package.manifest.id, package.manifest.version
            )
        }),
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
