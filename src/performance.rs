//! 系统性能采样只在相应指标可见时进行，绘制线程只交换请求和读数。
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Demand {
    Bar(BarStyle),
    Page,
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
    request: std::sync::mpsc::Sender<Demand>,
    result: std::sync::mpsc::Receiver<(Instant, Reading)>,
    pending: bool,
    next: Instant,
}
#[cfg(windows)]
impl Worker {
    fn start(ctx: egui::Context) -> Self {
        use std::sync::mpsc::channel;
        let (request, requests) = channel();
        let (results, result) = channel();
        std::thread::spawn(move || {
            let mut sampler = windows::Sampler::default();
            while let Ok(mode) = requests.recv() {
                let at = Instant::now();
                if results.send((at, sampler.read(mode, at))).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self {
            request,
            result,
            pending: false,
            next: Instant::now(),
        }
    }
}

impl Monitor {
    pub fn tick(&mut self, demand: Option<Demand>, ctx: &egui::Context) {
        #[cfg(windows)]
        {
            let Some(mode) = demand else {
                if self.worker.take().is_some() {
                    self.latest = Reading::default();
                }
                return;
            };
            let worker = self.worker.get_or_insert_with(|| Worker::start(ctx.clone()));
            let now = Instant::now();
            if let Ok((at, sample)) = worker.result.try_recv() {
                worker.pending = false;
                self.latest = sample.clone();
                self.history.retain(|(t, _)| now.duration_since(*t) < Duration::from_secs(60));
                self.history.push_back((at, sample));
                if self.history.len() > 60 {
                    self.history.pop_front();
                }
            }
            if now >= worker.next && !worker.pending {
                worker.pending = worker.request.send(mode).is_ok();
                worker.next = now + Duration::from_secs(1);
            }
            if !worker.pending {
                ctx.request_repaint_after(worker.next.saturating_duration_since(now));
            }
        }
        #[cfg(not(windows))]
        let _ = (demand, ctx);
    }
}
