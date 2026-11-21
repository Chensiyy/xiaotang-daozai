//! 已识别的 2.4G USB 接收器。拔插查询独立于蓝牙枚举。
use super::compat::{Package, Store};
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

fn receiver_snapshot(
    pack: &Package,
    instances: impl IntoIterator<Item = impl AsRef<str>>,
) -> Vec<Device> {
    let mut devices: Vec<_> = instances
        .into_iter()
        .filter_map(|instance| {
            let instance = instance.as_ref().to_ascii_uppercase();
            let (hardware, serial_or_path) = instance.strip_prefix("USB\\")?.split_once('\\')?;
            if serial_or_path.is_empty() || serial_or_path.contains('\\') {
                return None;
            }
            // 只匹配完整 USB 设备的硬件 ID，复合设备的 MI_* 子接口不会单独出现。
            let name = pack.receiver_name(hardware)?;
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

fn present_receivers(pack: &Package) -> Option<Vec<Device>> {
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
                return Some(receiver_snapshot(pack, ids));
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

pub(super) fn run(
    tx: SyncSender<Vec<Device>>,
    stop: Arc<AtomicBool>,
    store: Store,
    wake: impl Fn(),
) {
    let (events, changes) = mpsc::sync_channel(1);
    let notification = Notification::register(events.clone());
    // 接口事件可能先于设备树更新。轻量的 USB 枚举为先到的通知和注册失败兜底。
    let scan_interval = Duration::from_secs(if notification.is_some() { 2 } else { 1 });
    run_scans(
        tx,
        stop,
        store,
        wake,
        changes,
        scan_interval,
        present_receivers,
    );
}

fn run_scans(
    tx: SyncSender<Vec<Device>>,
    stop: Arc<AtomicBool>,
    store: Store,
    wake: impl Fn(),
    changes: mpsc::Receiver<()>,
    scan_interval: Duration,
    scan: impl Fn(&Package) -> Option<Vec<Device>>,
) {
    let mut active = store.package();
    let mut next_scan = Instant::now();
    let mut latest = None;
    let mut published = None;
    while !stop.load(Ordering::Relaxed) {
        let selected = store.package();
        if Instant::now() >= next_scan || !Arc::ptr_eq(&selected, &active) {
            active = selected;
            if let Some(devices) = scan(&active) {
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
    use crate::bt::compat::fixtures::Files;
    use serde_json::json;

    #[test]
    fn receiver_worker_hot_reloads_compat_pack() {
        let files = Files::new();
        let store = files.store();
        std::thread::scope(|scope| {
            let (tx, rx) = mpsc::sync_channel(1);
            let (_events, changes) = mpsc::sync_channel(1);
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = stop.clone();
            let service = crate::bt::Service { rx, stop };
            let worker_store = store.clone();
            scope.spawn(move || {
                run_scans(
                    tx,
                    worker_stop,
                    worker_store,
                    || {},
                    changes,
                    Duration::from_secs(60),
                    |pack| {
                        Some(receiver_snapshot(
                            pack,
                            [
                                r"USB\VID_046D&PID_C542\DEVICE_1",
                                r"usb\vid_046d&pid_c542\device_1",
                                r"USB\VID_046D&PID_C542&MI_00\INTERFACE_1",
                            ],
                        ))
                    },
                );
            });

            let initial = service.rx.recv_timeout(Duration::from_secs(3)).unwrap();
            assert!(initial.is_empty());
            let mut data = Files::builtin();
            data["version"] = json!("1.1.0");
            data["receivers"]
                .as_array_mut()
                .unwrap()
                .push(json!({"vid":"046D","pid":"C542","name":"罗技 Nano 接收器"}));
            store.install(&files.write(&data)).unwrap();

            let updated = service.rx.recv_timeout(Duration::from_secs(3)).unwrap();
            assert_eq!(updated.len(), 1);
            assert_eq!(updated[0].name, "罗技 Nano 接收器");
            assert_eq!(updated[0].id, r"usb:USB\VID_046D&PID_C542\DEVICE_1");
            assert_eq!(files.store().version(), "1.1.0");
        });
    }

    #[test]
    #[ignore = "需本机插有小米无线键鼠套装 2 的接收器"]
    fn receiver_live_usb_hot_reloads_compat_pack() {
        let files = Files::new();
        let store = files.store();
        let is_xiaomi = |device: &Device| device.id.starts_with(r"usb:USB\VID_2717&PID_5055\");
        assert!(
            present_receivers(&store.package())
                .unwrap()
                .iter()
                .any(is_xiaomi),
            "请插入小米无线键鼠套装 2 的接收器"
        );

        let mut data = Files::builtin();
        data["version"] = json!("1.0.1");
        data["receivers"]
            .as_array_mut()
            .unwrap()
            .retain(|row| row["vid"] != "2717" || row["pid"] != "5055");
        store.install(&files.write(&data)).unwrap();
        std::thread::scope(|scope| {
            let (tx, rx) = mpsc::sync_channel(1);
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = stop.clone();
            let service = crate::bt::Service { rx, stop };
            let worker_store = store.clone();
            scope.spawn(move || run(tx, worker_stop, worker_store, || {}));

            let initial = service.rx.recv_timeout(Duration::from_secs(3)).unwrap();
            assert!(!initial.iter().any(is_xiaomi));
            let mut data = Files::builtin();
            data["version"] = json!("1.1.0");
            store.install(&files.write(&data)).unwrap();

            let updated = service.rx.recv_timeout(Duration::from_secs(3)).unwrap();
            let xiaomi: Vec<_> = updated.iter().filter(|device| is_xiaomi(device)).collect();
            assert_eq!(xiaomi.len(), 1);
            assert_eq!(xiaomi[0].name, "小米无线键鼠套装 2");
            assert_eq!(xiaomi[0].battery, None);
        });
    }

    #[test]
    fn receiver_snapshot_uses_physical_usb_instance() {
        let files = Files::new();
        let devices = receiver_snapshot(
            &files.store().package(),
            [
                r"USB\VID_2717&PID_5055\5&AAA&0&1",
                r"usb\vid_2717&pid_5055\5&aaa&0&1",
                r"USB\VID_2717&PID_5055&MI_00\6&BBB&0&0000",
                r"USB\VID_2717&PID_5055&MI_01\6&BBB&0&0001",
                r"HID\VID_2717&PID_5055&MI_01\7&CCC&0&0000",
                r"USB\VID_2717&PID_5055\5&AAA&0&2",
            ],
        );
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
        let files = Files::new();
        let pack = files.store().package();
        assert!(receiver_snapshot(
            &pack,
            [
                r"USB\VID_8087&PID_0026\5&AAA&0&1",
                r"USB\VID_046D&PID_C08B\5&AAA&0&2",
                r"USB\VID_046D&PID_C51B\5&AAA&0&3",
                r"USB\VID_046D&PID_C548\",
                r"USB\VID_046D&PID_C548\A\B",
                r"HID\VID_046D&PID_C548\A",
            ]
        )
        .is_empty());
        assert_eq!(
            receiver_snapshot(&pack, [r"USB\VID_046D&PID_C548\A"])[0].name,
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
        let files = Files::new();
        let (sender, _changes) = mpsc::sync_channel(1);
        let _notification = Notification::register(sender).expect("Windows USB 拔插通知注册应成功");
        let devices =
            present_receivers(&files.store().package()).expect("Windows 已插入 USB 设备枚举应成功");
        for device in devices {
            println!(
                "{} | id={} | 电量={:?}",
                device.name, device.id, device.battery
            );
        }
    }
}
