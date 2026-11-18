//! 蓝牙设备连接与多来源电量监控。
mod state;
pub use state::{Device,DeviceKind,BatterySource,BatteryReading,FRESH_FOR};
#[cfg(windows)]
mod windows;

fn device_kind(name: &str, classic: bool, class: Option<u32>) -> DeviceKind {
    use DeviceKind::*;
    let name = name.to_lowercase();
    if ["shokz", "aftershokz", "openmove", "openrun", "骨传导"].iter().any(|s| name.contains(s)) { return Bone; }
    if ["earbuds", "airpods", "buds", "earphones", "入耳"].iter().any(|s| name.contains(s)) { return Earbuds; }
    if ["keyboard", "键盘"].iter().any(|s| name.contains(s)) { return Keyboard; }
    if ["mouse", "鼠标"].iter().any(|s| name.contains(s)) { return Mouse; }
    if ["gamepad", "controller", "手柄"].iter().any(|s| name.contains(s)) { return Gamepad; }
    if ["speaker", "音箱", "音响"].iter().any(|s| name.contains(s)) { return Speaker; }
    if let Some(class) = class {
        if classic {
            match (class >> 8) & 0x1f {
                2 => return Phone,
                4 => return if matches!((class >> 2) & 0x3f, 5 | 7 | 10) { Speaker } else { Headphones },
                5 => return match (class >> 6) & 3 { 1 => Keyboard, 2 => Mouse, 3 => Keyboard, _ => Other },
                _ => {}
            }
        } else {
            match (class >> 6, class & 0x3f) {
                (15, 1) => return Keyboard,
                (15, 2) => return Mouse,
                (15, 3 | 4) => return Gamepad,
                (1, _) => return Phone,
                _ => {}
            }
        }
    }
    if ["headphone", "headset", "耳机"].iter().any(|s| name.contains(s)) { Headphones } else { Other }
}


/// 释放服务会唤醒后台线程并取消未完成的蓝牙读取。
pub struct Service {
    pub rx: std::sync::mpsc::Receiver<Vec<Device>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Drop for Service {
    fn drop(&mut self) {self.stop.store(true,std::sync::atomic::Ordering::Relaxed);}
}
pub fn spawn()->Service {
    let (tx,rx)=std::sync::mpsc::sync_channel(1);
    let stop=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(windows)] {
        let stopped=stop.clone();
        let _=std::thread::Builder::new().name("daozai-bluetooth".into()).spawn(move||windows::run(tx,stopped));
    }
    #[cfg(not(windows))] let _=tx;
    Service{rx,stop}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifies_connected_audio_and_input_devices() {
        assert_eq!(device_kind("OpenMove by Shokz", true, Some(0x240418)), DeviceKind::Bone);
        assert_eq!(device_kind("QingLong 87 Ultra 8K", false, Some(961)), DeviceKind::Keyboard);
        assert_eq!(device_kind("MX Anywhere", false, Some(962)), DeviceKind::Mouse);
        assert_eq!(device_kind("耳机", true, Some(0x0404)), DeviceKind::Headphones);
        assert_eq!(device_kind("AirPods", true, None), DeviceKind::Earbuds);
        assert_eq!(device_kind("未知设备", false, None), DeviceKind::Other);
    }

    #[test]
    fn summary_omits_missing_battery() {
        let d = Device { id: "1".into(), name: "Headset".into(), battery: None, kind: DeviceKind::Headphones, ..Default::default() };
        assert_eq!(d.summary(), "Headset");
        let d = Device { battery: Some(80), ..d };
        assert_eq!(d.summary(), "Headset · 电量 80%");
    }
}
