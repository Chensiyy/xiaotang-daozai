//! 模块开启时独立保留最近一分钟的采样，界面可见时才请求重绘。
use eframe::egui;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[cfg(windows)]
mod windows;

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarStyle {
    #[default]
    Gauges,
    Pulse,
    Network,
    Dashboard,
}
impl BarStyle {
    pub(crate) const ALL: [Self; 4] = [Self::Gauges, Self::Pulse, Self::Network, Self::Dashboard];
    pub(crate) fn width(self) -> f32 {
        match self {
            Self::Gauges => 146.,
            Self::Pulse => 78.,
            Self::Network => 88.,
            Self::Dashboard => 308.,
        }
    }
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Gauges => "gauges",
            Self::Pulse => "pulse",
            Self::Network => "network",
            Self::Dashboard => "dashboard",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Gauges => "三联环",
            Self::Pulse => "处理器脉冲",
            Self::Network => "上下行",
            Self::Dashboard => "综合监测",
        }
    }
    pub(crate) fn parse(key: &str) -> Self {
        Self::ALL.into_iter().find(|v| v.key() == key).unwrap_or_default()
    }
}
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    #[default]
    Overview,
    History,
    Details,
}
impl View {
    pub(crate) const ALL: [Self; 3] = [Self::Overview, Self::History, Self::Details];
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::History => "history",
            Self::Details => "details",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Overview => "仪表",
            Self::History => "趋势",
            Self::Details => "明细",
        }
    }
    pub(crate) fn parse(key: &str) -> Self {
        Self::ALL.into_iter().find(|v| v.key() == key).unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Reading {
    pub cpu: Option<f32>,
    pub gpu: Option<f32>,
    pub memory: Option<(u64, u64)>,
    pub gpu_name: String,
    pub gpu_temp: Option<f32>,
    pub video_memory: Option<(u64, u64)>,
    pub network: [Option<f32>; 2],
    pub disk: [Option<f32>; 2],
}
impl Reading {
    pub fn ram(&self) -> Option<f32> {
        self.memory
            .filter(|&(_, total)| total > 0)
            .map(|(used, total)| 100. * used as f32 / total as f32)
    }
}

#[derive(Default)]
pub(crate) struct Monitor {
    pub latest: Reading,
    pub history: VecDeque<(Instant, Reading)>,
    pub chart: usize,
    #[cfg(windows)]
    worker: Option<Worker>,
}

#[cfg(windows)]
struct Worker {
    stop: std::sync::mpsc::Sender<()>,
    visible: std::sync::Arc<std::sync::atomic::AtomicBool>,
    samples: std::sync::Arc<std::sync::Mutex<VecDeque<(Instant, Reading)>>>,
}
#[cfg(windows)]
impl Worker {
    fn start(ctx: egui::Context, visible: bool) -> Self {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc, Mutex,
        };
        let (stop, stopped) = mpsc::channel();
        let samples = Arc::new(Mutex::new(VecDeque::with_capacity(60)));
        let output = Arc::clone(&samples);
        let visible = Arc::new(AtomicBool::new(visible));
        let repaint = Arc::clone(&visible);
        std::thread::spawn(move || {
            let mut sampler = windows::Sampler::default();
            loop {
                let at = Instant::now();
                let reading = sampler.read(at);
                {
                    let mut queue = output.lock().unwrap();
                    if queue.len() == 60 {
                        queue.pop_front();
                    }
                    queue.push_back((at, reading));
                }
                if repaint.load(Ordering::Relaxed) {
                    ctx.request_repaint();
                }
                let wait = Duration::from_secs(1).saturating_sub(at.elapsed());
                if !matches!(stopped.recv_timeout(wait), Err(mpsc::RecvTimeoutError::Timeout)) {
                    break;
                }
            }
        });
        Self { stop, visible, samples }
    }
}
#[cfg(windows)]
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

impl Monitor {
    pub fn tick(&mut self, enabled: bool, visible: bool, ctx: &egui::Context) {
        #[cfg(windows)]
        {
            if !enabled {
                if self.worker.take().is_some() {
                    self.latest = Reading::default();
                    self.history.clear();
                }
                return;
            }
            let worker = self.worker.get_or_insert_with(|| Worker::start(ctx.clone(), visible));
            worker.visible.store(visible, std::sync::atomic::Ordering::Relaxed);
            let mut queue = worker.samples.lock().unwrap();
            if let Some((_, last)) = queue.back() {
                self.latest = last.clone();
            } else {
                return;
            }
            let now = Instant::now();
            self.history.extend(queue.drain(..));
            while self
                .history
                .front()
                .is_some_and(|(at, _)| self.history.len() > 60 || now.duration_since(*at) >= Duration::from_secs(60))
            {
                self.history.pop_front();
            }
        }
        #[cfg(not(windows))]
        let _ = (enabled, visible, ctx);
    }
}

#[cfg(all(windows, test))]
mod tests {
    use super::*;
    use ::windows::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetProcessTimes},
    };

    fn cpu_seconds() -> f64 {
        let (mut created, mut ended, mut kernel, mut user) = Default::default();
        unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut ended, &mut kernel, &mut user) }.unwrap();
        let ticks = |t: FILETIME| u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime);
        (ticks(kernel) + ticks(user)) as f64 / 1e7
    }

    #[test]
    #[ignore = "单独运行以测量实际系统采样的 CPU 时间"]
    fn continuous_sampling_cost() {
        let ctx = egui::Context::default();
        let mut monitor = Monitor::default();
        monitor.tick(true, false, &ctx);
        std::thread::sleep(Duration::from_secs(3));
        monitor.tick(true, false, &ctx);
        let wall = Instant::now();
        let cpu = cpu_seconds();
        std::thread::sleep(Duration::from_secs(45));
        monitor.tick(true, true, &ctx);
        let elapsed = wall.elapsed().as_secs_f64();
        let used = cpu_seconds() - cpu;
        let count = monitor.history.iter().filter(|(t, _)| *t >= wall).count();
        println!(
            "elapsed={elapsed:.3} cpu_seconds={used:.4} one_core_pct={:.3} samples={count}",
            100. * used / elapsed
        );
        assert!((43..=46).contains(&count));
        assert!(monitor.latest.cpu.is_some() && monitor.latest.memory.is_some());
    }
}
