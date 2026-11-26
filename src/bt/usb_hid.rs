//! Windows USB 接收器的 HID 报告传输。
use std::time::Instant;
use windows::{
    core::{Owned, HRESULT, PCWSTR},
    Win32::{
        Devices::{DeviceAndDriverInstallation::*, HumanInterfaceDevice::*, Properties::*},
        Foundation::{ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE},
        Storage::FileSystem::{
            CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        },
        System::{
            Threading::{CreateEventW, ResetEvent},
            IO::{CancelIoEx, GetOverlappedResult, GetOverlappedResultEx, OVERLAPPED},
        },
    },
};

pub(super) struct Collection {
    pub(super) usage_page: u16,
    pub(super) usage: u16,
    pub(super) input_len: usize,
    pub(super) output_len: usize,
    path: Vec<u16>,
}

pub(super) fn collections(instance: &str) -> Vec<Collection> {
    let instance = instance.strip_prefix("usb:").unwrap_or(instance);
    let instance = instance.to_ascii_uppercase();
    if !instance.starts_with("USB\\VID_") {
        return Vec::new();
    }
    let instance: Vec<_> = instance.encode_utf16().chain([0]).collect();
    let mut root = 0;
    if unsafe {
        CM_Locate_DevNodeW(
            &mut root,
            PCWSTR(instance.as_ptr()),
            CM_LOCATE_DEVNODE_NORMAL,
        )
    } != CR_SUCCESS
    {
        return Vec::new();
    }
    let mut collections = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        let mut id = [0u16; MAX_DEVICE_ID_LEN as usize];
        if unsafe { CM_Get_Device_IDW(node, &mut id, 0) } != CR_SUCCESS {
            continue;
        }
        let Some(end) = id.iter().position(|&unit| unit == 0) else {
            continue;
        };
        let name = String::from_utf16_lossy(&id[..end]).to_ascii_uppercase();
        let mut identity = name.split('\\');
        let (Some(bus), Some(hardware)) = (identity.next(), identity.next()) else {
            continue;
        };
        // 设备树中的另一枚完整 USB 设备属于另一个物理实例。
        if node != root && bus == "USB" && !hardware.split('&').any(|part| part.starts_with("MI_"))
        {
            continue;
        }
        if bus == "HID" && vendor_collection(node) {
            if let Some(paths) = present_paths(PCWSTR(id.as_ptr())) {
                collections.extend(
                    paths
                        .split_inclusive(|&unit| unit == 0)
                        .filter(|path| path.len() > 1 && path.last() == Some(&0))
                        .filter_map(collection_for),
                );
            }
        }
        let mut child = 0;
        if unsafe { CM_Get_Child(&mut child, node, 0) } == CR_SUCCESS {
            loop {
                pending.push(child);
                let mut sibling = 0;
                if unsafe { CM_Get_Sibling(&mut sibling, child, 0) } != CR_SUCCESS {
                    break;
                }
                child = sibling;
            }
        }
    }
    collections
}

fn present_paths(instance: PCWSTR) -> Option<Vec<u16>> {
    for _ in 0..3 {
        let mut size = 0;
        if unsafe {
            CM_Get_Device_Interface_List_SizeW(
                &mut size,
                &GUID_DEVINTERFACE_HID,
                instance,
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        } != CR_SUCCESS
        {
            return None;
        }
        let mut paths = vec![0; size as usize];
        match unsafe {
            CM_Get_Device_Interface_ListW(
                &GUID_DEVINTERFACE_HID,
                instance,
                &mut paths,
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        } {
            CR_SUCCESS => return Some(paths),
            CR_BUFFER_SMALL => continue,
            _ => return None,
        }
    }
    None
}

fn collection_for(path: &[u16]) -> Option<Collection> {
    // 零访问权限仅取报告描述长度。发送请求前由适配器选定集合并打开读写句柄。
    let metadata = open_path(path, false)?;
    let mut prepared = PHIDP_PREPARSED_DATA::default();
    if !unsafe { HidD_GetPreparsedData(*metadata, &mut prepared) }.as_bool() {
        return None;
    }
    let mut caps = HIDP_CAPS::default();
    let status = unsafe { HidP_GetCaps(prepared, &mut caps) };
    unsafe {
        let _ = HidD_FreePreparsedData(prepared);
    }
    if status != HIDP_STATUS_SUCCESS || caps.UsagePage < 0xFF00 {
        return None;
    }
    Some(Collection {
        usage_page: caps.UsagePage,
        usage: caps.Usage,
        input_len: usize::from(caps.InputReportByteLength),
        output_len: usize::from(caps.OutputReportByteLength),
        path: path.to_vec(),
    })
}

fn vendor_collection(node: u32) -> bool {
    let mut kind = DEVPROPTYPE::default();
    let mut ids = [0u16; 512];
    let mut size = std::mem::size_of_val(&ids) as u32;
    if unsafe {
        CM_Get_DevNode_PropertyW(
            node,
            &DEVPKEY_Device_HardwareIds,
            &mut kind,
            Some(ids.as_mut_ptr().cast()),
            &mut size,
            0,
        )
    } != CR_SUCCESS
        || kind != DEVPROP_TYPE_STRING_LIST
    {
        return false;
    }
    String::from_utf16_lossy(&ids[..size as usize / 2])
        .to_ascii_uppercase()
        .split('\0')
        .any(|id| {
            id.strip_prefix("HID_DEVICE_UP:")
                .and_then(|usage| usage.split_once("_U:"))
                .and_then(|(page, _)| u16::from_str_radix(page, 16).ok())
                .is_some_and(|page| page >= 0xFF00)
        })
}

fn open_path(path: &[u16], read_write: bool) -> Option<Owned<HANDLE>> {
    let access = if read_write {
        (GENERIC_READ | GENERIC_WRITE).0
    } else {
        0
    };
    let flags = if read_write {
        FILE_FLAG_OVERLAPPED
    } else {
        Default::default()
    };
    unsafe {
        let file = CreateFileW(
            PCWSTR(path.as_ptr()),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            flags,
            HANDLE::default(),
        )
        .ok()?;
        Some(Owned::new(file))
    }
}

pub(super) struct Handle {
    file: Owned<HANDLE>,
    event: Owned<HANDLE>,
    input_len: usize,
    output_len: usize,
}

impl Collection {
    pub(super) fn open(&self) -> Option<Handle> {
        let file = open_path(&self.path, true)?;
        let event = unsafe { Owned::new(CreateEventW(None, true, false, PCWSTR::null()).ok()?) };
        Some(Handle {
            file,
            event,
            input_len: self.input_len,
            output_len: self.output_len,
        })
    }
}

enum Direction {
    Read,
    Write,
}

impl Handle {
    pub(super) fn write(&mut self, bytes: &[u8], deadline: Instant) -> bool {
        if bytes.is_empty() || bytes.len() > self.output_len {
            return false;
        }
        let mut report = vec![0; self.output_len];
        report[..bytes.len()].copy_from_slice(bytes);
        self.transfer(&mut report, deadline, Direction::Write) == Some(self.output_len)
    }

    pub(super) fn read(&mut self, deadline: Instant) -> Option<Vec<u8>> {
        if self.input_len == 0 {
            return None;
        }
        let mut report = vec![0; self.input_len];
        let received = self.transfer(&mut report, deadline, Direction::Read)?;
        if received == 0 || received > report.len() {
            return None;
        }
        report.truncate(received);
        Some(report)
    }

    fn transfer(
        &mut self,
        report: &mut [u8],
        deadline: Instant,
        direction: Direction,
    ) -> Option<usize> {
        if Instant::now() >= deadline {
            return None;
        }
        unsafe { ResetEvent(*self.event) }.ok()?;
        let mut operation = OVERLAPPED {
            hEvent: *self.event,
            ..Default::default()
        };
        let file = *self.file;
        let submitted = unsafe {
            match direction {
                Direction::Read => ReadFile(file, Some(report), None, Some(&mut operation)),
                Direction::Write => WriteFile(file, Some(report), None, Some(&mut operation)),
            }
        };
        if submitted.is_err_and(|err| err.code() != HRESULT::from_win32(ERROR_IO_PENDING.0)) {
            return None;
        }
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u128::from(u32::MAX - 1)) as u32;
        let mut bytes = 0;
        if unsafe { GetOverlappedResultEx(file, &operation, &mut bytes, timeout, false) }.is_ok() {
            return (Instant::now() <= deadline).then_some(bytes as usize);
        }
        // 取消只提交请求。确认 I/O 已退出后，才能释放栈上的操作结构和报告缓冲区。
        unsafe {
            let _ = CancelIoEx(file, Some(&operation));
            let _ = GetOverlappedResult(file, &operation, &mut bytes, true);
        }
        None
    }
}
