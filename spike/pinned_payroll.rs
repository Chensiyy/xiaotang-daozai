use crate::{
    paint::{Card, H},
    plugin_canvas::live::Counter,
    plugin_canvas_ui::CanvasTheme,
    skin_paint,
    theme::{self, W},
};
use chrono::{DateTime, Utc};
use eframe::egui::Color32;
use std::time::Duration;

const FLIP_SECONDS: f64 = 0.2;
const FLIP_FRAME: Duration = Duration::from_millis(33);
const CELL: f32 = 8.0;
const TOP_ICON: f32 = 19.0;
const DOT: f32 = 3.5;
const GREEN: Color32 = Color32::from_rgb(61, 220, 132);

#[derive(Clone)]
struct Digits {
    current: String,
    previous: String,
    changed_at: f64,
    last_pass: u64,
}

pub(super) fn width(counter: &Counter, now: DateTime<Utc>) -> f32 {
    let reading = |second| format!("{:.2}", counter.sample(second).0.clamp(0.0, 10_000_000.0));
    let length = reading(now.timestamp())
        .len()
        .max(reading(now.timestamp() + 1).len());
    TOP_ICON + CELL * (length as f32 - 1.0) + DOT + 2.0
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    c: &Card<'_>,
    x: f32,
    y: f32,
    width: f32,
    vertical: bool,
    theme: CanvasTheme,
    counter: &Counter,
    now: DateTime<Utc>,
) -> (Option<Duration>, bool) {
    let (value, second) = counter.sample(now.timestamp());
    let amount = format!("{value:.2}");
    let height = if vertical { 40.0 } else { 22.0 };
    let area = c.rect(x - width / 2.0, y - height / 2.0, width, height);
    if !area.intersect(c.painter.clip_rect()).is_positive() {
        return (None, false);
    }
    let ctx = c.ui.ctx();
    let time = c.ui.input(|input| input.time);
    let pass = ctx.cumulative_pass_nr();
    let id = c.ui.make_persistent_id(("live-pay-digits", vertical));
    let (previous, progress) = if c.live && c.ui.style().animation_time > 0.0 {
        ctx.data_mut(|data| {
            let digits = data.get_temp_mut_or_insert_with(id, || Digits {
                current: amount.clone(),
                previous: amount.clone(),
                changed_at: time,
                last_pass: pass,
            });
            if pass > digits.last_pass.saturating_add(1) {
                // 展开时常驻位不可见，重新出现时直接从当前金额开始。
                digits.current.clone_from(&amount);
                digits.previous.clone_from(&amount);
            } else if digits.current != amount {
                digits.previous = std::mem::replace(&mut digits.current, amount.clone());
                digits.changed_at = time;
            }
            digits.last_pass = pass;
            let elapsed = time - digits.changed_at;
            if digits.previous != digits.current && (0.0..FLIP_SECONDS).contains(&elapsed) {
                let t = (elapsed / FLIP_SECONDS) as f32;
                (Some(digits.previous.clone()), t * t * (3.0 - 2.0 * t))
            } else {
                (None, 1.0)
            }
        })
    } else {
        (None, 1.0)
    };
    let c = c.clipped(x - width / 2.0, y - height / 2.0, width, height);
    let map = |color| {
        theme
            .palette
            .map_or(color, |palette| skin_paint::map_color(color, &palette))
    };
    let background = map(theme::CARD);
    let ink = map(theme::WHITE);
    let dot = map(theme::GRAY);
    let coin = |center_x, center_y, radius, size| {
        c.circle(center_x, center_y, radius, GREEN);
        c.centered_text(
            center_x,
            center_y,
            "¥",
            size,
            W::Bold,
            Color32::from_rgb(13, 36, 23),
            H::Center,
        );
    };
    let row = |text: &str,
               offset: usize,
               center: f32,
               baseline: f32,
               size: f32,
               step: f32,
               last_green: bool| {
        let row_width = text
            .bytes()
            .map(|digit| if digit == b'.' { DOT } else { step })
            .sum::<f32>();
        let mut left = center - row_width / 2.0;
        for (index, digit) in text.bytes().enumerate() {
            let w = if digit == b'.' { DOT } else { step };
            let cx = left + w / 2.0;
            let color = if digit == b'.' {
                dot
            } else if last_green && index + 1 == text.len() {
                GREEN
            } else {
                ink
            };
            let label = |card: &Card<'_>, value: u8, delta: f32| {
                let mut buffer = [0; 4];
                card.centered_text(
                    cx,
                    baseline + delta,
                    char::from(value).encode_utf8(&mut buffer),
                    size,
                    W::Bold,
                    color,
                    H::Center,
                );
            };
            if digit.is_ascii_digit() {
                let h = size + if vertical { 1.0 } else { 4.0 };
                c.fill(left + 0.5, baseline - h / 2.0, w - 1.0, h, 2.0, background);
                let old_digit = previous
                    .as_ref()
                    .and_then(|before| {
                        let from_right = amount.len() - offset - index;
                        before
                            .len()
                            .checked_sub(from_right)
                            .map(|pos| before.as_bytes()[pos])
                    })
                    .filter(u8::is_ascii_digit);
                if previous.is_some() && old_digit != Some(digit) {
                    let cell = c.clipped(left, baseline - h / 2.0, w, h);
                    if let Some(old) = old_digit {
                        label(&cell, old, -h * progress);
                    }
                    label(&cell, digit, h * (1.0 - progress));
                } else {
                    label(&c, digit, 0.0);
                }
                c.painter.line_segment(
                    [c.pos(left + 0.5, baseline), c.pos(left + w - 0.5, baseline)],
                    (0.65 * c.s, theme.background.gamma_multiply(0.45)),
                );
            } else {
                label(&c, digit, 1.0);
            }
            left += w;
        }
    };
    if vertical {
        let (whole, _) = amount.split_once('.').unwrap_or((&amount, ""));
        let cut = whole.len().saturating_sub(4);
        let whole_step = ((width - 2.0) / 4.0).clamp(1.0, 9.0);
        if cut == 0 {
            coin(x, y - 14.0, 5.0, 7.5);
        } else {
            row(&whole[..cut], 0, x, y - 14.0, 12.0, whole_step, false);
        }
        row(&whole[cut..], cut, x, y, 14.0, whole_step, false);
        row(
            &amount[whole.len()..],
            whole.len(),
            x,
            y + 14.0,
            11.0,
            7.0,
            true,
        );
    } else {
        let amount_width = CELL * (amount.len() as f32 - 1.0) + DOT;
        let left = x - (TOP_ICON + amount_width) / 2.0;
        coin(left + 7.0, y, 7.0, 10.5);
        row(
            &amount,
            0,
            left + TOP_ICON + amount_width / 2.0,
            y,
            12.0,
            CELL,
            true,
        );
    }

    let animating = previous.is_some();
    let repaint = c
        .live
        .then(|| {
            if animating {
                Some(FLIP_FRAME)
            } else {
                second.map(|next| {
                    Duration::from_millis((next * 1000 - now.timestamp_millis()).max(1) as u64)
                })
            }
        })
        .flatten();
    (repaint, animating)
}
