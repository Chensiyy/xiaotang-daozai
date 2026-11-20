//! 主岛材质的装载、底片与绘制节拍。

mod backdrop;
mod controls;
mod pipeline;
pub use backdrop::{Backdrop, CleanPlate, Geometry, ScreenRect};
pub(crate) use controls::control;

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Pos2, Rect};

use crate::{app::FrameOut, model::Dock};
use controls::{Controls, Registration};
use pipeline::{Frame, Pipeline};

pub const FALLBACK_ARG: &str = "--material-fallback";

pub fn falling_back() -> bool {
    std::env::args_os().any(|arg| arg == FALLBACK_ARG)
}

pub fn debug_requested() -> bool {
    std::env::var_os("DAOZAI_MATERIAL_DEBUG").is_some_and(|value| value == "1") && !falling_back()
}

pub fn configure_startup(
    options: &mut eframe::NativeOptions,
    compatible_rendering: bool,
    material_active: bool,
) {
    options.wgpu_options.direct_composition =
        compatible_rendering && material_active && !falling_back();
}

pub fn take_startup_failure() -> Option<String> {
    let error = eframe::egui_wgpu::winit::take_direct_composition_error();
    if let Some(error) = &error {
        log(&format!("透明交换链初始化失败，退回普通绘制：{error}"));
    }
    error
}

pub fn report_run_failure(result: &eframe::Result<()>) -> bool {
    let _ = take_startup_failure();
    if let Err(eframe::Error::Wgpu(eframe::egui_wgpu::WgpuError::DirectComposition(error))) = result
    {
        log(&format!("透明交换链运行失败，退回普通绘制：{error}"));
        true
    } else {
        false
    }
}

fn log(message: &str) {
    crate::resources::append_log(
        &crate::model::config_dir().join("diagnostics"),
        "material",
        message,
    );
}

#[derive(Clone)]
pub struct Options {
    pub controls: bool,
    pub stiffness: f32,
    pub damping: f32,
    pub params: [f32; 32],
}

#[cfg(feature = "sdk-js")]
impl From<&crate::sdk_spike::local::MaterialSpec<'_>> for Options {
    fn from(spec: &crate::sdk_spike::local::MaterialSpec<'_>) -> Self {
        Self {
            controls: spec.controls,
            stiffness: spec.stiffness,
            damping: spec.damping,
            params: spec.params,
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            controls: true,
            stiffness: 1.0,
            damping: 0.5,
            params: [0.0; 32],
        }
    }
}

pub(crate) struct Layer {
    painter: egui::Painter,
    slot: egui::layers::ShapeIdx,
    body: Rect,
    capture: Rect,
    radius: f32,
    expansion: f32,
    notch: Option<(f32, f32)>,
    dock: Dock,
    artwork: Option<(egui::TextureId, Arc<egui::ColorImage>)>,
}

impl Layer {
    pub(crate) fn reserve(
        painter: &egui::Painter,
        canvas: Rect,
        body: Rect,
        radius: f32,
        expansion: f32,
        notch: Option<(f32, f32)>,
        dock: Dock,
        artwork: Option<&(Arc<egui::ColorImage>, egui::TextureHandle)>,
    ) -> Self {
        let padding = if dock.is_side() {
            egui::vec2(48.0, 64.0)
        } else {
            egui::vec2(64.0, 48.0)
        };
        Self {
            painter: painter.clone(),
            slot: painter.add(egui::Shape::Noop),
            body,
            capture: body.expand2(padding).intersect(canvas),
            radius,
            expansion,
            notch,
            dock,
            artwork: artwork.map(|(image, texture)| (texture.id(), Arc::clone(image))),
        }
    }
}

#[derive(Default)]
struct Cadence {
    last_activity: Option<Instant>,
    last_sample: Option<Instant>,
}

impl Cadence {
    fn interval(&mut self, now: Instant, active: bool) -> Duration {
        let last = self.last_activity.get_or_insert(now);
        if active {
            *last = now;
            Duration::from_secs_f64(1.0 / 60.0)
        } else if now.duration_since(*last) >= Duration::from_secs(120) {
            Duration::from_secs(1)
        } else {
            Duration::from_secs_f64(1.0 / 15.0)
        }
    }

    fn remaining(&self, now: Instant, interval: Duration) -> Duration {
        self.last_sample.map_or(Duration::ZERO, |at| {
            interval.saturating_sub(now.duration_since(at))
        })
    }
}

#[derive(Default)]
struct Budget(u32);

impl Budget {
    fn exceeded(&mut self, elapsed: Duration) -> bool {
        if elapsed > Duration::from_millis(4) {
            self.0 += 1;
        } else {
            self.0 = 0;
        }
        self.0 >= 120
    }
}

pub struct Runtime {
    pipeline: Pipeline,
    options: Options,
    backdrop: Option<Backdrop>,
    controls: Controls,
    registrations: Vec<Registration>,
    cadence: Cadence,
    budget: Budget,
    last_frame: Option<Instant>,
    pointer: Option<Pos2>,
    screen: Option<ScreenRect>,
    tint: Option<(egui::TextureId, [f32; 3])>,
    capture_ready: bool,
    has_drawn: bool,
}

impl Runtime {
    /// 用宿主已选定的材质创建管线。调用方负责先建立主岛透明交换链。
    pub fn load(
        render_state: &eframe::egui_wgpu::RenderState,
        shader: &str,
        options: Options,
    ) -> Result<Self, String> {
        if render_state.adapter.get_info().backend != eframe::wgpu::Backend::Dx12 {
            return Err("材质需要 Direct3D 12 兼容渲染".into());
        }
        Ok(Self {
            pipeline: Pipeline::new(render_state, shader)?,
            options,
            backdrop: None,
            controls: Controls::default(),
            registrations: Vec::new(),
            cadence: Cadence::default(),
            budget: Budget::default(),
            last_frame: None,
            pointer: None,
            screen: None,
            tint: None,
            capture_ready: false,
            has_drawn: false,
        })
    }

    pub fn debug(render_state: &eframe::egui_wgpu::RenderState) -> Result<Self, String> {
        let material = Self::load(render_state, pipeline::DEBUG_SHADER, Options::default())?;
        log("调试材质已加载");
        Ok(material)
    }

    pub fn set_options(&mut self, options: Options) {
        self.options = options;
    }

    pub fn capture_ready(&self) -> bool {
        self.capture_ready
    }

    fn clear_backdrop(&mut self) {
        self.backdrop = None;
        self.pipeline.clear_backdrop();
        self.screen = None;
    }

    pub fn draw(&mut self, draw_island: impl FnOnce() -> FrameOut) -> FrameOut {
        if self.options.controls {
            let (out, registrations) = controls::collect(draw_island);
            self.registrations = registrations;
            out
        } else {
            draw_island()
        }
    }

    /// 隐藏或锁屏期间释放桌面捕获，并在恢复后重新定位底片。
    pub fn suspend(&mut self) {
        if self.backdrop.is_some() {
            self.clear_backdrop();
            self.controls = Controls::default();
            self.registrations.clear();
            self.cadence = Cadence::default();
            self.budget = Budget::default();
            self.last_frame = None;
            self.pointer = None;
        }
    }

    pub fn finish(
        &mut self,
        ctx: &egui::Context,
        window: &mut crate::native::Window,
        out: &mut FrameOut,
        time: f64,
        screen_pointer: Option<(i32, i32)>,
    ) -> Result<Option<Duration>, String> {
        let Some(layer) = out.material_layer.take() else {
            return Ok(None);
        };
        if ctx.will_discard() {
            return Ok(None);
        }
        let hwnd = window.material_window().ok_or("透明交换链不可用")?;
        if !unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd) }.as_bool() {
            return Ok(Some(Duration::from_millis(20)));
        }
        let started = Instant::now();
        if window.ensure_material_capture_exclusion()? {
            // 原生样式恢复后，旧的抓屏会话可能含有岛体画面。
            self.clear_backdrop();
        }
        self.capture_ready = true;
        let ppp = ctx.pixels_per_point();
        let Some((geometry, viewport)) =
            Geometry::for_window(hwnd, layer.body, layer.capture, ppp)?
        else {
            return Ok(Some(self.cadence.interval(started, out.animating)));
        };
        let [bx, by, bw, bh] = geometry.body;
        let body_pixels = Rect::from_min_size(egui::pos2(bx, by), egui::vec2(bw, bh));
        let pointer_in_island = screen_pointer
            .map(|(x, y)| {
                egui::pos2(
                    (x - geometry.screen.x) as f32,
                    (y - geometry.screen.y) as f32,
                )
            })
            .filter(|&p| body_pixels.contains(p));
        let moved = self.pointer != pointer_in_island;
        self.pointer = pointer_in_island;
        let dt = self
            .last_frame
            .replace(started)
            .map_or(1.0 / 60.0, |at| (started - at).as_secs_f32());
        let moving = out.animating
            || moved
            || pointer_in_island.is_some()
                && ctx.input(|i| i.pointer.any_down() || i.pointer.any_released());
        let capture_origin = (viewport.min.to_vec2() * ppp).round().to_pos2();
        let controls = self.controls.frame(
            ctx,
            std::mem::take(&mut self.registrations),
            ppp,
            capture_origin,
            dt,
            self.options.stiffness,
            self.options.damping,
        );
        let interval = self.cadence.interval(started, moving || controls.animating);
        let backdrop = self.backdrop.get_or_insert_with(Backdrop::default);
        let plate = if self.screen != Some(geometry.screen)
            || self.cadence.remaining(started, interval).is_zero()
        {
            self.cadence.last_sample = Some(started);
            self.screen = Some(geometry.screen);
            backdrop.capture(geometry).map_err(str::to_owned)?
        } else {
            backdrop.current(geometry)
        };

        let shape = if let Some(plate) = plate {
            let mut frame = Frame::default();
            let [x, y, w, h] = [
                geometry.screen.x,
                geometry.screen.y,
                geometry.screen.width,
                geometry.screen.height,
            ];
            frame.capture = [x as f32, y as f32, w as f32, h as f32];
            frame.body = geometry.body;
            let pointer = pointer_in_island.unwrap_or(egui::pos2(-1.0, -1.0));
            frame.shape = [layer.radius * ppp, layer.expansion, pointer.x, pointer.y];
            let (flare, height) = layer.notch.unwrap_or_default();
            frame.notch = [
                flare * ppp,
                height * ppp,
                u8::from(layer.notch.is_some()) as f32,
                0.0,
            ];
            frame.env = [time as f32, plate.brightness, ppp, 1.0];
            if let Some((id, image)) = layer.artwork {
                if !self.tint.is_some_and(|(known, _)| known == id) {
                    self.tint = Some((id, crate::fluid::Fluid::tint_from_image(&image)));
                }
                if let Some((_, [red, green, blue])) = self.tint {
                    frame.tint = [red, green, blue, 1.0];
                }
            }
            frame.count[0] = controls.count;
            frame.ctl_rect = controls.rects;
            frame.ctl_shape = controls.shapes;
            frame.ctl_spring = controls.springs;
            for (dest, source) in frame.params.iter_mut().flatten().zip(self.options.params) {
                *dest = source;
            }
            Some(self.pipeline.callback(viewport, frame, plate, layer.dock)?)
        } else {
            self.pipeline.clear_backdrop();
            None
        };

        if self.budget.exceeded(started.elapsed()) {
            return Err("材质通道连续 120 帧超过 4 ms".into());
        }
        if let Some(shape) = shape {
            layer.painter.set(layer.slot, shape);
            if !self.has_drawn {
                self.has_drawn = true;
                log(&format!(
                    "材质首帧就绪（{} × {}）",
                    geometry.screen.width, geometry.screen.height
                ));
            }
        }
        Ok(Some(self.cadence.remaining(Instant::now(), interval)))
    }

    pub fn report_failure(error: &str) {
        log(&format!("材质已停用，退回普通绘制：{error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_swapchain_requires_compatible_rendering_and_an_active_material() {
        let mut options = eframe::NativeOptions::default();
        assert!(!options.wgpu_options.direct_composition);
        configure_startup(&mut options, true, true);
        assert!(options.wgpu_options.direct_composition);
        configure_startup(&mut options, true, false);
        assert!(!options.wgpu_options.direct_composition);
        configure_startup(&mut options, false, true);
        assert!(!options.wgpu_options.direct_composition);
    }

    #[test]
    fn material_refresh_slows_after_two_minutes_and_wakes_on_activity() {
        let start = Instant::now();
        let mut cadence = Cadence::default();
        let idle = Duration::from_secs_f64(1.0 / 15.0);
        assert_eq!(cadence.interval(start, false), idle);
        assert_eq!(
            cadence.interval(start + Duration::from_secs(119), false),
            idle
        );
        let wake = start + Duration::from_secs(120);
        assert_eq!(cadence.interval(wake, false), Duration::from_secs(1));
        assert_eq!(
            cadence.interval(wake, true),
            Duration::from_secs_f64(1.0 / 60.0)
        );
        assert_eq!(cadence.interval(wake + Duration::from_secs(1), false), idle);
    }

    #[test]
    fn material_budget_only_trips_after_120_consecutive_slow_frames() {
        let slow = Duration::from_millis(5);
        let mut budget = Budget::default();
        for _ in 0..119 {
            assert!(!budget.exceeded(slow));
        }
        assert!(!budget.exceeded(Duration::from_millis(4)));
        for _ in 0..119 {
            assert!(!budget.exceeded(slow));
        }
        assert!(budget.exceeded(slow));
    }
}
