use crate::{
    app::{App, PERFORMANCE_PAGE},
    paint::{Card, H},
    performance::{BarStyle, Reading, View},
    theme::W,
};
use eframe::egui::{self, Color32};
use std::time::Instant;

pub(crate) const CPU_COLOR: Color32 = Color32::from_rgb(99, 218, 207);
const GPU: Color32 = Color32::from_rgb(171, 156, 245);
const RAM: Color32 = Color32::from_rgb(233, 189, 117);
const BLUE: Color32 = Color32::from_rgb(126, 188, 244);
const INK: Color32 = Color32::from_rgb(235, 240, 243);
const MUTED: Color32 = Color32::from_rgb(159, 172, 181);
const GRID: Color32 = Color32::from_rgb(50, 58, 65);
const SURFACE: Color32 = Color32::from_rgb(28, 33, 38);
const COLORS: [Color32; 3] = [CPU_COLOR, GPU, RAM];
const NAMES: [&str; 3] = ["处理器", "显卡", "内存"];

fn percent(value: Option<f32>) -> String {
    value.map_or_else(|| "—".into(), |n| format!("{n:.0}%"))
}
fn speed(value: Option<f32>) -> String {
    let Some(n) = value else {
        return "—".into();
    };
    for (unit, factor) in [("GB/s", 1e9), ("MB/s", 1e6), ("KB/s", 1e3)] {
        if n >= factor {
            let v = n / factor;
            return if v >= 100. {
                format!("{v:.0} {unit}")
            } else {
                format!("{v:.1} {unit}")
            };
        }
    }
    format!("{n:.0} B/s")
}
fn capacity(value: Option<(u64, u64)>) -> String {
    value.map_or_else(
        || "—".into(),
        |(used, total)| format!("{:.1} / {:.1} GiB", used as f64 / 1073741824., total as f64 / 1073741824.),
    )
}
fn metric(r: &Reading, i: usize) -> Option<f32> {
    [r.cpu, r.gpu, r.ram()][i]
}
fn dial(c: &Card, x: f32, y: f32, r: f32, value: Option<f32>, color: Color32) {
    let start = 2.25;
    let sweep = 4.93;
    c.arc(x, y, r, start, start + sweep, 2.6, GRID);
    if let Some(v) = value.filter(|v| *v > 0.) {
        c.arc(x, y, r, start, start + sweep * v.clamp(0., 100.) / 100., 2.6, color);
    }
}

impl App {
    pub(crate) fn performance_on_bar(&self) -> bool {
        self.module_on(PERFORMANCE_PAGE) && self.settings.show_performance && !self.effective_minimal()
    }
    pub(crate) fn paint_performance_bar(&self, c: &Card, x: f32) {
        let r = &self.performance.latest;
        match self.settings.performance_bar {
            BarStyle::Gauges | BarStyle::Dashboard => {
                let dashboard = self.settings.performance_bar == BarStyle::Dashboard;
                let left_edge = x - self.settings.performance_bar.width() / 2.;
                if dashboard {
                    self.performance_plot(c, 0, [left_edge, 8., 26., 15.], 16.);
                    for (j, (values, labels, color)) in [(r.network, ["↓", "↑"], BLUE), (r.disk, ["读", "写"], RAM)]
                        .into_iter()
                        .enumerate()
                    {
                        let left = left_edge + 178. + j as f32 * 66.;
                        c.seg((left - 3., 7.), (left - 3., 23.), 0.7, GRID);
                        for i in 0..2 {
                            let cy = 9. + i as f32 * 12.;
                            c.centered_text(left, cy, labels[i], 9., W::Bold, color, H::Left);
                            c.centered_text(left + 60., cy, &speed(values[i]), 9., W::Bold, INK, H::Right);
                        }
                    }
                }
                for i in 0..3 {
                    let left = left_edge + if dashboard { 30. } else { 1. } + i as f32 * 49.;
                    let value = metric(r, i);
                    dial(c, left + 8., 15., 7.5, value, COLORS[i]);
                    c.centered_text(left + 8., 15., ["C", "G", "M"][i], 8., W::Bold, COLORS[i], H::Center);
                    c.centered_text(left + 18., 15., &percent(value), 11., W::Bold, INK, H::Left);
                }
            }
            BarStyle::Pulse => {
                c.centered_text(x - 38., 9., "CPU", 8., W::Bold, CPU_COLOR, H::Left);
                c.text(x - 38., 24., &percent(r.cpu), 12., W::Bold, INK, H::Left);
                c.fill(x - 5., 6., 42., 18., 4., SURFACE);
                self.performance_plot(c, 0, [x - 3., 9., 38., 12.], 16.);
            }
            BarStyle::Network => {
                for (i, (arrow, col)) in [("↓", CPU_COLOR), ("↑", BLUE)].into_iter().enumerate() {
                    let cy = 9. + i as f32 * 12.;
                    c.centered_text(x - 40., cy, arrow, 12., W::Bold, col, H::Left);
                    c.centered_text(x + 40., cy, &speed(r.network[i]), 10.5, W::Bold, INK, H::Right);
                }
            }
        }
    }
    pub(crate) fn performance_bar(&mut self, c: &Card, x: f32) {
        self.paint_performance_bar(c, x);
        let width = self.settings.performance_bar.width();
        if crate::quota_ui::bar_tooltip(
            c.hit(x - width / 2., 0., width, 30., "performance-open"),
            "打开系统性能".into(),
        )
        .clicked()
        {
            self.show_page(PERFORMANCE_PAGE);
        }
    }
    pub(crate) fn side_performance(&mut self, c: &Card, x: f32, y: f32) {
        let r = &self.performance.latest;
        match self.settings.performance_bar {
            BarStyle::Dashboard => {
                for i in 0..3 {
                    let cy = y - 10. + i as f32 * 10.;
                    c.centered_text(x - 15., cy, ["C", "G", "M"][i], 8., W::Bold, COLORS[i], H::Left);
                    c.centered_text(x + 15., cy, &percent(metric(r, i)), 9., W::Bold, INK, H::Right);
                }
            }
            BarStyle::Gauges => {
                for (i, (dx, dy)) in [(0., -6.), (-8., 7.), (8., 7.)].into_iter().enumerate() {
                    dial(c, x + dx, y + dy, 5., metric(r, i), COLORS[i]);
                }
            }
            BarStyle::Pulse => {
                c.centered_text(x, y - 6., "CPU", 8., W::Bold, CPU_COLOR, H::Center);
                c.centered_text(x, y + 6., &percent(r.cpu), 11., W::Bold, INK, H::Center);
            }
            BarStyle::Network => {
                for (i, col) in [CPU_COLOR, BLUE].into_iter().enumerate() {
                    let raw = speed(r.network[i]).replace("B/s", "").replace(' ', "");
                    let value = format!("{}{raw}", ["↓", "↑"][i]);
                    let size = 9.0 * (30.0 / c.text_w(&value, 9., W::Bold)).min(1.);
                    c.centered_text(x, y - 6. + 12. * i as f32, &value, size, W::Bold, col, H::Center);
                }
            }
        }
        if crate::quota_ui::bar_tooltip(c.hit(x - 16., y - 17., 32., 34., "side-performance"), "打开系统性能".into())
            .clicked()
        {
            self.show_page(PERFORMANCE_PAGE);
        }
    }

    fn performance_plot(&self, c: &Card, i: usize, rect: [f32; 4], seconds: f32) {
        let [x, y, w, h] = rect;
        let now = Instant::now();
        let mut last = None;
        for (at, sample) in &self.performance.history {
            let age = now.saturating_duration_since(*at).as_secs_f32();
            if age > seconds {
                continue;
            }
            let Some(v) = metric(sample, i) else {
                last = None;
                continue;
            };
            let p = (x + w * (1. - age / seconds), y + h * (1. - v.clamp(0., 100.) / 100.));
            if let Some((prior, _)) = last.filter(|&(_, t)| at.saturating_duration_since(t).as_secs_f32() < 2.5) {
                c.seg(prior, p, 1.6, COLORS[i]);
            } else {
                c.circle(p.0, p.1, 1., COLORS[i]);
            }
            last = Some((p, *at));
        }
    }
    pub(crate) fn page_performance(&mut self, c: &Card) {
        c.fill(37., 36., 145., 24., 8., SURFACE);
        for (i, view) in View::ALL.into_iter().enumerate() {
            let x = 39. + i as f32 * 47.;
            let active = self.settings.performance_view == view;
            if active {
                c.fill(x, 38., 46., 20., 6., GRID);
            }
            c.centered_text(
                x + 23.,
                48.,
                view.label(),
                11.,
                W::Bold,
                if active { INK } else { MUTED },
                H::Center,
            );
            if c.hit(x, 36., 46., 24., ("performance-view", i)).clicked() && !active {
                self.settings.performance_view = view;
                self.settings.save();
            }
        }
        c.circle(
            334.,
            48.,
            2.5,
            if self.performance.history.is_empty() {
                MUTED
            } else {
                CPU_COLOR
            },
        );
        c.centered_text(381., 48., "1 秒采样", 10.5, W::Regular, MUTED, H::Right);
        match self.settings.performance_view {
            View::Overview => self.performance_overview(c),
            View::History => self.performance_history(c),
            View::Details => self.performance_details(c),
        }
    }
    fn performance_overview(&self, c: &Card) {
        let r = &self.performance.latest;
        for i in 0..3 {
            let x = 37. + i as f32 * 117.;
            c.fill(x, 69., 110., 75., 10., SURFACE);
            c.text(x + 11., 87., NAMES[i], 10.5, W::Regular, MUTED, H::Left);
            dial(c, x + 22., 111., 11., metric(r, i), COLORS[i]);
            c.circle(x + 22., 111., 2.2, COLORS[i]);
            c.text(x + 42., 118., &percent(metric(r, i)), 20., W::Bold, INK, H::Left);
            self.performance_plot(c, i, [x + 9., 128., 92., 9.], 60.);
        }
        for (i, (label, values, arrows, color)) in
            [("网络", r.network, ["↓", "↑"], BLUE), ("磁盘", r.disk, ["读", "写"], RAM)]
                .into_iter()
                .enumerate()
        {
            let x = 37. + i as f32 * 176.;
            c.text(x, 162., label, 11., W::Bold, color, H::Left);
            for j in 0..2 {
                let base = 178. + 14. * j as f32;
                c.text(x, base, arrows[j], 10., W::Regular, MUTED, H::Left);
                c.text(x + 158., base, &speed(values[j]), 11., W::Bold, INK, H::Right);
            }
        }
    }
    fn performance_history(&mut self, c: &Card) {
        let r = &self.performance.latest;
        for i in 0..3 {
            let x = 37. + i as f32 * 117.;
            let active = i == self.performance.chart;
            c.fill(x, 69., 110., 30., 6., if active { SURFACE } else { Color32::TRANSPARENT });
            if active {
                c.fill(x + 2., 78., 2., 12., 1., COLORS[i]);
            }
            c.centered_text(
                x + 10.,
                84.,
                NAMES[i],
                11.,
                W::Regular,
                if active { INK } else { MUTED },
                H::Left,
            );
            c.centered_text(x + 99., 84., &percent(metric(r, i)), 13., W::Bold, COLORS[i], H::Right);
            if c.hit(x, 69., 110., 30., ("performance-metric", i)).clicked() {
                self.performance.chart = i;
            }
        }
        for (i, label) in ["100%", "50%", "0%"].into_iter().enumerate() {
            let y = 111. + i as f32 * 35.;
            c.centered_text(65., y, label, 9., W::Regular, MUTED, H::Right);
            c.seg((72., y), (381., y), 0.6, GRID);
        }
        self.performance_plot(c, self.performance.chart, [72., 111., 309., 70.], 60.);
        c.text(72., 198., "60 秒前", 10., W::Regular, MUTED, H::Left);
        c.text(381., 198., "现在", 10., W::Regular, MUTED, H::Right);
    }
    fn performance_details(&self, c: &Card) {
        let r = &self.performance.latest;
        let rows = [
            ("内存占用", capacity(r.memory), "专用显存", capacity(r.video_memory)),
            (
                "处理器",
                percent(r.cpu),
                "显卡温度",
                r.gpu_temp.map_or_else(|| "—".into(), |n| format!("{n:.0} °C")),
            ),
            ("网络下行", speed(r.network[0]), "网络上行", speed(r.network[1])),
            ("磁盘读取", speed(r.disk[0]), "磁盘写入", speed(r.disk[1])),
        ];
        let name = if r.gpu_name.is_empty() {
            "显卡型号未知"
        } else {
            &r.gpu_name
        };
        c.text(37., 81., &c.fit(name, 12., W::Bold, 344.), 12., W::Bold, GPU, H::Left);
        for (i, (left, a, right, b)) in rows.iter().enumerate() {
            let y = 101. + i as f32 * 28.;
            for (j, (label, val)) in [(*left, a), (*right, b)].into_iter().enumerate() {
                let x = 37. + j as f32 * 177.;
                c.centered_text(x, y, label, 10.5, W::Regular, MUTED, H::Left);
                c.centered_text(x + 166., y, val, 11.5, W::Bold, INK, H::Right);
                if i < 3 {
                    c.seg((x, y + 13.), (x + 166., y + 13.), 0.6, GRID);
                }
            }
        }
    }
    pub(crate) fn performance_settings(&mut self, ui: &mut egui::Ui) {
        use crate::extras::{setting_row, settings_card, settings_label};
        settings_label(ui, "常驻显示");
        settings_card(ui, |ui| {
            setting_row(
                ui,
                "显示在岛上",
                "点击指标可展开系统性能",
                &mut self.settings.show_performance,
            );
            ui.separator();
            ui.add_enabled_ui(self.settings.show_performance, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for style in BarStyle::ALL {
                        ui.selectable_value(&mut self.settings.performance_bar, style, style.label());
                    }
                });
                let hint = match self.settings.performance_bar {
                    BarStyle::Gauges => "依次显示处理器、显卡和内存占用率。",
                    BarStyle::Pulse => "显示处理器占用率和最近 16 秒的负载曲线。",
                    BarStyle::Dashboard => "横向并排显示负载曲线、三项占用率、网速和磁盘读写；侧边停靠时显示三项占用率。",
                    BarStyle::Network => "分别显示物理网卡的下载与上传速度。",
                };
                ui.weak(hint);
            });
        });
        settings_label(ui, "展开视图");
        settings_card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for view in View::ALL {
                    ui.selectable_value(&mut self.settings.performance_view, view, view.label());
                }
            });
            ui.weak("可在岛内切换视图。硬件未提供的读数显示为 —。");
        });
    }
}
