use std::{
    future::Future,
    mem::size_of,
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

use eframe::{
    egui::{self, epaint::PaintCallbackInfo},
    egui_wgpu::{self, Callback, CallbackTrait},
    wgpu,
};

use super::CleanPlate;
use crate::model::Dock;

pub(super) const PRELUDE: &str = include_str!("prelude.wgsl");

/// 插件 API 1 的 uniform 布局（1040 字节，16 字节对齐）。修改字段时同步核对 `prelude.wgsl`。
#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
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
    pub motion: [f32; 4],
}

const FRAME_SIZE: u64 = size_of::<Frame>() as u64;

impl Frame {
    pub(super) fn uniform_bytes(&mut self) -> &[u8] {
        self.notch[3] = 0.0;
        self.count[2..].fill(0);
        // SAFETY: `Frame` is `#[repr(C, align(16))]` containing strictly POD `[f32; 4]` / `[u32; 4]` fields
        // with no padding bytes (`size_of::<Frame>() == 1040`).
        unsafe { std::slice::from_raw_parts((self as *const Self).cast::<u8>(), size_of::<Self>()) }
    }
}

struct UniformSlot {
    buffer: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

struct BackdropTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    slots: Vec<UniformSlot>,
    revision: Option<u64>,
}

pub(super) struct Pipeline {
    device: wgpu::Device,
    queue: wgpu::Queue,
    render: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    backdrop: Option<BackdropTexture>,
}

impl Pipeline {
    pub(super) fn new(
        state: &egui_wgpu::RenderState,
        shader: &str,
        msaa_samples: u32,
    ) -> Result<Self, String> {
        let source = format!("{PRELUDE}\n{shader}");
        let device = &state.device;
        with_error_scopes(device, || {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("material shader"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("material binds"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(FRAME_SIZE),
                        },
                        count: None,
                    },
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
                ],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("material pipeline layout"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
            let constants = std::collections::HashMap::from([(
                "daozai_srgb_target".to_string(),
                f64::from(u8::from(state.target_format.is_srgb())),
            )]);
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
                multisample: wgpu::MultisampleState {
                    count: msaa_samples.max(1),
                    ..Default::default()
                },
                multiview: None,
                cache: None,
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
        card_index: usize,
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
        let device = &self.device;
        let layout = &self.layout;
        let sampler = &self.sampler;
        let queue = &self.queue;
        let [w, h] = [width as f32, height as f32];
        frame.tex = [w, h, w, h];
        frame.count[1] = dock_code(dock);

        let bind_clone = with_error_scopes(device, || {
            let backdrop = self.backdrop.as_mut().unwrap();
            if backdrop.revision != Some(plate.revision) {
                queue.write_texture(
                    backdrop.texture.as_image_copy(),
                    &plate.bgra,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * 4),
                        rows_per_image: Some(height),
                    },
                    size,
                );
                backdrop.revision = Some(plate.revision);
            }
            while backdrop.slots.len() <= card_index {
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("material uniforms"),
                    size: FRAME_SIZE,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("material bind group"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&backdrop.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(sampler),
                        },
                    ],
                });
                backdrop.slots.push(UniformSlot { buffer, bind });
            }
            let slot = &backdrop.slots[card_index];
            queue.write_buffer(&slot.buffer, 0, frame.uniform_bytes());
            slot.bind.clone()
        })?;

        Ok(egui::Shape::Callback(Callback::new_paint_callback(
            rect,
            MaterialCallback {
                render: self.render.clone(),
                bind: bind_clone,
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
        BackdropTexture {
            texture,
            view,
            slots: Vec::new(),
            revision: None,
        }
    }
}

pub(super) fn dock_code(dock: Dock) -> u32 {
    match dock {
        Dock::Top => 0,
        Dock::Left => 1,
        Dock::Right => 2,
        Dock::Bottom => 3,
    }
}

struct ThreadWaker(std::thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => std::thread::park(),
        }
    }
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
        let caught = block_on(device.pop_error_scope());
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
    fn frame_layout_matches_wgsl_prelude_offsets() {
        assert_eq!(align_of::<Frame>(), 16);
        assert_eq!(offset_of!(Frame, capture), 0);
        assert_eq!(offset_of!(Frame, body), 16);
        assert_eq!(offset_of!(Frame, shape), 32);
        assert_eq!(offset_of!(Frame, notch), 48);
        assert_eq!(offset_of!(Frame, env), 64);
        assert_eq!(offset_of!(Frame, tint), 80);
        assert_eq!(offset_of!(Frame, tex), 96);
        assert_eq!(offset_of!(Frame, count), 112);
        assert_eq!(offset_of!(Frame, ctl_rect), 128);
        assert_eq!(offset_of!(Frame, ctl_shape), 384);
        assert_eq!(offset_of!(Frame, ctl_spring), 640);
        assert_eq!(offset_of!(Frame, params), 896);
        assert_eq!(offset_of!(Frame, motion), 1024);
        assert_eq!(size_of::<Frame>(), 1040);
        for off in [
            offset_of!(Frame, capture),
            offset_of!(Frame, body),
            offset_of!(Frame, shape),
            offset_of!(Frame, notch),
            offset_of!(Frame, env),
            offset_of!(Frame, tint),
            offset_of!(Frame, tex),
            offset_of!(Frame, count),
            offset_of!(Frame, ctl_rect),
            offset_of!(Frame, ctl_shape),
            offset_of!(Frame, ctl_spring),
            offset_of!(Frame, params),
            offset_of!(Frame, motion),
        ] {
            assert_eq!(off % 16, 0);
        }
    }

    #[test]
    fn cayww_wgsl_entry_and_prelude_combine_cleanly() {
        let shader = include_str!("cayww_liquid_glass.wgsl");
        assert!(PRELUDE.contains("fn daozai_fs("));
        assert!(shader.contains("fn material(i : MaterialIn) -> vec4<f32>"));
    }
}
