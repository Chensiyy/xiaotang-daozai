//! 用岛仔画布绘制只读的状态详情。

use crate::{
    paint::{self, Card, H},
    plugin_status::{PluginStatus, StatusHealth, StatusState},
    theme::{self, size, W},
};
use eframe::egui::Color32;
use std::f32::consts::{FRAC_PI_2, TAU};

pub(crate) const STATUS_PAGE_H: f32 = 236.0;

const LEFT: f32 = 42.0;
const WIDTH: f32 = 335.0;
const TEXT_RIGHT: f32 = 363.0;

pub(crate) fn draw_status_page(c: &Card, status: Option<&PluginStatus>, health: StatusHealth) {
    let Some(status) = status.filter(|_| health == StatusHealth::Ready) else {
        draw_unavailable(c, health);
        return;
    };

    let color = match status.state {
        StatusState::Running => theme::SOFT_GREEN,
        StatusState::Paused => theme::YELLOW,
        StatusState::Succeeded => theme::CYAN,
        StatusState::Error => theme::RED_TOP,
        StatusState::Offline | StatusState::Idle => theme::DIM,
    };
    c.fill(LEFT, 42.0, WIDTH, 78.0, 9.0, theme::CARD);
    let head_x = if let Some(progress) = status.progress_pct {
        draw_progress(c, progress, color);
        128.0
    } else {
        56.0
    };
    let device_offline = status.state == StatusState::Offline && status.label != "设备离线";
    let title_width = TEXT_RIGHT - head_x - if device_offline { 73.0 } else { 12.0 };
    c.circle(head_x + 3.0, 61.0, 2.8, color);
    let label = c.fit(&status.label, size::TITLE, W::Semibold, title_width);
    c.centered_text(
        head_x + 12.0,
        61.0,
        &label,
        size::TITLE,
        W::Semibold,
        theme::WHITE,
        H::Left,
    );
    if device_offline {
        c.centered_text(
            TEXT_RIGHT,
            61.0,
            "设备离线",
            size::TINY,
            W::Regular,
            theme::GRAY,
            H::Right,
        );
    }
    if let Some(summary) = status.summary.as_deref() {
        draw_lines(c, summary, head_x, 81.0, TEXT_RIGHT - head_x, theme::GRAY);
    }
    if let Some(at) = status
        .observed_at
        .and_then(|at| chrono::DateTime::from_timestamp(at, 0))
    {
        let time = at.with_timezone(&chrono::Local).format("%m-%d %H:%M");
        c.centered_text(
            head_x,
            110.0,
            &format!("更新于 {time}"),
            size::TINY,
            W::Regular,
            theme::DIM,
            H::Left,
        );
    }

    for (i, metric) in status.metrics.iter().enumerate() {
        let x = 51.0 + (i % 2) as f32 * 165.0;
        let y = 133.0 + (i / 2) as f32 * 18.0;
        let value = c.fit(&metric.value, size::BODY, W::Semibold, 87.0);
        let label_width = 151.0 - c.text_w(&value, size::BODY, W::Semibold) - 8.0;
        let label = c.fit(&metric.label, size::SMALL, W::Regular, label_width);
        c.centered_text(x, y, &label, size::SMALL, W::Regular, theme::GRAY, H::Left);
        c.centered_text(
            x + 151.0,
            y,
            &value,
            size::BODY,
            W::Semibold,
            theme::WHITE,
            H::Right,
        );
    }
    if let Some(alert) = status.alert.as_deref() {
        let y = if status.metrics.is_empty() {
            131.0
        } else {
            128.0 + status.metrics.len().div_ceil(2) as f32 * 18.0
        };
        let text_width = TEXT_RIGHT - 63.0;
        let one_line = c.text_w(alert, size::SMALL, W::Regular) <= text_width;
        let (height, lead) = if one_line { (24.0, 12.0) } else { (30.0, 9.0) };
        c.fill(
            LEFT,
            y,
            WIDTH,
            height,
            7.0,
            paint::alpha(theme::ORANGE, 0.14),
        );
        c.circle(54.0, y + lead, 2.4, theme::ORANGE);
        draw_lines(c, alert, 63.0, y + lead, text_width, theme::ORANGE);
    }
}

fn draw_progress(c: &Card, progress: f64, color: Color32) {
    let (x, y, radius) = (84.0, 81.0, 27.0);
    c.track(x, y, radius, 4.0, theme::TRACK);
    if progress >= 100.0 {
        c.track(x, y, radius, 4.0, color);
    } else if progress > 0.0 {
        let start = -FRAC_PI_2;
        let end = start + TAU * progress as f32 / 100.0;
        c.arc(x, y, radius, start, end, 4.0, color);
        for angle in [start, end] {
            c.circle(
                x + radius * angle.cos(),
                y + radius * angle.sin(),
                2.0,
                color,
            );
        }
    }
    let percent = if progress == 0.0 {
        "0".into()
    } else if progress < 1.0 {
        "<1".into()
    } else {
        format!("{:.0}", progress.floor())
    };
    let size = if percent.len() > 2 { 19.0 } else { 22.0 };
    let width = c.text_w(&percent, size, W::Bold);
    let left = x - (width + c.text_w("%", 9.0, W::Semibold) + 1.0) / 2.0;
    c.centered_text(left, y, &percent, size, W::Bold, theme::WHITE, H::Left);
    c.centered_text(
        left + width + 1.0,
        y + 3.0,
        "%",
        9.0,
        W::Semibold,
        theme::GRAY,
        H::Left,
    );
}

fn draw_lines(c: &Card, text: &str, x: f32, y: f32, width: f32, color: Color32) {
    let mut lines = c.wrap(text, size::SMALL, W::Regular, width, 2);
    if lines.iter().map(|line| line.chars().count()).sum::<usize>() < text.chars().count() {
        if let Some(last) = lines.last_mut() {
            *last = c.fit(
                &format!("{}…", last.trim_end_matches('…')),
                size::SMALL,
                W::Regular,
                width,
            );
        }
    }
    for (i, line) in lines.iter().enumerate() {
        c.centered_text(
            x,
            y + i as f32 * 13.0,
            line,
            size::SMALL,
            W::Regular,
            color,
            H::Left,
        );
    }
}

fn draw_unavailable(c: &Card, health: StatusHealth) {
    let (name, hint, color) = match health {
        StatusHealth::Waiting | StatusHealth::Ready => ("等待数据", None, theme::DIM),
        StatusHealth::Unconfigured => {
            ("等待数据源授权", Some("可在插件设置中授权读取"), theme::DIM)
        }
        StatusHealth::Offline => (
            "数据源失联",
            Some("检查已授权的文件或本机接口"),
            theme::ORANGE,
        ),
        StatusHealth::Stale => ("状态已过期", None, theme::ORANGE),
        StatusHealth::Invalid => ("状态数据格式无效", None, theme::ORANGE),
    };
    c.fill(LEFT, 56.0, WIDTH, 78.0, 9.0, theme::CARD);
    let x = LEFT + WIDTH / 2.0;
    let y = if hint.is_some() { 86.0 } else { 95.0 };
    c.centered_text(x, y, name, size::TITLE, W::Semibold, color, H::Center);
    if let Some(hint) = hint {
        c.centered_text(
            x,
            107.0,
            hint,
            size::SMALL,
            W::Regular,
            theme::GRAY,
            H::Center,
        );
    }
}
