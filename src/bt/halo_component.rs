//! 无线电量增强的可选安装与版本校验。
use crate::updates::{self, Package};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

const RELEASE_PREFIX: &str = "wireless-battery-v";
const RELEASE_API: &str =
    "https://api.github.com/repos/Renly1994/xiaotang-daozai/releases?per_page=100";
const DOWNLOAD_ROOT: &str = "https://github.com/Renly1994/xiaotang-daozai/releases/download";
const MAX_SIZE: u64 = 96 * 1024 * 1024;
const RECEIPT: &str = "component.json";
const CHOICE: &str = "enabled";
const TEST_FEED: &str = "wireless-battery-test.json";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub(crate) struct Component {
    directory: PathBuf,
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    enabled: bool,
    work: Option<Arc<AtomicBool>>,
    active: Option<PathBuf>,
    error: Option<String>,
}

pub(crate) struct Status {
    pub enabled: bool,
    pub busy: bool,
    pub installed: bool,
    pub error: Option<String>,
}

impl Component {
    pub(crate) fn load(directory: PathBuf) -> Self {
        let directory = std::path::absolute(&directory).unwrap_or(directory);
        let enabled = File::open(directory.join(CHOICE))
            .and_then(|file| {
                let mut choice = Vec::new();
                file.take(2).read_to_end(&mut choice)?;
                Ok(choice == b"1")
            })
            .unwrap_or(false);
        let component = Self {
            directory,
            state: Arc::new(Mutex::new(State {
                enabled,
                ..State::default()
            })),
        };
        if enabled {
            component.begin(false, false, || {});
        }
        component
    }

    pub(crate) fn status(&self) -> Status {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        Status {
            enabled: state.enabled,
            busy: state.work.is_some(),
            installed: state.active.is_some(),
            error: state.error.clone(),
        }
    }

    pub(super) fn executable(&self) -> Option<PathBuf> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.active.clone().filter(|_| state.enabled)
    }

    pub(crate) fn set_enabled(
        &self,
        enabled: bool,
        wake: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if enabled == state.enabled {
                return Ok(());
            }
            fs::create_dir_all(&self.directory)
                .and_then(|_| {
                    crate::model::atomic_write(
                        &self.directory.join(CHOICE),
                        if enabled { b"1" } else { b"0" },
                    )
                })
                .map_err(|_| "无法保存电量增强开关")?;
            state.enabled = enabled;
            state.error = None;
            if !enabled {
                if let Some(work) = state.work.take() {
                    work.store(true, Ordering::Relaxed);
                }
                state.active = None;
            }
        }
        if enabled {
            self.begin(true, false, wake);
        }
        Ok(())
    }

    pub(crate) fn check_update(&self, wake: impl FnOnce() + Send + 'static) {
        self.begin(true, true, wake);
    }

    fn begin(&self, allow_download: bool, refresh: bool, wake: impl FnOnce() + Send + 'static) {
        let work = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if state.work.is_some() || !state.enabled {
                return;
            }
            let work = Arc::new(AtomicBool::new(false));
            state.work = Some(work.clone());
            state.error = None;
            work
        };
        let component = self.clone();
        let result = std::thread::Builder::new()
            .name("daozai-battery-component".into())
            .spawn(move || {
                let prepared = if !refresh {
                    cached(&component.directory, &work).map(|path| Prepared {
                        path,
                        installation: None,
                    })
                } else {
                    Err("需要检查组件更新".into())
                }
                .or_else(|error| {
                    if allow_download && !work.load(Ordering::Relaxed) {
                        download(&component.directory, &work)
                    } else {
                        Err(error)
                    }
                });
                {
                    let mut state = component
                        .state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    if state
                        .work
                        .as_ref()
                        .is_some_and(|active| Arc::ptr_eq(active, &work))
                    {
                        state.work = None;
                        match prepared.and_then(|prepared| prepared.install(&component.directory)) {
                            Ok(path) => state.active = Some(path),
                            Err(error) => state.error = Some(error),
                        }
                    }
                }
                wake();
            });
        if result.is_err() {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.work = None;
            state.error = Some("无法启动组件安装任务".into());
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    size: u64,
    hash: String,
}

impl Receipt {
    fn package(self, url: String) -> Option<Package> {
        (self.size > 0
            && self.size <= MAX_SIZE
            && self.hash.len() == 64
            && self.hash.bytes().all(|ch| ch.is_ascii_hexdigit()))
        .then(|| Package {
            url,
            size: self.size,
            hash: self.hash.to_ascii_lowercase(),
        })
    }
}

fn asset(value: &serde_json::Value) -> Option<([u32; 3], Package)> {
    let tag = value["tag_name"].as_str()?;
    let mut components = tag.strip_prefix(RELEASE_PREFIX)?.split('.');
    let mut version = [0; 3];
    for number in &mut version {
        let part = components.next()?;
        if !part.bytes().all(|digit| digit.is_ascii_digit()) {
            return None;
        }
        *number = part.parse().ok()?;
    }
    if components.next().is_some() || value["draft"].as_bool()? || !value["prerelease"].as_bool()? {
        return None;
    }
    let expected = format!("{DOWNLOAD_ROOT}/{tag}/daozai-halo.exe");
    let entry = value["assets"]
        .as_array()?
        .iter()
        .find(|entry| entry["name"] == "daozai-halo.exe")?;
    if entry["state"].as_str()? != "uploaded" || entry["browser_download_url"].as_str()? != expected
    {
        return None;
    }
    Receipt {
        size: entry["size"].as_u64()?,
        hash: entry["digest"].as_str()?.strip_prefix("sha256:")?.into(),
    }
    .package(expected)
    .map(|package| (version, package))
}

fn binary_path(directory: &Path, package: &Package) -> PathBuf {
    directory.join(format!("daozai-halo-{}.exe", package.hash))
}

fn cached(directory: &Path, cancel: &AtomicBool) -> Result<PathBuf, String> {
    let invalid = || "电量组件需要重新下载".to_owned();
    let receipt = File::open(directory.join(RECEIPT)).map_err(|_| invalid())?;
    let package = serde_json::from_reader::<_, Receipt>(receipt.take(1024))
        .ok()
        .and_then(|receipt| receipt.package(String::new()))
        .ok_or_else(invalid)?;
    let path = binary_path(directory, &package);
    if !fs::symlink_metadata(&path)
        .is_ok_and(|meta| meta.file_type().is_file() && meta.len() == package.size)
    {
        return Err(invalid());
    }
    let mut executable = File::open(&path).map_err(|_| invalid())?;
    updates::transfer(&mut executable, &mut io::sink(), &package, cancel, |_| {})
        .map_err(|_| invalid())?;
    Ok(path)
}

fn download(directory: &Path, cancel: &AtomicBool) -> Result<Prepared, String> {
    if option_env!("DAOZAI_TEST_BUILD").is_some() {
        if let Some((package, file)) = preview_asset()? {
            return prepare(directory, package, cancel, file);
        }
    }
    let agent = updates::http_agent()?;
    let response = agent
        .get(RELEASE_API)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", concat!("Daozai/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(404, _) => "官方电量组件暂未发布",
            _ => "无法获取电量组件，请检查网络后重试",
        })?;
    if response.get_url() != RELEASE_API {
        return Err("电量组件发布地址无效".into());
    }
    let releases: Vec<serde_json::Value> =
        serde_json::from_reader(response.into_reader().take(8 * 1024 * 1024))
            .map_err(|_| "电量组件发布信息无效")?;
    let (_, package) = releases
        .iter()
        .filter_map(asset)
        .max_by_key(|(version, _)| *version)
        .ok_or("官方电量组件暂未发布或缺少校验信息")?;
    if cancel.load(Ordering::Relaxed) {
        return Err("已取消下载".into());
    }
    if let Ok(path) = cached(directory, cancel) {
        if path == binary_path(directory, &package) {
            return Ok(Prepared {
                path,
                installation: None,
            });
        }
    }
    let response = agent
        .get(&package.url)
        .call()
        .map_err(|_| "无法下载电量组件，请重试")?;
    let final_url = url::Url::parse(response.get_url()).map_err(|_| "电量组件下载地址无效")?;
    if final_url.scheme() != "https"
        || !matches!(
            final_url.host_str(),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
    {
        return Err("电量组件下载地址无效".into());
    }
    prepare(directory, package, cancel, response.into_reader())
}

fn preview_asset() -> Result<Option<(Package, File)>, String> {
    // 本地验收源由测试版程序旁的文件指定，与插件导入的目录无关。
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Feed {
        artifact: PathBuf,
        package: Receipt,
    }
    let path = std::env::current_exe()
        .map_err(|_| "无法找到测试版程序")?
        .with_file_name(TEST_FEED);
    let feed = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("无法读取本地验收源".into()),
    };
    let feed: Feed = serde_json::from_reader(feed.take(4096)).map_err(|_| "本地验收源格式无效")?;
    if !feed.artifact.is_absolute()
        || feed.artifact.file_name().and_then(|name| name.to_str()) != Some("daozai-halo.exe")
    {
        return Err("本地验收组件路径无效".into());
    }
    let package = feed
        .package
        .package(String::new())
        .ok_or("本地验收组件校验信息无效")?;
    let file = File::open(feed.artifact).map_err(|_| "无法读取本地验收组件")?;
    Ok(Some((package, file)))
}

struct Temporary(PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

struct Prepared {
    path: PathBuf,
    installation: Option<(Temporary, Receipt)>,
}

impl Prepared {
    fn install(self, directory: &Path) -> Result<PathBuf, String> {
        if let Some((staged, receipt)) = self.installation {
            crate::model::replace_file(&staged.0, &self.path).map_err(|_| "无法安装电量组件")?;
            let metadata = serde_json::to_vec(&receipt).map_err(|_| "无法保存电量组件版本")?;
            crate::model::atomic_write(&directory.join(RECEIPT), &metadata)
                .map_err(|_| "无法保存电量组件版本")?;
        }
        Ok(self.path)
    }
}

fn prepare(
    directory: &Path,
    package: Package,
    cancel: &AtomicBool,
    mut input: impl Read,
) -> Result<Prepared, String> {
    let path = binary_path(directory, &package);
    // 组件可能正在运行。相同版本经过校验后直接复用安全缓存。
    let available = cached(directory, cancel).is_ok_and(|known| known == path);
    if available {
        return Ok(Prepared {
            path,
            installation: None,
        });
    }
    fs::create_dir_all(directory).map_err(|_| "无法创建电量组件目录")?;
    let temporary = Temporary(directory.join(format!(
        ".download-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
    )));
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary.0)
        .map_err(|_| "无法写入电量组件目录")?;
    updates::transfer(&mut input, &mut output, &package, cancel, |_| {})
        .map_err(|_| "电量组件下载不完整或校验失败")?;
    output.sync_all().map_err(|_| "无法保存电量组件")?;
    Ok(Prepared {
        path,
        installation: Some((
            temporary,
            Receipt {
                size: package.size,
                hash: package.hash,
            },
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn optional_battery_accepts_only_the_official_release_asset() {
        let mut value = json!({
            "tag_name": "wireless-battery-v1.18.0", "draft": false, "prerelease": true,
            "assets": [{
                "name": "daozai-halo.exe", "state": "uploaded", "size": 30,
                "digest": format!("sha256:{}", "A".repeat(64)),
                "browser_download_url": format!("{DOWNLOAD_ROOT}/wireless-battery-v1.18.0/daozai-halo.exe")
            }]
        });
        let (version, package) = asset(&value).unwrap();
        assert_eq!(version, [1, 18, 0]);
        assert_eq!(package.hash, "a".repeat(64));
        for (field, invalid) in [
            ("digest", json!("sha256:short")),
            ("size", json!(MAX_SIZE + 1)),
            (
                "browser_download_url",
                json!("https://example.com/daozai-halo.exe"),
            ),
        ] {
            let mut changed = value.clone();
            changed["assets"][0][field] = invalid;
            assert!(asset(&changed).is_none());
        }
        value["draft"] = json!(true);
        assert!(asset(&value).is_none());
    }

    #[test]
    fn optional_battery_default_creates_nothing_and_rejects_tampered_component() {
        let directory = std::env::temp_dir().join(format!(
            "daozai-optional-battery-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
        ));
        let component = Component::load(directory.clone());
        assert!(!component.status().enabled);
        assert!(component.executable().is_none());
        assert!(!directory.exists());
        let bytes = b"component fixture";
        let package = || Package {
            url: String::new(),
            size: bytes.len() as u64,
            hash: ring::digest::digest(&ring::digest::SHA256, bytes)
                .as_ref()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        };
        let cancel = AtomicBool::new(false);
        assert!(prepare(&directory, package(), &cancel, &b"corrupted fixture"[..]).is_err());
        assert!(!directory.join(RECEIPT).exists());
        let installed = prepare(&directory, package(), &cancel, &bytes[..])
            .unwrap()
            .install(&directory)
            .unwrap();
        assert_eq!(cached(&directory, &cancel).unwrap(), installed);
        fs::write(&installed, b"corrupted fixture").unwrap();
        assert!(cached(&directory, &cancel).is_err());
        assert!(component.executable().is_none());
        fs::remove_file(installed).unwrap();
        fs::remove_file(directory.join(RECEIPT)).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    #[ignore = "需要在测试二进制旁配置本地验收组件"]
    fn optional_battery_preview_installs_and_runs_only_after_opt_in() {
        use super::super::halo_worker::{Receiver, Worker};
        use std::{
            sync::mpsc,
            time::{Duration, Instant},
        };

        assert!(option_env!("DAOZAI_TEST_BUILD").is_some());
        assert!(preview_asset().unwrap().is_some());
        let directory = std::env::temp_dir().join(format!(
            "daozai-preview-battery-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
        ));
        let component = Component::load(directory.clone());
        assert!(!directory.exists());
        let (finished, ready) = mpsc::channel();
        component
            .set_enabled(true, move || {
                let _ = finished.send(());
            })
            .unwrap();
        ready.recv_timeout(Duration::from_secs(25)).unwrap();
        let path = component
            .executable()
            .unwrap_or_else(|| panic!("{:?}", component.status().error));
        let worker = Worker::spawn(path.clone()).unwrap();
        worker.set_targets(
            1,
            vec![Receiver {
                vid: 1,
                pid: 2,
                usb_instance: r"USB\VID_0001&PID_0002\DAOZAI-COMPONENT-PREVIEW".into(),
            }],
        );
        let deadline = Instant::now() + Duration::from_secs(25);
        let snapshot = loop {
            if let Some(snapshot) = worker.snapshot(Instant::now()) {
                break snapshot;
            }
            assert!(Instant::now() < deadline, "组件没有返回验收快照");
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(snapshot.generation, 1);
        assert!(snapshot.devices.is_empty());
        component.set_enabled(false, || {}).unwrap();
        drop(worker);
        assert!(component.executable().is_none());
        assert!(!Component::load(directory.clone()).status().enabled);
        // 测试退出后 Windows 任务对象负责释放子进程的文件句柄。
        for _ in 0..20 {
            if fs::remove_file(&path).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!path.exists());
        fs::remove_file(directory.join(CHOICE)).unwrap();
        fs::remove_file(directory.join(RECEIPT)).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
