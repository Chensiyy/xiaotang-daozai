//! 主岛的透明交换链和桌面底片。

mod backdrop;
pub use backdrop::{Backdrop, CleanPlate, Geometry, ScreenRect};

pub const FALLBACK_ARG: &str = "--material-fallback";

pub fn configure_startup(
    options: &mut eframe::NativeOptions,
    compatible_rendering: bool,
    material_active: bool,
) {
    options.wgpu_options.direct_composition = compatible_rendering
        && material_active
        && !std::env::args_os().any(|arg| arg == FALLBACK_ARG);
}

pub fn report_startup_failure() {
    if let Some(error) = eframe::egui_wgpu::winit::take_direct_composition_error() {
        log(&format!("透明交换链初始化失败，退回普通绘制：{error}"));
    }
}

pub fn report_run_failure(result: &eframe::Result<()>) -> bool {
    report_startup_failure();
    if let Err(eframe::Error::Wgpu(eframe::egui_wgpu::WgpuError::DirectComposition(error))) = result {
        log(&format!("透明交换链运行失败，退回普通绘制：{error}"));
        true
    } else {
        false
    }
}

fn log(message: &str) {
    crate::resources::append_log(&crate::model::config_dir().join("diagnostics"), "material", message);
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
}
