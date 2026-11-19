//! 已识别的 2.4G USB 接收器。拔插查询独立于蓝牙枚举。
use super::{Device, DeviceKind};
use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender, TrySendError},
        Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::{w, GUID},
    Win32::Devices::DeviceAndDriverInstallation::*,
};

const GUID_DEVINTERFACE_USB_DEVICE: GUID = GUID::from_u128(0xa5dcbf10_6530_11d2_901f_00c04fb951ed);

fn receiver_snapshot(instances: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<Device> {
    let mut devices: Vec<_> = instances
        .into_iter()
        .filter_map(|instance| {
            let instance = instance.as_ref().to_ascii_uppercase();
            let (hardware, serial_or_path) = instance.strip_prefix("USB\\")?.split_once('\\')?;
            if serial_or_path.is_empty() || serial_or_path.contains('\\') {
                return None;
            }
            // 只匹配完整 USB 设备的硬件 ID，复合设备的 MI_* 子接口不会单独出现。
            let name = match hardware {
                "VID_2717&PID_5055" => "小米无线键鼠套装 2",
                "VID_045E&PID_07A5" | "VID_045E&PID_07B2" => "微软无线接收器",
                // 罗技系列的 VID/PID 见 Solaar 的 lib/logitech_receiver/base_usb.py。
                "VID_046D&PID_C548" => "罗技 Bolt 接收器",
                "VID_046D&PID_C52B" | "VID_046D&PID_C532" => "罗技 Unifying 接收器",
                "VID_046D&PID_C539" | "VID_046D&PID_C53D" | "VID_046D&PID_C53F"
                | "VID_046D&PID_C541" | "VID_046D&PID_C543" | "VID_046D&PID_C545"
                | "VID_046D&PID_C547" | "VID_046D&PID_C54D" => "罗技 LIGHTSPEED 接收器",
                "VID_046D&PID_C53A" => "罗技 POWERPLAY 接收器",
                "VID_046D&PID_C52F" | "VID_046D&PID_C521" | "VID_046D&PID_C525"
                | "VID_046D&PID_C526" | "VID_046D&PID_C52E" | "VID_046D&PID_C531"
                | "VID_046D&PID_C534" | "VID_046D&PID_C537" => "罗技 Nano 接收器",
                _ => return None,
            };
            Some(Device {
                id: format!("usb:{instance}"),
                name: name.into(),
                kind: DeviceKind::Receiver24G,
                ..Default::default()
            })
        })
        .collect();
    devices.sort_unstable_by(|a, b| a.id.cmp(&b.id));
    devices.dedup_by(|a, b| a.id == b.id);
    devices
}

fn present_receivers() -> Option<Vec<Device>> {
    const FLAGS: u32 = CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT;
    // 拔插发生在查询长度和读取之间时，PnP 可能要求扩大缓冲区。
    for _ in 0..3 {
        let mut size = 0;
        if unsafe { CM_Get_Device_ID_List_SizeW(&mut size, w!("USB"), FLAGS) } != CR_SUCCESS {
            return None;
        }
        let mut buffer = vec![0; size as usize];
        match unsafe { CM_Get_Device_ID_ListW(w!("USB"), &mut buffer, FLAGS) } {
            CR_SUCCESS => {
                let ids = buffer
                    .split(|&ch| ch == 0)
                    .take_while(|id| !id.is_empty())
                    .map(String::from_utf16_lossy);
                return Some(receiver_snapshot(ids));
            }
            CR_BUFFER_SMALL => continue,
            _ => return None,
        }
    }
    None
}

struct Notification {
    handle: HCMNOTIFICATION,
    sender: *mut SyncSender<()>,
}

impl Notification {
    fn register(sender: SyncSender<()>) -> Option<Self> {
        let mut filter = CM_NOTIFY_FILTER {
            cbSize: std::mem::size_of::<CM_NOTIFY_FILTER>() as u32,
            FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
            ..Default::default()
        };
        filter.u.DeviceInterface = CM_NOTIFY_FILTER_0_2 {
            ClassGuid: GUID_DEVINTERFACE_USB_DEVICE,
        };
        let mut handle = HCMNOTIFICATION::default();
        let sender = Box::into_raw(Box::new(sender));
        let result = unsafe {
            CM_Register_Notification(
                &filter,
                Some(sender.cast()),
                Some(on_usb_change),
                &mut handle,
            )
        };
        if result == CR_SUCCESS {
            return Some(Self { handle, sender });
        }
        unsafe {
            drop(Box::from_raw(sender));
        }
        None
    }
}

impl Drop for Notification {
    fn drop(&mut self) {
        // 注销成功会等回调结束。失败时保留回调上下文，避免系统仍访问已释放的发送端。
        unsafe {
            if CM_Unregister_Notification(self.handle) == CR_SUCCESS {
                drop(Box::from_raw(self.sender));
            }
        }
    }
}

unsafe extern "system" fn on_usb_change(
    _: HCMNOTIFICATION,
    context: *const c_void,
    action: CM_NOTIFY_ACTION,
    _: *const CM_NOTIFY_EVENT_DATA,
    _: u32,
) -> u32 {
    if matches!(
        action,
        CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL | CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL
    ) {
        // 发送端在注销成功后释放。回调只唤醒工作线程，把 PnP 查询留给工作线程。
        let sender = unsafe { &*context.cast::<SyncSender<()>>() };
        let _ = sender.try_send(());
    }
    0
}

pub(super) fn run(tx: SyncSender<Vec<Device>>, stop: Arc<AtomicBool>, wake: impl Fn()) {
    let (events, changes) = mpsc::sync_channel(1);
    let notification = Notification::register(events.clone());
    // 接口事件可能先于设备树更新。轻量的 USB 枚举为先到的通知和注册失败兜底。
    let scan_interval = Duration::from_secs(if notification.is_some() { 2 } else { 1 });
    let mut next_scan = Instant::now();
    let mut latest = None;
    let mut published = None;
    while !stop.load(Ordering::Relaxed) {
        if Instant::now() >= next_scan {
            if let Some(devices) = present_receivers() {
                latest = Some(devices);
            }
            next_scan = Instant::now() + scan_interval;
        }
        if latest != published {
            if let Some(devices) = &latest {
                match tx.try_send(devices.clone()) {
                    Ok(()) => {
                        published.clone_from(&latest);
                        wake();
                    }
                    Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Disconnected(_)) => break,
                }
            }
        }
        if changes.recv_timeout(Duration::from_millis(200)).is_ok() {
            next_scan = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_snapshot_uses_physical_usb_instance() {
        let devices = receiver_snapshot([
            r"USB\VID_2717&PID_5055\5&AAA&0&1",
            r"usb\vid_2717&pid_5055\5&aaa&0&1",
            r"USB\VID_2717&PID_5055&MI_00\6&BBB&0&0000",
            r"USB\VID_2717&PID_5055&MI_01\6&BBB&0&0001",
            r"HID\VID_2717&PID_5055&MI_01\7&CCC&0&0000",
            r"USB\VID_2717&PID_5055\5&AAA&0&2",
        ]);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, r"usb:USB\VID_2717&PID_5055\5&AAA&0&1");
        assert_eq!(devices[1].id, r"usb:USB\VID_2717&PID_5055\5&AAA&0&2");
        assert!(devices
            .iter()
            .all(|device| device.kind == DeviceKind::Receiver24G
                && device.connected
                && device.battery.is_none()
                && device.reading.is_none()));
    }

    #[test]
    fn receiver_snapshot_excludes_unverified_vid_pid_and_non_usb_nodes() {
        assert!(receiver_snapshot([
            r"USB\VID_8087&PID_0026\5&AAA&0&1",
            r"USB\VID_046D&PID_C08B\5&AAA&0&2",
            r"USB\VID_046D&PID_C51B\5&AAA&0&3",
            r"USB\VID_046D&PID_C548\",
            r"USB\VID_046D&PID_C548\A\B",
            r"HID\VID_046D&PID_C548\A",
        ])
        .is_empty());
        assert_eq!(
            receiver_snapshot([r"USB\VID_046D&PID_C548\A"])[0].name,
            "罗技 Bolt 接收器"
        );
    }

    #[test]
    fn receiver_usb_change_notifications_wake_the_scanner() {
        let (sender, changes) = mpsc::sync_channel(1);
        let context = std::ptr::from_ref(&sender).cast();
        for action in [
            CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL,
            CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL,
        ] {
            for _ in 0..2 {
                assert_eq!(
                    unsafe {
                        on_usb_change(
                            HCMNOTIFICATION::default(),
                            context,
                            action,
                            std::ptr::null(),
                            0,
                        )
                    },
                    0
                );
            }
            assert_eq!(changes.try_recv(), Ok(()));
            assert_eq!(changes.try_recv(), Err(mpsc::TryRecvError::Empty));
        }
    }

    #[test]
    #[ignore = "按需验证本机 USB 通知及接收器枚举"]
    fn receiver_live_usb_sources() {
        let (sender, _changes) = mpsc::sync_channel(1);
        let _notification = Notification::register(sender).expect("Windows USB 拔插通知注册应成功");
        let devices = present_receivers().expect("Windows 已插入 USB 设备枚举应成功");
        for device in devices {
            println!(
                "{} | id={} | 电量={:?}",
                device.name, device.id, device.battery
            );
        }
    }
}
