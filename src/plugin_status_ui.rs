//! 用岛仔画布绘制状态详情。
#[path = "plugin_status_controls.rs"]
mod controls;
pub(crate) use controls::{ClickTarget, DEFAULT_FORM_HEIGHT, Interaction, InteractionResponse};

use crate::{
    paint::{self, Card, H},
    plugin_status::{
        PluginStatus, ProgressVariant, StatusComponent, StatusDensity, StatusHealth, StatusLayout,
        StatusState, StatusSurface, MAX_ITEMS,
    },
    theme::{self, size, W},
};
use eframe::egui::Color32;
use std::f32::consts::{FRAC_PI_2, TAU};

pub(crate) const STATUS_PAGE_H: f32 = 236.0;

const LEFT: f32 = 42.0;
const WIDTH: f32 = 335.0;
const TEXT_RIGHT: f32 = 363.0;
const CONTENT_TOP: f32 = 42.0;
const PAGE_BOTTOM_SPACE: f32 = 16.0;
const MIN_LAYOUT_HEIGHT: f32 = 156.0;
const MAX_METRICS: usize = 6;
const LIST_ROW_HEIGHT: f32 = 22.0;

pub(crate) fn status_page_height(layout: Option<&StatusLayout>) -> f32 {
    let Some(layout) = layout else {
        return STATUS_PAGE_H;
    };
    let density = layout.style.density;
    let (padding, _) = spacing(density);
    // 给可出现的组件留足空间，读数缺失或断源时保留页面高度。
    let height = stack_height(layout.components.iter().copied(), density, MAX_METRICS, MAX_ITEMS);
    (CONTENT_TOP + padding * 2.0 + height + PAGE_BOTTOM_SPACE).max(MIN_LAYOUT_HEIGHT)
}

pub(crate) fn draw_status_page(
    c: &Card,
    status: Option<&PluginStatus>,
    health: StatusHealth,
    layout: Option<&StatusLayout>,
) {
    let Some(status) = status.filter(|_| health == StatusHealth::Ready) else {
        draw_unavailable(c, health);
        return;
    };
    if let Some(layout) = layout {
        draw_layout(c, status, layout, None);
    } else {
        draw_default(c, status);
    }
}

pub(crate) fn draw_interactive_status_page(
    c: &Card,
    status: &PluginStatus,
    layout: Option<&StatusLayout>,
    interaction: &mut Interaction<'_>,
) {
    if let Some(layout) = layout {
        draw_layout(c, status, layout, Some(interaction));
    } else {
        draw_default(c, status);
        let y = STATUS_PAGE_H - PAGE_BOTTOM_SPACE;
        interaction.fields(c, status, LEFT + 14.0, y, TEXT_RIGHT - LEFT - 14.0);
        interaction.actions(
            c, status, LEFT + 14.0, y + controls::fields_height(status.fields.len()),
            TEXT_RIGHT - LEFT - 14.0,
        );
    }
}

fn draw_default(c: &Card, status: &PluginStatus) {
    let color = state_color(status.state);
    c.fill(LEFT, 42.0, WIDTH, 78.0, 9.0, theme::CARD);
    let head_x = if let Some(progress) = status.progress_pct {
        draw_progress_ring(c, progress, color, (84.0, 81.0), 27.0);
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

fn state_color(state: StatusState) -> Color32 {
    match state {
        StatusState::Running => theme::SOFT_GREEN,
        StatusState::Paused => theme::YELLOW,
        StatusState::Succeeded => theme::CYAN,
        StatusState::Error => theme::RED_TOP,
        StatusState::Offline | StatusState::Idle => theme::DIM,
    }
}

fn spacing(density: StatusDensity) -> (f32, f32) {
    match density {
        StatusDensity::Comfortable => (8.0, 6.0),
        StatusDensity::Compact => (6.0, 3.0),
    }
}

fn metric_row_height(density: StatusDensity) -> f32 {
    match density {
        StatusDensity::Comfortable => 18.0,
        StatusDensity::Compact => 15.0,
    }
}

fn component_height(component: StatusComponent, density: StatusDensity, metrics: usize, items: usize) -> f32 {
    match component {
        StatusComponent::State {} => 18.0,
        StatusComponent::Summary {} => 26.0,
        StatusComponent::Progress { variant } => match (variant, density) {
            (ProgressVariant::Ring, StatusDensity::Comfortable) => 60.0,
            (ProgressVariant::Ring, StatusDensity::Compact) => 52.0,
            (ProgressVariant::Bar, StatusDensity::Comfortable) => 26.0,
            (ProgressVariant::Bar, StatusDensity::Compact) => 22.0,
        },
        StatusComponent::Metrics { columns } => {
            metrics.div_ceil(usize::from(columns)) as f32 * metric_row_height(density)
        }
        StatusComponent::Alert {} => 30.0,
        StatusComponent::UpdatedAt {} => 14.0,
        StatusComponent::Actions {} => controls::ACTION_HEIGHT,
        StatusComponent::Fields {} => controls::fields_height(controls::MAX_FIELDS),
        StatusComponent::List {} => LIST_ROW_HEIGHT * items as f32,
    }
}

fn stack_height(
    components: impl Iterator<Item = StatusComponent>,
    density: StatusDensity,
    metrics: usize,
    items: usize,
) -> f32 {
    let (_, gap) = spacing(density);
    let height: f32 = components
        .map(|component| component_height(component, density, metrics, items) + gap)
        .sum();
    (height - gap).max(0.0)
}

fn draw_layout(
    c: &Card,
    status: &PluginStatus,
    layout: &StatusLayout,
    mut interaction: Option<&mut Interaction<'_>>,
) {
    let density = layout.style.density;
    let accent = layout.style.accent_rgb().map_or_else(
        || state_color(status.state),
        |[r, g, b]| Color32::from_rgb(r, g, b),
    );
    let components = layout
        .components
        .iter()
        .copied()
        .filter(|component| match component {
            StatusComponent::State {} => true,
            StatusComponent::Summary {} => status.summary.is_some(),
            StatusComponent::Progress { .. } => status.progress_pct.is_some(),
            StatusComponent::Metrics { .. } => !status.metrics.is_empty(),
            StatusComponent::Alert {} => status.alert.is_some(),
            StatusComponent::UpdatedAt {} => status.observed_at.is_some(),
            StatusComponent::Actions {} => !status.actions.is_empty(),
            StatusComponent::Fields {} => !status.fields.is_empty(),
            StatusComponent::List {} => !status.items.is_empty(),
        });
    let height = stack_height(components.clone(), density, status.metrics.len(), status.items.len());
    if height == 0.0 {
        draw_message(c, "当前没有可显示的数据", None, theme::DIM);
        return;
    }
    let (padding, gap) = spacing(density);
    if layout.style.surface == StatusSurface::Card {
        c.fill(
            LEFT,
            CONTENT_TOP,
            WIDTH,
            height + padding * 2.0,
            9.0,
            theme::CARD,
        );
    }
    let mut y = CONTENT_TOP + padding;
    for component in components {
        let height = component_height(component, density, status.metrics.len(), status.items.len());
        let center_y = y + height / 2.0;
        let x = LEFT + 14.0;
        match component {
            StatusComponent::State {} => draw_state(c, status, accent, x, center_y),
            StatusComponent::Summary {} => {
                if let Some(summary) = status.summary.as_deref() {
                    draw_lines(c, summary, x, y + 7.0, TEXT_RIGHT - x, theme::GRAY);
                }
            }
            StatusComponent::Progress { variant } => {
                if let Some(progress) = status.progress_pct {
                    match variant {
                        ProgressVariant::Ring => draw_progress_ring(
                            c,
                            progress,
                            accent,
                            (LEFT + WIDTH / 2.0, center_y),
                            height / 2.0 - 4.0,
                        ),
                        ProgressVariant::Bar => draw_progress_bar(c, progress, accent, x, center_y),
                    }
                }
            }
            StatusComponent::Metrics { columns } => {
                draw_metrics(c, status, usize::from(columns), density, x, y);
            }
            StatusComponent::Alert {} => {
                if let Some(alert) = status.alert.as_deref() {
                    c.fill(
                        x,
                        y,
                        TEXT_RIGHT - x,
                        height,
                        7.0,
                        paint::alpha(theme::ORANGE, 0.14),
                    );
                    let text_width = TEXT_RIGHT - x - 24.0;
                    let lead = if c.text_w(alert, size::SMALL, W::Regular) <= text_width {
                        center_y
                    } else {
                        y + 9.0
                    };
                    c.circle(x + 9.0, lead, 2.4, theme::ORANGE);
                    draw_lines(c, alert, x + 16.0, lead, text_width, theme::ORANGE);
                }
            }
            StatusComponent::UpdatedAt {} => {
                if let Some(at) = status
                    .observed_at
                    .and_then(|at| chrono::DateTime::from_timestamp(at, 0))
                {
                    let time = at.with_timezone(&chrono::Local).format("%m-%d %H:%M");
                    c.centered_text(
                        x,
                        center_y,
                        &format!("更新于 {time}"),
                        size::TINY,
                        W::Regular,
                        theme::DIM,
                        H::Left,
                    );
                }
            }
            StatusComponent::Actions {} => {
                if let Some(interaction) = interaction.as_deref_mut() {
                    interaction.actions(c, status, x, y, TEXT_RIGHT - x);
                }
            }
            StatusComponent::Fields {} => {
                if let Some(interaction) = interaction.as_deref_mut() {
                    interaction.fields(c, status, x, y, TEXT_RIGHT - x);
                }
            }
            StatusComponent::List {} => {
                draw_list(c, status, x, y, TEXT_RIGHT - x, interaction.as_deref_mut());
            }
        }
        y += height + gap;
    }
}

fn draw_list(
    c: &Card,
    status: &PluginStatus,
    x: f32,
    y: f32,
    width: f32,
    mut interaction: Option<&mut Interaction<'_>>,
) {
    for (index, item) in status.items.iter().enumerate() {
        let top = y + index as f32 * LIST_ROW_HEIGHT;
        let width = interaction.as_deref_mut().map_or(width, |interaction| {
            interaction.list_target(c, item, index, x, top, width, LIST_ROW_HEIGHT)
        });
        let center = top + LIST_ROW_HEIGHT / 2.0;
        let mut left = x + 4.0;
        if let Some(state) = item.state {
            c.circle(left + 2.5, center, 2.5, state_color(state));
            left += 11.0;
        }
        let right = x + width - 4.0;
        let max_label = (right - left) * if item.detail.is_some() { 0.55 } else { 1.0 };
        let label = c.fit(&item.label, size::SMALL, W::Regular, max_label);
        let label_width = c.text_w(&label, size::SMALL, W::Regular);
        c.centered_text(left, center, &label, size::SMALL, W::Regular, theme::WHITE, H::Left);
        if let Some(detail) = &item.detail {
            let detail_x = left + label_width + 6.0;
            let detail = c.fit(detail, size::TINY, W::Regular, right - detail_x);
            c.centered_text(detail_x, center, &detail, size::TINY, W::Regular, theme::GRAY, H::Left);
        }
    }
}

fn draw_state(c: &Card, status: &PluginStatus, accent: Color32, x: f32, y: f32) {
    let device_offline = status.state == StatusState::Offline && status.label != "设备离线";
    let color = match status.state {
        StatusState::Paused | StatusState::Error | StatusState::Offline => {
            state_color(status.state)
        }
        _ => accent,
    };
    c.circle(x + 3.0, y, 2.8, color);
    let x = x + 12.0;
    let reserved = if device_offline { 58.0 } else { 0.0 };
    let label = c.fit(
        &status.label,
        size::TITLE,
        W::Semibold,
        TEXT_RIGHT - x - reserved,
    );
    c.centered_text(
        x,
        y,
        &label,
        size::TITLE,
        W::Semibold,
        theme::WHITE,
        H::Left,
    );
    if device_offline {
        c.centered_text(
            TEXT_RIGHT,
            y,
            "设备离线",
            size::TINY,
            W::Regular,
            theme::GRAY,
            H::Right,
        );
    }
}

fn draw_metrics(
    c: &Card,
    status: &PluginStatus,
    columns: usize,
    density: StatusDensity,
    x: f32,
    y: f32,
) {
    let gap = 12.0;
    let width = (TEXT_RIGHT - x - gap * (columns - 1) as f32) / columns as f32;
    let row_height = metric_row_height(density);
    for (index, metric) in status.metrics.iter().enumerate() {
        let left = x + (index % columns) as f32 * (width + gap);
        let center_y = y + (index / columns) as f32 * row_height + row_height / 2.0;
        let value = c.fit(&metric.value, size::BODY, W::Semibold, width * 0.55);
        let label_width = width - c.text_w(&value, size::BODY, W::Semibold) - 8.0;
        let label = c.fit(&metric.label, size::SMALL, W::Regular, label_width);
        c.centered_text(
            left,
            center_y,
            &label,
            size::SMALL,
            W::Regular,
            theme::GRAY,
            H::Left,
        );
        c.centered_text(
            left + width,
            center_y,
            &value,
            size::BODY,
            W::Semibold,
            theme::WHITE,
            H::Right,
        );
    }
}

fn draw_progress_bar(c: &Card, progress: f64, color: Color32, x: f32, y: f32) {
    let width = TEXT_RIGHT - x - 55.0;
    c.fill(x, y - 4.0, width, 8.0, 4.0, theme::TRACK);
    if progress > 0.0 {
        let filled = width * progress as f32 / 100.0;
        c.fill(x, y - 4.0, filled, 8.0, 4.0_f32.min(filled / 2.0), color);
    }
    c.centered_text(
        TEXT_RIGHT - 12.0,
        y,
        &progress_text(progress),
        17.0,
        W::Bold,
        theme::WHITE,
        H::Right,
    );
    c.centered_text(
        TEXT_RIGHT,
        y + 2.0,
        "%",
        9.0,
        W::Semibold,
        theme::GRAY,
        H::Right,
    );
}

fn draw_progress_ring(c: &Card, progress: f64, color: Color32, center: (f32, f32), radius: f32) {
    let (x, y) = center;
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
    let percent = progress_text(progress);
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

fn progress_text(progress: f64) -> String {
    if progress == 0.0 {
        "0".into()
    } else if progress < 1.0 {
        "<1".into()
    } else {
        format!("{:.0}", progress.floor())
    }
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
    draw_message(c, name, hint, color);
}

fn draw_message(c: &Card, name: &str, hint: Option<&str>, color: Color32) {
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
