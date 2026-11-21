//! 本地数据型兼容包。安装成功后才替换运行中的接收器识别表。
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const PACKAGE_ID: &str = "daozai.wireless-receivers";
const API_VERSION: u32 = 1;
const FILE_NAME: &str = "wireless-receivers.json";
const BUILTIN: &[u8] = include_bytes!("../../assets/wireless-receivers.json");
const MAX_BYTES: usize = 256 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String,
    api_version: u32,
    version: String,
    receivers: Vec<Receiver>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receiver {
    vid: String,
    pid: String,
    name: String,
}

pub(super) struct Package {
    version: String,
    receivers: BTreeMap<(u16, u16), String>,
}

impl Package {
    fn parse(raw: &[u8]) -> Result<Self, &'static str> {
        let manifest: Manifest =
            serde_json::from_slice(raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(raw))
                .map_err(|_| "兼容包格式无效")?;
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
        for receiver in manifest.receivers {
            let vid = usb_id(&receiver.vid)
                .filter(|id| *id != 0)
                .ok_or("接收器 VID 无效")?;
            let pid = usb_id(&receiver.pid).ok_or("接收器 PID 无效")?;
            if !valid_text(&receiver.name, 64) {
                return Err("接收器名称无效");
            }
            if receivers.insert((vid, pid), receiver.name).is_some() {
                return Err("兼容包有重复的接收器 VID/PID");
            }
        }
        Ok(Self {
            version: manifest.version,
            receivers,
        })
    }

    pub(super) fn receiver_name(&self, hardware: &str) -> Option<&str> {
        let (vid, pid) = hardware.strip_prefix("VID_")?.split_once("&PID_")?;
        self.receivers
            .get(&(usb_id(vid)?, usb_id(pid)?))
            .map(String::as_str)
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

fn read_pack(path: &Path) -> Result<(Vec<u8>, Package), &'static str> {
    let metadata = fs::metadata(path).map_err(|_| "无法读取兼容包文件")?;
    if !metadata.is_file() {
        return Err("请选择兼容包文件");
    }
    if metadata.len() > MAX_BYTES as u64 {
        return Err("兼容包文件过大");
    }
    let file = fs::File::open(path).map_err(|_| "无法读取兼容包文件")?;
    let mut raw = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "无法读取兼容包文件")?;
    if raw.len() > MAX_BYTES {
        return Err("兼容包文件过大");
    }
    let package = Package::parse(&raw)?;
    Ok((raw, package))
}

struct Inner {
    path: PathBuf,
    package: Mutex<Arc<Package>>,
}

#[derive(Clone)]
pub struct Store(Arc<Inner>);

impl Store {
    pub fn load() -> Self {
        Self::load_from(crate::model::config_dir().join(FILE_NAME))
    }

    fn load_from(path: PathBuf) -> Self {
        let package = read_pack(&path)
            .map(|(_, package)| package)
            .unwrap_or_else(|_| Package::parse(BUILTIN).expect("内置无线设备兼容包应有效"));
        Self(Arc::new(Inner {
            path,
            package: Mutex::new(Arc::new(package)),
        }))
    }

    pub fn version(&self) -> String {
        self.package().version.clone()
    }

    pub fn install(&self, source: &Path) -> Result<(), String> {
        let (raw, package) = read_pack(source)?;
        let mut active = self.0.package.lock().unwrap_or_else(|e| e.into_inner());
        let parent = self.0.path.parent().ok_or("无法定位兼容包保存目录")?;
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|_| "无法创建兼容包保存目录")?;
        }
        crate::model::atomic_write(&self.0.path, &raw).map_err(|_| "无法保存兼容包")?;
        *active = Arc::new(package);
        Ok(())
    }

    pub(super) fn package(&self) -> Arc<Package> {
        self.0
            .package
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
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
    fn builtin_pack_covers_existing_receivers_and_recovers_from_a_bad_local_file() {
        let files = Files::new();
        fs::write(files.dir.join(FILE_NAME), b"invalid JSON").unwrap();
        let store = files.store();
        assert_eq!(store.version(), "1.0.0");
        assert_eq!(store.package().receivers.len(), 23);
        assert_eq!(
            store.package().receiver_name("VID_2717&PID_5055"),
            Some("小米无线键鼠套装 2")
        );
    }

    #[test]
    fn invalid_pack_keeps_installed_version_in_memory_and_on_disk() {
        let files = Files::new();
        let store = files.store();
        let mut valid = Files::builtin();
        valid["version"] = json!("1.1.0");
        store.install(&files.write(&valid)).unwrap();
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
            assert_eq!(store.version(), "1.1.0");
            assert_eq!(fs::read(files.dir.join(FILE_NAME)).unwrap(), expected);
        }
        assert_eq!(files.store().version(), "1.1.0");
    }

    #[test]
    fn storage_failure_does_not_activate_pack() {
        let files = Files::new();
        let store = files.store();
        fs::create_dir(files.dir.join(FILE_NAME)).unwrap();
        let mut data = Files::builtin();
        data["version"] = json!("1.1.0");
        assert!(store.install(&files.write(&data)).is_err());
        assert_eq!(store.version(), "1.0.0");
    }
}
