use super::Reading;
use ::windows::core::{w, PCWSTR};
use ::windows::Wdk::Graphics::Direct3D::*;
use ::windows::Win32::{
    Foundation::{FILETIME, LUID},
    Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE},
    NetworkManagement::{
        IpHelper::{FreeMibTable, GetIfTable2},
        Ndis::IfOperStatusUp,
    },
    System::{
        Performance::*,
        SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
        Threading::GetSystemTimes,
    },
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

type Interfaces = HashMap<u64, [u64; 2]>;

#[derive(Default)]
pub(super) struct Sampler {
    cpu: Option<[u64; 2]>,
    net: Option<(Instant, Interfaces)>,
    disk: Option<Disk>,
    gpu: Option<Gpu>,
    gpu_retry: Option<Instant>,
}

impl Sampler {
    pub fn read(&mut self, now: Instant) -> Reading {
        let mut r = Reading::default();
        let times = cpu_times();
        r.cpu = self.cpu.zip(times).and_then(|(prev, cur)| cpu_load(prev, cur));
        self.cpu = times;
        let mut mem = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if unsafe { GlobalMemoryStatusEx(&mut mem) }.is_ok() {
            r.memory = Some((mem.ullTotalPhys.saturating_sub(mem.ullAvailPhys), mem.ullTotalPhys));
        }
        if self.gpu.is_none() && self.gpu_retry.is_none_or(|t| now >= t) {
            self.gpu = Gpu::open();
            self.gpu_retry = Some(now + Duration::from_secs(30));
        }
        if let Some(gpu) = &mut self.gpu {
            if !gpu.read(now, &mut r) {
                self.gpu = None;
            }
        }
        let next = interfaces();
        if let (Some((prev_at, prev)), Some(cur)) = (&self.net, &next) {
            r.network = net_rates(prev, cur, now.duration_since(*prev_at).as_secs_f64());
        }
        self.net = next.map(|v| (now, v));
        if self.disk.is_none() {
            self.disk = Disk::open();
        }
        if let Some(disk) = &self.disk {
            r.disk = disk.read();
        }
        r
    }
}

fn cpu_times() -> Option<[u64; 2]> {
    let (mut idle, mut kernel, mut user) = Default::default();
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    let ticks = |v: FILETIME| u64::from(v.dwHighDateTime) << 32 | u64::from(v.dwLowDateTime);
    Some([ticks(idle), ticks(kernel) + ticks(user)])
}
fn cpu_load(prev: [u64; 2], cur: [u64; 2]) -> Option<f32> {
    let total = cur[1].checked_sub(prev[1])?;
    let busy = total.checked_sub(cur[0].checked_sub(prev[0])?)?;
    (total > 0).then(|| 100. * busy as f32 / total as f32)
}

fn interfaces() -> Option<Interfaces> {
    let mut table = std::ptr::null_mut();
    unsafe { GetIfTable2(&mut table) }.ok().ok()?;
    // 行数由同一次系统分配给出，只在释放表前借用行。
    let rows = unsafe { std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize) };
    let mut counters = HashMap::new();
    for row in rows {
        if row.OperStatus == IfOperStatusUp && row.InterfaceAndOperStatusFlags._bitfield & 3 == 1 && row.Type != 24 {
            counters.insert(unsafe { row.InterfaceLuid.Value }, [row.InOctets, row.OutOctets]);
        }
    }
    unsafe { FreeMibTable(table.cast()) };
    Some(counters)
}

fn net_rates(prev: &Interfaces, cur: &Interfaces, secs: f64) -> [Option<f32>; 2] {
    if secs <= 0. || secs > 5. {
        return [None; 2];
    }
    std::array::from_fn(|i| {
        let mut changes = cur
            .iter()
            .filter_map(|(key, bytes)| bytes[i].checked_sub(prev.get(key)?[i]))
            .peekable();
        if changes.peek().is_none() {
            return None;
        }
        Some((changes.map(|n| n as f64).sum::<f64>() / secs) as f32)
    })
}

struct Disk {
    query: isize,
    counters: [isize; 2],
}
impl Disk {
    fn open() -> Option<Self> {
        let mut disk = Self {
            query: 0,
            counters: [0; 2],
        };
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut disk.query) } != 0 {
            return None;
        }
        let names = [
            w!("\\PhysicalDisk(_Total)\\Disk Read Bytes/sec"),
            w!("\\PhysicalDisk(_Total)\\Disk Write Bytes/sec"),
        ];
        for (slot, name) in disk.counters.iter_mut().zip(names) {
            unsafe { PdhAddEnglishCounterW(disk.query, name, 0, slot) };
        }
        Some(disk)
    }
    fn read(&self) -> [Option<f32>; 2] {
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return [None; 2];
        }
        self.counters.map(|c| {
            let mut v = PDH_FMT_COUNTERVALUE::default();
            if c == 0 || unsafe { PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE, None, &mut v) } != 0 || v.CStatus > 1 {
                return None;
            }
            let rate = unsafe { v.Anonymous.doubleValue } as f32;
            (rate.is_finite() && rate >= 0.).then_some(rate)
        })
    }
}
impl Drop for Disk {
    fn drop(&mut self) {
        if self.query != 0 {
            unsafe { PdhCloseQuery(self.query) };
        }
    }
}

struct Gpu {
    luid: LUID,
    handle: Option<u32>,
    nodes: u32,
    segments: u32,
    capacity: u64,
    name: String,
    previous: Option<(Instant, Vec<i64>)>,
}

impl Gpu {
    fn open() -> Option<Self> {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
        let desc = (0..)
            .map_while(|i| unsafe { factory.EnumAdapters1(i) }.ok())
            .filter_map(|a| unsafe { a.GetDesc1() }.ok())
            .filter(|d| d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0)
            .max_by_key(|d| d.DedicatedVideoMemory)?;
        let name = String::from_utf16_lossy(
            &desc.Description[..desc
                .Description
                .iter()
                .position(|&n| n == 0)
                .unwrap_or(desc.Description.len())],
        );
        let mut gpu = Self {
            luid: desc.AdapterLuid,
            handle: None,
            nodes: 0,
            segments: 0,
            capacity: desc.DedicatedVideoMemory as u64,
            name,
            previous: None,
        };
        let info = unsafe { gpu.stat(D3DKMT_QUERYSTATISTICS_ADAPTER, 0)?.AdapterInformation };
        gpu.nodes = info.NodeCount;
        gpu.segments = info.NbSegments;
        let mut open = D3DKMT_OPENADAPTERFROMLUID {
            AdapterLuid: gpu.luid,
            hAdapter: 0,
        };
        if unsafe { D3DKMTOpenAdapterFromLuid(&mut open) }.is_ok() {
            gpu.handle = Some(open.hAdapter);
        }
        Some(gpu)
    }
    fn stat(&self, kind: D3DKMT_QUERYSTATISTICS_TYPE, index: u32) -> Option<D3DKMT_QUERYSTATISTICS_RESULT> {
        let mut q = D3DKMT_QUERYSTATISTICS {
            Type: kind,
            AdapterLuid: self.luid,
            ..Default::default()
        };
        if kind == D3DKMT_QUERYSTATISTICS_NODE {
            q.Anonymous.QueryNode.NodeId = index;
        }
        if kind == D3DKMT_QUERYSTATISTICS_SEGMENT {
            q.Anonymous.QuerySegment.SegmentId = index;
        }
        // 内核根据 Type 写入对应的联合体成员。
        unsafe { D3DKMTQueryStatistics(&raw mut q) }.is_ok().then_some(q.QueryResult)
    }
    fn read(&mut self, now: Instant, r: &mut Reading) -> bool {
        let times: Option<Vec<_>> = (0..self.nodes)
            .map(|i| {
                Some(unsafe {
                    self.stat(D3DKMT_QUERYSTATISTICS_NODE, i)?
                        .NodeInformation
                        .GlobalInformation
                        .RunningTime
                })
            })
            .collect();
        let Some(times) = times else {
            return false;
        };
        if let Some((at, prev)) = &self.previous {
            let interval = now.duration_since(*at).as_secs_f64();
            if interval > 0. && interval <= 5. {
                r.gpu = times
                    .iter()
                    .zip(prev)
                    .filter_map(|(a, b)| a.checked_sub(*b).filter(|n| *n >= 0))
                    .max()
                    .map(|ticks| (ticks as f64 / (1e5 * interval)).clamp(0., 100.) as f32);
            }
        }
        self.previous = Some((now, times));
        r.gpu_name = self.name.clone();
        if self.capacity > 0 {
            let used = (0..self.segments)
                .map(|i| Some(unsafe { self.stat(D3DKMT_QUERYSTATISTICS_SEGMENT, i)?.SegmentInformation }))
                .try_fold(0u64, |total, seg| {
                    let s = seg?;
                    Some(total + if s.Aperture == 0 { s.BytesResident } else { 0 })
                });
            r.video_memory = used.map(|v| (v, self.capacity));
        }
        if let Some(handle) = self.handle {
            let mut perf = D3DKMT_ADAPTER_PERFDATA::default();
            let mut q = D3DKMT_QUERYADAPTERINFO {
                hAdapter: handle,
                Type: KMTQAITYPE_ADAPTERPERFDATA,
                pPrivateDriverData: (&raw mut perf).cast(),
                PrivateDriverDataSize: size_of_val(&perf) as u32,
            };
            if unsafe { D3DKMTQueryAdapterInfo(&mut q) }.is_ok() && (1..1300).contains(&perf.Temperature) {
                r.gpu_temp = Some(perf.Temperature as f32 / 10.);
            }
        }
        true
    }
}
impl Drop for Gpu {
    fn drop(&mut self) {
        if let Some(h) = self.handle {
            let _ = unsafe { D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: h }) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counter_resets_and_adapter_changes_do_not_spike() {
        assert_eq!(cpu_load([20, 100], [40, 180]), Some(75.));
        assert_eq!(cpu_load([20, 100], [1, 10]), None);
        let prev = HashMap::from([(1, [100, 90]), (2, [800, 900])]);
        let cur = HashMap::from([(1, [300, 40]), (3, [5000, 9000])]);
        assert_eq!(net_rates(&prev, &cur, 2.), [Some(100.), None]);
    }
    #[test]
    fn windows_samples_live_cpu_and_memory() {
        let mut sampler = Sampler::default();
        let first = sampler.read(Instant::now());
        assert!(first.cpu.is_none());
        std::thread::sleep(Duration::from_millis(1050));
        let next = sampler.read(Instant::now());
        assert!(next.cpu.is_some_and(|v| (0.0..=100.0).contains(&v)));
        assert!(next.memory.is_some_and(|(used, total)| used <= total && total > 0));
        println!("{next:?}");
    }
}
