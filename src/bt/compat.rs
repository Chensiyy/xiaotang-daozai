//! 本地数据型兼容包。只有已启用的包参与接收器识别。
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

const PACKAGE_ID: &str = "daozai.wireless-receivers";
const API_VERSION: u32 = 1;
const FILE_NAME: &str = "wireless-receivers.json";
#[cfg(test)]
const BUILTIN: &[u8] = include_bytes!("../../assets/wireless-receivers.json");
const MAX_BYTES: usize = 256 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String,
    api_version: u32,
    version: String,
    receivers: Vec<Receiver>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receiver {
    vid: String,
    pid: String,
    name: String,
}

pub(super) struct Package {
    manifest: Manifest,
    receivers: BTreeMap<(u16, u16), usize>,
}

impl Package {
    fn parse(raw: &[u8]) -> Result<Self, &'static str> {
        let manifest = serde_json::from_slice(raw).map_err(|_| "兼容包格式无效")?;
        Self::from_manifest(manifest)
    }

    fn from_manifest(manifest: Manifest) -> Result<Self, &'static str> {
        if manifest.id != PACKAGE_ID {
            return Err("此文件不是岛仔无线设备兼容包");
        }
        if manifest.api_version != API_VERSION {
            return Err("当前岛仔版本不支持此兼容包接口");
        }
        if !valid_text(&manifest.version, 32) {
            return Err("兼容包版本无效");
        }
        let mut receivers = BTreeMap::new();
        for (index, receiver) in manifest.receivers.iter().enumerate() {
            let vid = usb_id(&receiver.vid)
                .filter(|id| *id != 0)
                .ok_or("接收器 VID 无效")?;
            let pid = usb_id(&receiver.pid).ok_or("接收器 PID 无效")?;
            if !valid_text(&receiver.name, 64) {
                return Err("接收器名称无效");
            }
            if receivers.insert((vid, pid), index).is_some() {
                return Err("兼容包有重复的接收器 VID/PID");
            }
        }
        Ok(Self {
            manifest,
            receivers,
        })
    }

    pub(super) fn receiver_name(&self, hardware: &str) -> Option<&str> {
        let (vid, pid) = hardware.strip_prefix("VID_")?.split_once("&PID_")?;
        self.receivers
            .get(&(usb_id(vid)?, usb_id(pid)?))
            .map(|&index| self.manifest.receivers[index].name.as_str())
    }
}

fn usb_id(hex: &str) -> Option<u16> {
    if hex.len() != 4 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(hex, 16).ok()
}

fn valid_text(text: &str, max: usize) -> bool {
    !text.is_empty()
        && text.trim() == text
        && text.chars().count() <= max
        && !text.chars().any(char::is_control)
}

fn read_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, &'static str> {
    let metadata = fs::metadata(path).map_err(|_| "无法读取兼容包文件")?;
    if !metadata.is_file() {
        return Err("请选择兼容包文件");
    }
    if metadata.len() > max_bytes as u64 {
        return Err("兼容包文件过大");
    }
    let file = fs::File::open(path).map_err(|_| "无法读取兼容包文件")?;
    let mut raw = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "无法读取兼容包文件")?;
    if raw.len() > max_bytes {
        return Err("兼容包文件过大");
    }
    if raw.starts_with(b"\xef\xbb\xbf") {
        raw.drain(..3);
    }
    Ok(raw)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record<T> {
    enabled: bool,
    package: T,
}

type Installed = Record<Arc<Package>>;

struct Inner {
    path: PathBuf,
    installed: Mutex<Option<Installed>>,
}

#[derive(Clone)]
pub struct Store(Arc<Inner>);

impl Store {
    pub fn load() -> Self {
        Self::load_from(crate::model::config_dir().join(FILE_NAME))
    }

    fn load_from(path: PathBuf) -> Self {
        // 保存记录比原始包多一层启停信息；旧版裸包仍可作为停用的安装记录读取。
        let installed = read_file(&path, MAX_BYTES + 128).ok().and_then(|raw| {
            let record = serde_json::from_slice::<Record<Manifest>>(&raw)
                .or_else(|_| {
                    serde_json::from_slice(&raw).map(|package| Record {
                        enabled: false,
                        package,
                    })
                })
                .ok()?;
            Some(Record {
                enabled: record.enabled,
                package: Arc::new(Package::from_manifest(record.package).ok()?),
            })
        });
        Self(Arc::new(Inner {
            path,
            installed: Mutex::new(installed),
        }))
    }

    pub fn version(&self) -> Option<String> {
        self.installed()
            .as_ref()
            .map(|record| record.package.manifest.version.clone())
    }

    pub fn enabled(&self) -> bool {
        self.installed()
            .as_ref()
            .is_some_and(|record| record.enabled)
    }

    pub fn install(&self, source: &Path) -> Result<(), String> {
        self.install_package(Package::parse(&read_file(source, MAX_BYTES)?)?)
    }

    fn install_package(&self, package: Package) -> Result<(), String> {
        let mut installed = self.installed();
        let enabled = installed.as_ref().is_some_and(|record| record.enabled);
        self.persist(&package, enabled)?;
        *installed = Some(Record {
            enabled,
            package: Arc::new(package),
        });
        Ok(())
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        let mut installed = self.installed();
        let record = installed.as_mut().ok_or("请先安装无线设备兼容包")?;
        if enabled != record.enabled {
            self.persist(&record.package, enabled)?;
            record.enabled = enabled;
        }
        Ok(())
    }

    pub fn uninstall(&self) -> Result<(), String> {
        let mut installed = self.installed();
        if let Err(error) = fs::remove_file(&self.0.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err("无法卸载无线设备兼容包".into());
            }
        }
        *installed = None;
        Ok(())
    }

    fn persist(&self, package: &Package, enabled: bool) -> Result<(), String> {
        let parent = self.0.path.parent().ok_or("无法定位兼容包保存目录")?;
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|_| "无法创建兼容包保存目录")?;
        }
        let raw = serde_json::to_vec(&Record {
            enabled,
            package: &package.manifest,
        })
        .map_err(|_| "无法保存兼容包")?;
        crate::model::atomic_write(&self.0.path, &raw).map_err(|_| "无法保存兼容包".into())
    }

    fn installed(&self) -> MutexGuard<'_, Option<Installed>> {
        self.0.installed.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(super) fn package(&self) -> Option<Arc<Package>> {
        self.installed()
            .as_ref()
            .filter(|record| record.enabled)
            .map(|record| record.package.clone())
    }

    #[cfg(test)]
    pub(crate) fn test_install_builtin(&self) {
        self.install_package(Package::parse(BUILTIN).unwrap())
            .unwrap();
    }
}

#[cfg(test)]
pub(super) mod fixtures {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    pub(in crate::bt) struct Files {
        pub dir: PathBuf,
    }

    impl Files {
        pub(in crate::bt) fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "daozai-receiver-pack-{}-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&dir).unwrap();
            Self { dir }
        }

        pub(in crate::bt) fn store(&self) -> Store {
            Store::load_from(self.dir.join(FILE_NAME))
        }

        #[cfg(windows)]
        pub(in crate::bt) fn enabled_store(&self) -> Store {
            let store = self.store();
            store.test_install_builtin();
            store.set_enabled(true).unwrap();
            store
        }

        pub(in crate::bt) fn builtin() -> serde_json::Value {
            serde_json::from_slice(BUILTIN).unwrap()
        }

        pub(in crate::bt) fn write(&self, data: &serde_json::Value) -> PathBuf {
            let path = self.dir.join("install.json");
            fs::write(&path, serde_json::to_vec(data).unwrap()).unwrap();
            path
        }
    }

    impl Drop for Files {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fixtures::Files, *};
    use serde_json::json;

    #[test]
    fn clean_and_corrupt_config_have_no_installed_pack() {
        let files = Files::new();
        let empty = files.store();
        assert!(empty.version().is_none());
        assert!(!empty.enabled());
        assert!(empty.package().is_none());
        fs::write(files.dir.join(FILE_NAME), b"invalid JSON").unwrap();
        let store = files.store();
        assert!(store.version().is_none());
        assert!(!store.enabled());
        assert!(store.package().is_none());
        assert!(store.set_enabled(true).is_err());
    }

    #[test]
    fn installs_disabled_and_restores_changes_to_enabled_state() {
        let files = Files::new();
        let store = files.store();
        store.test_install_builtin();
        assert_eq!(store.version().as_deref(), Some("1.0.0"));
        assert!(!files.store().enabled());
        assert!(store.package().is_none());

        store.set_enabled(true).unwrap();
        let loaded = files.store();
        assert!(loaded.enabled());
        let package = loaded.package().unwrap();
        assert_eq!(package.receivers.len(), 23);
        assert_eq!(
            package.receiver_name("VID_2717&PID_5055"),
            Some("小米无线键鼠套装 2")
        );

        store.set_enabled(false).unwrap();
        assert_eq!(files.store().version().as_deref(), Some("1.0.0"));
        assert!(!files.store().enabled());
        assert!(store.package().is_none());
    }

    #[test]
    fn updates_keep_enablement_and_reinstall_after_uninstall_starts_disabled() {
        let files = Files::new();
        let store = files.store();
        store.test_install_builtin();
        store.set_enabled(true).unwrap();
        let mut update = Files::builtin();
        update["version"] = json!("1.1.0");
        store.install(&files.write(&update)).unwrap();
        assert!(files.store().enabled());
        assert_eq!(files.store().version().as_deref(), Some("1.1.0"));

        store.set_enabled(false).unwrap();
        update["version"] = json!("1.2.0");
        store.install(&files.write(&update)).unwrap();
        assert!(!files.store().enabled());
        assert_eq!(files.store().version().as_deref(), Some("1.2.0"));

        store.set_enabled(true).unwrap();
        store.uninstall().unwrap();
        assert!(store.version().is_none() && store.package().is_none());
        assert!(!files.dir.join(FILE_NAME).exists());
        assert!(files.store().version().is_none());
        store.test_install_builtin();
        assert_eq!(store.version().as_deref(), Some("1.0.0"));
        assert!(!store.enabled());
        assert!(files.store().package().is_none());
    }

    #[test]
    fn legacy_manifest_loads_as_installed_but_disabled() {
        let files = Files::new();
        let path = files.dir.join(FILE_NAME);
        fs::write(&path, BUILTIN).unwrap();
        let store = files.store();
        assert_eq!(store.version().as_deref(), Some("1.0.0"));
        assert!(!store.enabled());
        assert!(store.package().is_none());
        store.set_enabled(true).unwrap();
        assert!(files.store().enabled());
        assert!(files.store().package().is_some());
    }

    #[test]
    fn invalid_pack_keeps_installed_version_in_memory_and_on_disk() {
        let files = Files::new();
        let store = files.store();
        let mut valid = Files::builtin();
        valid["version"] = json!("1.1.0");
        store.install(&files.write(&valid)).unwrap();
        store.set_enabled(true).unwrap();
        let expected = fs::read(files.dir.join(FILE_NAME)).unwrap();

        let mut wrong_id = valid.clone();
        wrong_id["id"] = json!("other.package");
        let mut wrong_interface = valid.clone();
        wrong_interface["api_version"] = json!(2);
        let mut duplicate = valid.clone();
        duplicate["receivers"]
            .as_array_mut()
            .unwrap()
            .push(json!({"vid":"046d","pid":"c548","name":"重复接收器"}));
        let mut wildcard = valid.clone();
        wildcard["receivers"][0]["pid"] = json!("50*");
        let mut extra_field = valid.clone();
        extra_field["receivers"][0]["command"] = json!("noop");
        for data in [wrong_id, wrong_interface, duplicate, wildcard, extra_field] {
            assert!(store.install(&files.write(&data)).is_err());
            assert_eq!(store.version().as_deref(), Some("1.1.0"));
            assert!(store.enabled());
            assert_eq!(fs::read(files.dir.join(FILE_NAME)).unwrap(), expected);
        }
        assert_eq!(files.store().version().as_deref(), Some("1.1.0"));
        assert!(files.store().enabled());
    }

    #[test]
    fn storage_failure_does_not_activate_pack() {
        let files = Files::new();
        let store = files.store();
        fs::create_dir(files.dir.join(FILE_NAME)).unwrap();
        let mut data = Files::builtin();
        data["version"] = json!("1.1.0");
        assert!(store.install(&files.write(&data)).is_err());
        assert!(store.version().is_none());
    }

    #[test]
    fn enablement_and_uninstall_failures_keep_current_state() {
        let files = Files::new();
        let path = files.dir.join(FILE_NAME);
        let store = files.store();
        store.test_install_builtin();
        store.set_enabled(true).unwrap();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(store.set_enabled(false).is_err());
        assert!(store.uninstall().is_err());
        assert_eq!(store.version().as_deref(), Some("1.0.0"));
        assert!(store.enabled());
        assert!(store.package().is_some());
    }
}
