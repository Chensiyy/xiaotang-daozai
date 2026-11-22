//! 收起时与时钟并排的日期，按宿主日期计算，不依赖插件或网络。

use chrono::{Datelike, NaiveDate};
use eframe::egui::{self, Color32};

use crate::{
    lunar_calendar::{self, LunarDate},
    model::ResidentDate,
    paint::{self, Card, H},
    theme::{self, size, W},
};

const CENTER_Y: f32 = 15.0;
const DATE_SIZE: f32 = 9.0;
const LUNAR_SIZE: f32 = 7.8;
const TERM_SIZE: f32 = 7.0;
const CLOCK_GAP: f32 = 12.0;
const TERM_GAP: f32 = 3.0;
const TERM_PAD: f32 = 6.0;
const MUTED: Color32 = Color32::from_rgb(190, 191, 200);
const TERM_BG: Color32 = Color32::from_rgb(27, 45, 63);

pub(crate) struct ResidentDay {
    gregorian: String,
    short_gregorian: String,
    lunar: Option<LunarDate>,
}

impl ResidentDay {
    pub(crate) fn new(mode: ResidentDate, date: NaiveDate) -> Option<Self> {
        if mode == ResidentDate::Off {
            return None;
        }
        let lunar = if mode == ResidentDate::Lunar {
            lunar_calendar::for_date(date)
        } else {
            None
        };
        Some(Self {
            gregorian: format!("{}月{}日", date.month(), date.day()),
            short_gregorian: format!("{}/{}", date.month(), date.day()),
            lunar,
        })
    }

    pub(crate) fn group_width(&self, ctx: &egui::Context, clock: Option<&str>) -> f32 {
        let measure = |text: &str, size: f32, weight: W| {
            ctx.fonts(|fonts| {
                fonts
                    .layout_no_wrap(text.to_owned(), theme::font(size, weight), theme::WHITE)
                    .size()
                    .x
            })
        };
        let gregorian = measure(&self.gregorian, DATE_SIZE, W::Semibold);
        let lunar = self.lunar.as_ref().map_or(0.0, |lunar| {
            measure(&lunar.label, LUNAR_SIZE, W::Regular)
                + lunar.solar_term.map_or(0.0, |term| {
                    TERM_GAP + TERM_PAD + measure(term, TERM_SIZE, W::Semibold)
                })
        });
        let clock_space = clock.map_or(0.0, |clock| {
            let width = if let Some((period, digits)) = clock.split_once(' ') {
                measure(period, size::PILL_CLOCK * 0.72, W::Semibold)
                    + 4.0
                    + measure(digits, size::PILL_CLOCK, W::Bold)
            } else {
                measure(clock, size::PILL_CLOCK, W::Bold)
            };
            width + CLOCK_GAP
        });
        clock_space + gregorian.max(lunar)
    }

    /// 和设置中的常驻排序预览使用同一套位置和色彩。
    pub(crate) fn draw_group(&self, c: &Card, center_x: f32, clock: Option<&str>) {
        let left = center_x - self.group_width(c.ui.ctx(), clock) / 2.0;
        if let Some(clock) = clock {
            paint::clock(c, left, 21.0, clock, size::PILL_CLOCK);
        }
        self.draw_after_clock(c, left, clock);
    }

    pub(crate) fn draw_after_clock(&self, c: &Card, left: f32, clock: Option<&str>) {
        let x = if let Some(clock) = clock {
            let right = left + paint::clock_width(c, clock, size::PILL_CLOCK);
            c.seg(
                (right + 6.0, 7.0),
                (right + 6.0, 23.0),
                0.6,
                Color32::from_gray(68),
            );
            right + CLOCK_GAP
        } else {
            left
        };
        let Some(lunar) = self.lunar.as_ref() else {
            c.centered_text(
                x,
                CENTER_Y,
                &self.gregorian,
                DATE_SIZE,
                W::Semibold,
                theme::WHITE,
                H::Left,
            );
            return;
        };
        c.centered_text(
            x,
            9.5,
            &self.gregorian,
            DATE_SIZE,
            W::Semibold,
            theme::WHITE,
            H::Left,
        );
        c.centered_text(
            x,
            20.5,
            &lunar.label,
            LUNAR_SIZE,
            W::Regular,
            MUTED,
            H::Left,
        );
        if let Some(term) = lunar.solar_term {
            let badge_left = x + c.text_w(&lunar.label, LUNAR_SIZE, W::Regular) + TERM_GAP;
            Self::term_badge(c, badge_left, 20.5, term);
        }
    }

    fn term_badge(c: &Card, left: f32, y: f32, term: &str) {
        let width = c.text_w(term, TERM_SIZE, W::Semibold) + TERM_PAD;
        c.fill(left, y - 4.4, width, 8.8, 3.0, TERM_BG);
        c.centered_text(
            left + width / 2.0,
            y,
            term,
            TERM_SIZE,
            W::Semibold,
            theme::WHITE,
            H::Center,
        );
    }

    /// 侧停靠的窄条使用月/日，过宽的农历按月和日拆行。
    pub(crate) fn draw_side(&self, c: &Card, x: f32, y: f32) {
        let Some(lunar) = self.lunar.as_ref() else {
            c.centered_text(
                x,
                y,
                &self.short_gregorian,
                DATE_SIZE,
                W::Semibold,
                theme::WHITE,
                H::Center,
            );
            return;
        };
        let split = if c.text_w(&lunar.label, LUNAR_SIZE, W::Regular) > 30.0 {
            lunar
                .label
                .find('月')
                .map(|i| lunar.label.split_at(i + '月'.len_utf8()))
        } else {
            None
        };
        let extra = u8::from(lunar.solar_term.is_some()) + u8::from(split.is_some());
        let first = y - 5.0 * (1.0 + f32::from(extra));
        c.centered_text(
            x,
            first,
            &self.short_gregorian,
            DATE_SIZE,
            W::Semibold,
            theme::WHITE,
            H::Center,
        );
        let mut term_y = first + 20.0;
        if let Some((month, day)) = split {
            c.centered_text(
                x,
                first + 10.0,
                month,
                LUNAR_SIZE,
                W::Regular,
                MUTED,
                H::Center,
            );
            c.centered_text(
                x,
                first + 20.0,
                day,
                LUNAR_SIZE,
                W::Regular,
                MUTED,
                H::Center,
            );
            term_y += 10.0;
        } else {
            c.centered_text(
                x,
                first + 10.0,
                &lunar.label,
                LUNAR_SIZE,
                W::Regular,
                MUTED,
                H::Center,
            );
        }
        if let Some(term) = lunar.solar_term {
            let left = x - (c.text_w(term, TERM_SIZE, W::Semibold) + TERM_PAD) / 2.0;
            Self::term_badge(c, left, term_y, term);
        }
    }
}
