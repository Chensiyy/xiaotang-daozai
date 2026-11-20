use bytemuck::{Pod, Zeroable};
use eframe::{
    egui::{self, epaint::PaintCallbackInfo},
    egui_wgpu::{self, Callback, CallbackTrait},
    wgpu,
};

use super::CleanPlate;
use crate::model::Dock;

const PRELUDE: &str = include_str!("prelude.wgsl");
pub(super) const DEBUG_SHADER: &str = include_str!("debug.wgsl");

/// 插件 API 1 的 uniform 布局。修改字段时同步核对 prelude 的偏移测试。
#[repr(C, align(16))]
#[derive(Clone, Copy, Default, Pod, Zeroable)]
pub(super) struct Frame {
    pub capture: [f32; 4],
    pub body: [f32; 4],
    pub shape: [f32; 4],
    pub notch: [f32; 4],
    pub env: [f32; 4],
    pub tint: [f32; 4],
    pub tex: [f32; 4],
    pub count: [u32; 4],
    pub ctl_rect: [[f32; 4]; 16],
    pub ctl_shape: [[f32; 4]; 16],
    pub ctl_spring: [[f32; 4]; 16],
    pub params: [[f32; 4]; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    frame: Frame,
    dock: [u32; 4],
}

const FRAME_SIZE: u64 = size_of::<Frame>() as u64;
const HOST_SIZE: u64 = size_of::<[u32; 4]>() as u64;

pub(super) struct Pipeline {
    device: wgpu::Device,
    queue: wgpu::Queue,
    render: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    backdrop: Option<BackdropTexture>,
}

struct BackdropTexture {
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    revision: Option<u64>,
}

impl Pipeline {
    pub(super) fn new(state: &egui_wgpu::RenderState, shader: &str) -> Result<Self, String> {
        let source = format!("{PRELUDE}\n{shader}");
        validate_shader(&source)?;
        let device = &state.device;
        with_error_scopes(device, || {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("material shader"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let uniform_entry = |binding, size| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(size),
                },
                count: None,
            };
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("material binds"),
                entries: &[
                    uniform_entry(0, FRAME_SIZE),
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    uniform_entry(3, HOST_SIZE),
                ],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("material pipeline layout"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
            let constants = [(
                "daozai_srgb_target".to_owned(),
                f64::from(u8::from(state.target_format.is_srgb())),
            )]
            .into_iter()
            .collect();
            let render = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("material pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("daozai_vs"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("daozai_fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: state.target_format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &constants,
                        ..Default::default()
                    },
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            });
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("material uniforms"),
                size: size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("material backdrop sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            Self {
                device: device.clone(),
                queue: state.queue.clone(),
                render,
                layout,
                uniform,
                sampler,
                backdrop: None,
            }
        })
    }

    pub(super) fn clear_backdrop(&mut self) {
        self.backdrop = None;
    }

    pub(super) fn callback(
        &mut self,
        rect: egui::Rect,
        mut frame: Frame,
        plate: &CleanPlate,
        dock: Dock,
    ) -> Result<egui::Shape, String> {
        let screen = plate.geometry.screen;
        let width = u32::try_from(screen.width).map_err(|_| "底片宽度无效")?;
        let height = u32::try_from(screen.height).map_err(|_| "底片高度无效")?;
        let limit = self.device.limits().max_texture_dimension_2d;
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|area| area.checked_mul(4));
        if width == 0
            || height == 0
            || width > limit
            || height > limit
            || bytes != Some(plate.bgra.len())
        {
            return Err("底片尺寸或数据长度无效".into());
        }
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        if self
            .backdrop
            .as_ref()
            .is_none_or(|entry| entry.texture.size() != size)
        {
            self.backdrop = Some(with_error_scopes(&self.device, || {
                self.make_backdrop(size)
            })?);
        }
        let backdrop = self.backdrop.as_mut().unwrap();
        let [w, h] = [width as f32, height as f32];
        frame.tex = [w, h, w, h];
        let uniform = Uniforms {
            frame,
            dock: [dock_code(dock), 0, 0, 0],
        };
        with_error_scopes(&self.device, || {
            if backdrop.revision != Some(plate.revision) {
                self.queue.write_texture(
                    backdrop.texture.as_image_copy(),
                    &plate.bgra,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * 4),
                        rows_per_image: Some(height),
                    },
                    size,
                );
            }
            self.queue
                .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        })?;
        backdrop.revision = Some(plate.revision);
        Ok(egui::Shape::Callback(Callback::new_paint_callback(
            rect,
            MaterialCallback {
                render: self.render.clone(),
                bind: backdrop.bind.clone(),
            },
        )))
    }

    fn make_backdrop(&self, size: wgpu::Extent3d) -> BackdropTexture {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("material backdrop"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let buffer_entry = |binding, offset, size| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &self.uniform,
                offset,
                size: wgpu::BufferSize::new(size),
            }),
        };
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material bind group"),
            layout: &self.layout,
            entries: &[
                buffer_entry(0, 0, FRAME_SIZE),
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                buffer_entry(3, FRAME_SIZE, HOST_SIZE),
            ],
        });
        BackdropTexture {
            texture,
            bind,
            revision: None,
        }
    }
}

fn dock_code(dock: Dock) -> u32 {
    match dock {
        Dock::Top => 0,
        Dock::Left => 1,
        Dock::Right => 2,
    }
}

fn validate_shader(source: &str) -> Result<wgpu::naga::Module, String> {
    use wgpu::naga::{front::wgsl, valid};

    let module = wgsl::parse_str(source).map_err(|error| error.emit_to_string(source))?;
    valid::Validator::new(valid::ValidationFlags::all(), valid::Capabilities::empty())
        .validate(&module)
        .map_err(|error| error.emit_to_string(source))?;
    Ok(module)
}

fn with_error_scopes<T>(device: &wgpu::Device, op: impl FnOnce() -> T) -> Result<T, String> {
    use wgpu::ErrorFilter;

    let filters = [
        ErrorFilter::Validation,
        ErrorFilter::Internal,
        ErrorFilter::OutOfMemory,
    ];
    for filter in filters {
        device.push_error_scope(filter);
    }
    let value = op();
    let mut error = None;
    for _ in filters {
        let caught = pollster::block_on(device.pop_error_scope());
        error = error.or(caught);
    }
    match error {
        Some(caught) => Err(caught.to_string()),
        None => Ok(value),
    }
}

struct MaterialCallback {
    render: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
}

impl CallbackTrait for MaterialCallback {
    fn paint(
        &self,
        _: PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _: &egui_wgpu::CallbackResources,
    ) {
        pass.set_pipeline(&self.render);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of};

    #[test]
    fn debug_shader_validates_and_matches_rust_frame_layout() {
        let module = validate_shader(&format!("{PRELUDE}\n{DEBUG_SHADER}")).unwrap();
        let frame_type = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("Frame"))
            .unwrap()
            .1;
        let wgpu::naga::TypeInner::Struct { members, span } = &frame_type.inner else {
            panic!("Frame 必须是 WGSL 结构体");
        };
        let rust = [
            ("capture", offset_of!(Frame, capture)),
            ("body", offset_of!(Frame, body)),
            ("shape", offset_of!(Frame, shape)),
            ("notch", offset_of!(Frame, notch)),
            ("env", offset_of!(Frame, env)),
            ("tint", offset_of!(Frame, tint)),
            ("tex", offset_of!(Frame, tex)),
            ("count", offset_of!(Frame, count)),
            ("ctl_rect", offset_of!(Frame, ctl_rect)),
            ("ctl_shape", offset_of!(Frame, ctl_shape)),
            ("ctl_spring", offset_of!(Frame, ctl_spring)),
            ("params", offset_of!(Frame, params)),
        ];
        assert_eq!(align_of::<Frame>(), 16);
        assert_eq!(size_of::<Frame>(), 1024);
        assert_eq!(*span as usize, size_of::<Frame>());
        assert_eq!(offset_of!(Uniforms, dock) as u64, FRAME_SIZE);
        assert_eq!(members.len(), rust.len());
        for (member, (name, offset)) in members.iter().zip(rust) {
            assert_eq!(member.name.as_deref(), Some(name));
            assert_eq!(member.offset as usize, offset, "{name}");
            assert_eq!(offset % 16, 0, "{name}");
        }
    }

    #[test]
    fn malformed_material_returns_an_error() {
        for invalid in [
            "fn material(i : MaterialIn) -> vec4<f32> { return ; }",
            "fn material(i : MaterialIn) -> vec3<f32> { return vec3<f32>(0.0); }",
        ] {
            assert!(validate_shader(&format!("{PRELUDE}\n{invalid}")).is_err());
        }
    }

    #[test]
    #[ignore = "需要本机 Direct3D 12，显式运行离屏像素检查"]
    fn dx12_material_pixels_and_shader_error_scope() {
        const SIDE: u32 = 64;
        let mut config = egui_wgpu::WgpuConfiguration::default();
        let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup else {
            unreachable!();
        };
        setup.instance_descriptor.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(&setup.instance_descriptor);
        let state = pollster::block_on(egui_wgpu::RenderState::create(
            &config, &instance, None, None, 1, false,
        ))
        .unwrap();

        // WGSL 本身合法，实际建立管线时因布局不匹配被错误作用域收集。
        let bad_binding = "
            @group(1) @binding(0) var<uniform> other : vec4<f32>;
            fn material(i : MaterialIn) -> vec4<f32> { return other; }
        ";
        assert!(validate_shader(&format!("{PRELUDE}\n{bad_binding}")).is_ok());
        assert!(Pipeline::new(&state, bad_binding).is_err());
        let mut pipeline = Pipeline::new(&state, DEBUG_SHADER).unwrap();

        let rect = |x, y, w, h| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
        let capture = rect(0.0, 0.0, SIDE as f32, SIDE as f32);
        let pixels: Vec<u8> = (0..SIDE as u8)
            .flat_map(|y| (0..SIDE as u8).flat_map(move |x| [97, y, x, 255]))
            .collect();
        let size = wgpu::Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        };
        let target = state.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: state.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let staging = state.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (SIDE * SIDE * 4) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        for (dock, body, [x, y], [outside_x, outside_y]) in [
            (Dock::Top, rect(20.0, 0.0, 24.0, 44.0), [17, 4], [17, 18]),
            (Dock::Left, rect(0.0, 20.0, 44.0, 24.0), [4, 17], [18, 17]),
            (
                Dock::Right,
                rect(20.0, 20.0, 44.0, 24.0),
                [59, 17],
                [45, 17],
            ),
        ] {
            let geometry = super::super::Geometry::from_rects(body, capture, (0, 0), 1.0).unwrap();
            let plate = CleanPlate {
                geometry,
                bgra: pixels.clone(),
                brightness: 0.0,
                revision: 1,
            };
            let mut frame = Frame {
                capture: [0.0, 0.0, SIDE as f32, SIDE as f32],
                body: geometry.body,
                shape: [7.0, 0.0, 0.0, 0.0],
                notch: [10.0, 12.0, 1.0, 0.0],
                ..Default::default()
            };
            frame.count[0] = 1;
            frame.ctl_rect[0] = [29.0, 18.0, 12.0, 10.0];
            frame.ctl_shape[0][1] = 1.0;
            let shape = pipeline.callback(capture, frame, &plate, dock).unwrap();
            let mut encoder = state.device.create_command_encoder(&Default::default());
            // 让 egui 的实际 paint 回调负责设置 viewport、绑定和绘制。
            let paint = [egui::ClippedPrimitive {
                clip_rect: capture,
                primitive: match shape {
                    egui::Shape::Callback(callback) => egui::epaint::Primitive::Callback(callback),
                    _ => unreachable!(),
                },
            }];
            let screen = egui_wgpu::ScreenDescriptor {
                size_in_pixels: [SIDE, SIDE],
                pixels_per_point: 1.0,
            };
            let mut renderer = state.renderer.write();
            renderer.update_buffers(&state.device, &state.queue, &mut encoder, &paint, &screen);
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            renderer.render(&mut pass.forget_lifetime(), &paint, &screen);
            encoder.copy_texture_to_buffer(
                target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(SIDE * 4),
                        rows_per_image: Some(SIDE),
                    },
                },
                size,
            );
            state.queue.submit([encoder.finish()]);
            let (send, receive) = std::sync::mpsc::channel();
            let slice = staging.slice(..);
            slice.map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap();
            });
            let _ = state.device.poll(wgpu::Maintain::Wait);
            receive.recv().unwrap().unwrap();
            let mapped = slice.get_mapped_range();
            let pixel = |x, y| {
                let offset = 4 * (y * SIDE as usize + x);
                &mapped[offset..offset + 4]
            };
            assert_eq!(pixel(x, y), &[x as u8, y as u8, 97, 255], "{dock:?}");
            assert_eq!(pixel(outside_x, outside_y), &[0; 4], "{dock:?}");
            if dock == Dock::Top {
                assert!(pixel(29, 22)[1] > 180, "控件边框应清晰可见");
            }
            drop(mapped);
            staging.unmap();
        }
    }
}
