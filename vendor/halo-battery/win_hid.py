"""仅为已核实的 2.4 GHz 接收器列出 Windows HID 集合。"""

from __future__ import annotations

import ctypes
import re
from ctypes import wintypes

from halo.hid_transport import MetadataApi, _HIDP_CAPS
from halo.native_io import WindowsApi


USB_INSTANCE = re.compile(r"USB\\VID_([0-9A-F]{4})&PID_([0-9A-F]{4})\\[^\\\x00]+", re.IGNORECASE)
HID_PATH = re.compile(r"\\\\\?\\(hid)\#([^#\\]+)\#([^#\\]+)\#\{[0-9a-f-]{36}\}", re.IGNORECASE)
INTERFACE = re.compile(r"&mi_([0-9a-f]{2})(?:&|#)", re.IGNORECASE)


class _Attributes(ctypes.Structure):
    _fields_ = [
        ("size", wintypes.ULONG),
        ("vendor", wintypes.USHORT),
        ("product", wintypes.USHORT),
        ("release", wintypes.USHORT),
    ]


class WindowsHid:
    def __init__(self, list_paths):
        self.list_paths = list_paths
        self._file = WindowsApi()
        self._metadata = MetadataApi()
        attributes = self._metadata.dll.HidD_GetAttributes
        attributes.argtypes = [wintypes.HANDLE, ctypes.POINTER(_Attributes)]
        attributes.restype = ctypes.c_ubyte
        self._cfg = ctypes.WinDLL("cfgmgr32")
        methods = {
            "CM_Locate_DevNodeW": [ctypes.POINTER(wintypes.ULONG), wintypes.LPCWSTR, wintypes.ULONG],
            "CM_Get_Parent": [ctypes.POINTER(wintypes.ULONG), wintypes.ULONG, wintypes.ULONG],
            "CM_Get_Device_IDW": [wintypes.ULONG, wintypes.LPWSTR, wintypes.ULONG, wintypes.ULONG],
        }
        for name, args in methods.items():
            method = getattr(self._cfg, name)
            method.argtypes, method.restype = args, wintypes.ULONG

    def parent_for(self, path):
        # Windows HID 的符号链接包含对应设备实例。再由 PnP 验证祖先，避免路径猜测。
        match = HID_PATH.fullmatch(path)
        if match is None:
            return None
        node = wintypes.ULONG()
        if self._cfg.CM_Locate_DevNodeW(ctypes.byref(node), "\\".join(match.groups()), 0) != 0:
            return None
        parent = wintypes.ULONG()
        name = ctypes.create_unicode_buffer(512)
        for _ in range(10):
            if self._cfg.CM_Get_Parent(ctypes.byref(parent), node.value, 0) != 0:
                return None
            if self._cfg.CM_Get_Device_IDW(parent.value, name, len(name), 0) != 0:
                return None
            if USB_INSTANCE.fullmatch(name.value):
                return name.value.casefold()
            node.value = parent.value
        return None

    def paths_by_receiver(self, receivers):
        by_instance = {root.identity: [] for root in receivers}
        if by_instance:
            for path in self.list_paths() or ():
                root = self.parent_for(path)
                if root in by_instance:
                    by_instance[root].append(path)
        return {root: tuple(sorted(paths)) for root, paths in by_instance.items() if paths}

    def metadata(self, path, receiver):
        if self.parent_for(path) != receiver.identity:
            return None
        try:
            # 零访问权限，仅查询 HID 描述符与标识符；报告由上游传输层收发。
            handle = self._file.open_path(path, access=0)
        except OSError:
            return None
        dll = self._metadata.dll
        try:
            attributes = _Attributes(ctypes.sizeof(_Attributes))
            if (not dll.HidD_GetAttributes(handle.handle, ctypes.byref(attributes))
                    or (attributes.vendor, attributes.product) != (receiver.vid, receiver.pid)):
                return None
            preparsed = ctypes.c_void_p()
            if not dll.HidD_GetPreparsedData(handle.handle, ctypes.byref(preparsed)):
                return None
            try:
                caps = _HIDP_CAPS()
                if dll.HidP_GetCaps(preparsed, ctypes.byref(caps)) != 0x00110000:
                    return None
            finally:
                dll.HidD_FreePreparsedData(preparsed)
            strings = {}
            for key, field in (
                ("serial_number", "SerialNumber"),
                ("manufacturer_string", "Manufacturer"),
                ("product_string", "Product"),
            ):
                try:
                    strings[key] = self._metadata.string(handle, field)
                except OSError:
                    strings[key] = None
            interface = INTERFACE.search(path)
            return {
                "path": path.encode("utf-8"),
                "vendor_id": receiver.vid,
                "product_id": receiver.pid,
                "release_number": attributes.release,
                "interface_number": int(interface[1], 16) if interface else -1,
                "usage_page": caps.UsagePage,
                "usage": caps.Usage,
                "bus_type": 1,
                **strings,
            }
        finally:
            handle.close()
