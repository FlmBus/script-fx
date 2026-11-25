//! PATCH(scriptfx): a full-frame post-process pass: a radial (zoom) blur and/or a soft (disc)
//! blur inside a rectangle.
//!
//! The app sets the effect for the next frames (`set`); while one is set, `Renderer::present`
//! renders the frame into an offscreen texture and draws it to the surface through the blur.
//! Without an effect nothing changes (no offscreen pass, no extra cost).
//!
//! Coordinates are logical (the app's); the pass scales them by the viewport's scale factor.
//!
//! **Backdrop:** an app can mark part of its UI as the backdrop ([`mark_backdrop_start`] right
//! before `with_layer`, [`mark_backdrop_end`] right before that layer ends). Then only the layers
//! up to the backdrop's end go through the blur; the layers drawn after it are drawn on top,
//! sharp, whatever their shape (no `keep` rectangles needed). Without a backdrop the whole frame
//! goes through the blur (with `keep`).

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

/// A pending backdrop mark, consumed by the renderer's next `start_layer` / `end_layer`.
static MARK: AtomicU8 = AtomicU8::new(MARK_NONE);
const MARK_NONE: u8 = 0;
const MARK_START: u8 = 1;
const MARK_END: u8 = 2;

/// The next layer started is the backdrop's (call right before `Renderer::with_layer`).
pub fn mark_backdrop_start() {
    MARK.store(MARK_START, Ordering::Relaxed);
}

/// The next layer ended closes the backdrop (call at the end of the `with_layer` closure).
pub fn mark_backdrop_end() {
    MARK.store(MARK_END, Ordering::Relaxed);
}

pub(crate) fn take_mark_start() -> bool {
    MARK.compare_exchange(MARK_START, MARK_NONE, Ordering::Relaxed, Ordering::Relaxed).is_ok()
}

pub(crate) fn take_mark_end() -> bool {
    MARK.compare_exchange(MARK_END, MARK_NONE, Ordering::Relaxed, Ordering::Relaxed).is_ok()
}

/// A zoom blur towards `center` plus a soft blur of radius `soft`, applied inside `rect` (pixels
/// outside it pass through; samples stay inside, so nothing from outside bleeds in). `keep`:
/// areas inside `rect` that stay sharp (UI drawn over the blurred content; zero-sized = unused).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadialBlur {
    pub center: [f32; 2],
    /// x, y, width, height.
    pub rect: [f32; 4],
    /// How far each pixel smears towards the centre, as a fraction of its distance (0 = none).
    pub amount: f32,
    /// Radius of the soft blur in logical pixels (0 = none): the samples spread over a disc.
    pub soft: f32,
    /// x, y, width, height each.
    pub keep: [[f32; 4]; KEEP],
}

/// How many areas can stay sharp.
pub const KEEP: usize = 8;

static EFFECT: Mutex<Option<RadialBlur>> = Mutex::new(None);

/// The effect for the coming frames (`None` = off).
pub fn set(effect: Option<RadialBlur>) {
    *EFFECT.lock().unwrap_or_else(|e| e.into_inner()) = effect.filter(|e| e.amount > 0.0 || e.soft > 0.0);
}

pub(crate) fn current() -> Option<RadialBlur> {
    *EFFECT.lock().unwrap_or_else(|e| e.into_inner())
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    center: [f32; 2],
    rect_min: [f32; 2],
    rect_max: [f32; 2],
    size: [f32; 2],
    keep: [[f32; 4]; KEEP],
    amount: f32,
    soft: f32,
    _pad: [f32; 2],
}

const SHADER: &str = r#"
// keep: min.xy, max.xy of an area that stays sharp
struct U { center: vec2f, rect_min: vec2f, rect_max: vec2f, size: vec2f, keep: array<vec4f, 8>, amount: f32, soft: f32 };
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: U;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    // one triangle covering the screen
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

const N: i32 = 24;

@fragment
fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let px = pos.xy;
    let inside = all(px >= u.rect_min) && all(px <= u.rect_max);
    var kept = false;
    for (var k = 0; k < 8; k = k + 1) {
        kept = kept || (all(px >= u.keep[k].xy) && all(px < u.keep[k].zw));
    }
    if (!inside || kept) {
        return textureSampleLevel(frame, samp, px / u.size, 0.0);
    }
    let to_center = u.center - px;
    var acc = vec4f(0.0);
    for (var i = 0; i < N; i = i + 1) {
        let f = f32(i) / f32(N - 1) * u.amount;
        // soft: the samples on a golden-angle spiral filling a disc of radius `soft`
        let a = f32(i) * 2.39996;
        let disc = vec2f(cos(a), sin(a)) * sqrt((f32(i) + 0.5) / f32(N)) * u.soft;
        let q = clamp(px + to_center * f + disc, u.rect_min + 0.5, u.rect_max - 0.5);
        acc = acc + textureSampleLevel(frame, samp, q / u.size, 0.0);
    }
    return acc / f32(N);
}
"#;

/// GPU resources of the pass, kept by the renderer (made on first use, the offscreen texture
/// remade when the frame size or format changes).
pub(crate) struct PostFx {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    format: wgpu::TextureFormat,
    target: Option<(wgpu::Extent3d, wgpu::TextureView, wgpu::BindGroup)>,
}

impl PostFx {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scriptfx postfx"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scriptfx postfx"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scriptfx postfx"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scriptfx postfx"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scriptfx postfx"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scriptfx postfx"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        PostFx { pipeline, layout, sampler, uniforms, format, target: None }
    }

    pub(crate) fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// The offscreen texture to render the frame into (the size of the frame).
    pub(crate) fn target(&mut self, device: &wgpu::Device, width: u32, height: u32) -> &wgpu::TextureView {
        let size = wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 };
        if self.target.as_ref().is_none_or(|(s, ..)| *s != size) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scriptfx postfx frame"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scriptfx postfx"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.uniforms.as_entire_binding() },
                ],
            });
            self.target = Some((size, view, bind));
        }
        &self.target.as_ref().expect("just made").1
    }

    /// Draws the offscreen frame to `frame` through the blur (`scale`: logical → physical).
    pub(crate) fn apply(&self, encoder: &mut wgpu::CommandEncoder, queue: &wgpu::Queue, frame: &wgpu::TextureView, effect: RadialBlur, scale: f32) {
        let Some((size, _, bind)) = &self.target else { return };
        let [x, y, w, h] = effect.rect.map(|v| v * scale);
        let keep = |k: [f32; 4]| {
            let [x, y, w, h] = k.map(|v| v * scale);
            [x, y, x + w, y + h]
        };
        let u = Uniforms {
            center: effect.center.map(|v| v * scale),
            rect_min: [x, y],
            rect_max: [x + w, y + h],
            size: [size.width as f32, size.height as f32],
            keep: effect.keep.map(keep),
            amount: effect.amount,
            soft: effect.soft * scale,
            _pad: [0.0; 2],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scriptfx postfx"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: frame,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
}
