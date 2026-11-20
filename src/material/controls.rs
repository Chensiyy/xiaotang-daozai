//! 按实际绘制位置登记主岛控件，将交互和弹簧状态打包为材质输入。

use crate::anim::Spring;
use eframe::egui::{Context, Id, IdMap, Pos2, Rect, Vec2};
use std::cell::RefCell;

const MAX_CONTROLS: usize = 16;
const PRESS_SCALE: f32 = 0.14;

#[derive(Clone, Copy)]
pub(super) struct Registration {
    id: Id,
    rect: Rect,
    radius: f32,
}

thread_local! {
    static REGISTRATIONS: RefCell<Option<Vec<Registration>>> = const { RefCell::new(None) };
}

/// 在主岛绘制时登记控件。无材质或在其他窗口绘制时没有活动收集器。
pub(crate) fn control(id: Id, rect: Rect, radius: f32) {
    REGISTRATIONS.with_borrow_mut(|registrations| {
        let Some(registrations) = registrations.as_mut() else {
            return;
        };
        if !rect.is_finite() || !rect.is_positive() {
            return;
        }
        let entry = Registration { id, rect, radius };
        if let Some(existing) = registrations.iter_mut().find(|entry| entry.id == id) {
            *existing = entry;
        } else {
            registrations.push(entry);
        }
    });
}

pub(super) fn collect<R>(draw: impl FnOnce() -> R) -> (R, Vec<Registration>) {
    struct Restore(Option<Vec<Registration>>);

    impl Drop for Restore {
        fn drop(&mut self) {
            REGISTRATIONS.with_borrow_mut(|registrations| *registrations = self.0.take());
        }
    }

    let _restore = Restore(REGISTRATIONS.with_borrow_mut(|slot| slot.replace(Vec::new())));
    let output = draw();
    let registrations = REGISTRATIONS.with_borrow_mut(|slot| slot.take().unwrap_or_default());
    (output, registrations)
}

#[derive(Default)]
pub(super) struct ControlFrame {
    pub(super) count: u32,
    pub(super) rects: [[f32; 4]; MAX_CONTROLS],
    pub(super) shapes: [[f32; 4]; MAX_CONTROLS],
    pub(super) springs: [[f32; 4]; MAX_CONTROLS],
    pub(super) animating: bool,
}

struct Motion([Spring; 4]);

impl Motion {
    fn advance(
        &mut self,
        offset: Vec2,
        hovered: bool,
        pressed: bool,
        dt: f32,
        stiffness: f32,
        damping: f32,
    ) -> bool {
        let targets = [
            offset.x,
            offset.y,
            u8::from(hovered) as f32,
            u8::from(pressed) as f32,
        ];
        let mut animating = false;
        for (spring, target) in self.0.iter_mut().zip(targets) {
            spring.target = target;
            spring.tune(stiffness, damping);
            spring.step(dt);
            if spring.settled(1.0) {
                spring.snap(target);
            } else {
                animating = true;
            }
        }
        animating
    }
}

#[derive(Default)]
pub(super) struct Controls {
    motions: IdMap<Motion>,
}

impl Controls {
    pub(super) fn frame(
        &mut self,
        ctx: &Context,
        mut registrations: Vec<Registration>,
        ppp: f32,
        capture_origin_px: Pos2,
        dt: f32,
        stiffness: f32,
        damping: f32,
    ) -> ControlFrame {
        registrations.sort_by(|a, b| b.rect.area().total_cmp(&a.rect.area()));
        registrations.truncate(MAX_CONTROLS);
        self.motions
            .retain(|id, _| registrations.iter().any(|entry| entry.id == *id));
        let mut frame = ControlFrame::default();
        let stiffness = 300.0 * stiffness.clamp(0.2, 3.0);
        let damping = damping.clamp(0.1, 1.0);

        for entry in registrations {
            let Some(response) = ctx.read_response(entry.id) else {
                continue;
            };
            let pressed = response.enabled() && response.is_pointer_button_down_on();
            let hovered = response.enabled() && response.hovered();
            let offset =
                response
                    .interact_pointer_pos()
                    .filter(|_| pressed)
                    .map_or(Vec2::ZERO, |pointer| {
                        ((pointer - entry.rect.center()) * 0.14)
                            .clamp(Vec2::splat(-3.0), Vec2::splat(3.0))
                    });
            let motion = self
                .motions
                .entry(entry.id)
                .or_insert_with(|| Motion([Spring::new(0.0, stiffness, damping); 4]));
            frame.animating |= motion.advance(offset, hovered, pressed, dt, stiffness, damping);
            let [x, y, hover, press] = motion.0.map(|spring| spring.pos);
            let index = frame.count as usize;
            let min = entry.rect.min.to_vec2() * ppp - capture_origin_px.to_vec2();
            let size = entry.rect.size() * ppp;
            frame.rects[index] = [min.x, min.y, size.x, size.y];
            frame.shapes[index] = [
                entry
                    .radius
                    .clamp(0.0, entry.rect.width().min(entry.rect.height()) * 0.5)
                    * ppp,
                u8::from(response.enabled()) as f32,
                hover.clamp(0.0, 1.0),
                press.clamp(0.0, 1.0),
            ];
            frame.springs[index] = [
                x * ppp,
                y * ppp,
                1.0 + PRESS_SCALE * press.clamp(-0.3, 1.3),
                0.0,
            ];
            frame.count += 1;
        }

        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2, CentralPanel, RawInput, Sense};

    #[test]
    fn registration_is_scoped_and_reentrant() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(16.0));
        control(Id::new("outside"), rect, 2.0);
        let (_, outer) = collect(|| {
            control(Id::new("outer"), rect, 2.0);
            let (_, inner) = collect(|| control(Id::new("inner"), rect, 3.0));
            assert_eq!(inner.len(), 1);
            assert_eq!(inner[0].id, Id::new("inner"));
            control(Id::new("outer"), rect, 4.0);
        });
        assert_eq!(outer.len(), 1);
        assert_eq!(outer[0].id, Id::new("outer"));
        assert_eq!(outer[0].radius, 4.0);
        assert!(collect(|| {}).1.is_empty());
    }

    #[test]
    fn keeps_largest_controls_and_uses_physical_capture_origin() {
        let ctx = Context::default();
        let mut controls = Controls::default();
        let _ = ctx.run(RawInput::default(), |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                let (_, registrations) = collect(|| {
                    for width in 1..=20 {
                        let rect = Rect::from_min_size(pos2(20.0, 10.0), vec2(width as f32, 8.0));
                        let actual = ui.interact(rect, Id::new(width), Sense::click());
                        control(actual.id, rect, 3.0);
                    }
                });
                let frame =
                    controls.frame(ctx, registrations, 1.25, pos2(12.0, 4.0), 0.016, 1.0, 0.5);
                assert_eq!(frame.count, 16);
                assert_eq!(frame.rects[0], [13.0, 8.5, 25.0, 10.0]);
                assert_eq!(frame.rects[15][2], 6.25);
                assert_eq!(frame.shapes[0][0..2], [3.75, 1.0]);
                assert_eq!(frame.springs[0], [0.0, 0.0, 1.0, 0.0]);
            });
        });
    }

    #[test]
    fn a_steady_hover_stops_the_spring_repaint() {
        let mut motion = Motion([Spring::new(0.0, 300.0, 0.5); 4]);
        let dt = 1.0 / 60.0;
        assert!(motion.advance(vec2(2.0, -1.5), true, true, dt, 300.0, 0.5));
        assert_ne!(motion.0[0].pos, 0.0);
        for _ in 0..120 {
            motion.advance(Vec2::ZERO, true, false, dt, 300.0, 0.5);
        }
        assert!(!motion.advance(Vec2::ZERO, true, false, dt, 300.0, 0.5));
        assert_eq!(motion.0.map(|spring| spring.pos), [0.0, 0.0, 1.0, 0.0]);
    }
}
