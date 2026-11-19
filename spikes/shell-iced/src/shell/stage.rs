//! The stage: the patch window. In the spike it shows a GPU "generated layer" (a WGSL shader
//! run through iced's `shader` widget, i.e. our own wgpu pipeline inside iced's frame) with
//! the patch title on top. This is where the ControlPanel compositor will render later.

use super::{mock, theme};
use nice_plug::prelude::AtomicF32;
use nice_plug_iced::iced::{
    self, Element, Fill, Rectangle, mouse, wgpu,
    widget::{center, column, container, shader, stack, text},
};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
use std::time::Instant;

/// Frame interval (ms, f32 bits), for the fps readout.
static FRAME_MS: AtomicU32 = AtomicU32::new(0);

pub fn view<'a, M: 'a>(patch: &'static mock::Patch, level: Arc<AtomicF32>) -> Element<'a, M> {
    let art = shader(Art { patch, level }).width(Fill).height(Fill);

    let title = if patch.title_upper { patch.name.to_uppercase() } else { patch.name.to_string() };
    // Frame cost, measured in the patched iced_wgpu (vendor/iced_wgpu/src/frame_stats.rs):
    // CPU = ui (iced update + widget draw) + render (prepare, encode, submit); GPU from timestamps.
    let interval = f32::from_bits(FRAME_MS.load(Ordering::Relaxed));
    let st = iced_wgpu::frame_stats::get();
    let gpu = st.gpu_ms.map_or("n/a".to_string(), |g| format!("{g:.2} ms"));
    let readout = container(
        text(format!(
            "cpu {:.2} ms (ui {:.2} + render {:.2}) · gpu {} · {:.0} fps",
            st.ui_ms + st.render_ms,
            st.ui_ms,
            st.render_ms,
            gpu,
            if interval > 0.0 { 1000.0 / interval } else { 0.0 },
        ))
        .size(11)
        .font(theme::font::MONO)
        .color(theme::alpha(patch.ink, 0.6)),
    )
    .padding(14)
    .align_bottom(Fill);

    stack![
        art,
        center(column![text(title).size(40).font(patch.title_font).color(patch.title_color)]),
        readout,
    ]
    .into()
}

struct Art {
    patch: &'static mock::Patch,
    level: Arc<AtomicF32>,
}

impl<M> shader::Program<M> for Art {
    type State = ();
    type Primitive = Layer;

    // Pacing finding: requesting the next frame here (`Action::request_redraw_at`) spins
    // baseview's frame loop at 100 % CPU. The editor uses `always_redraw` instead, which needs a
    // throttling present mode (fifo); iced only takes it from `ICED_PRESENT_MODE` so far.

    /// Marks the start of each frame for the timing readout.
    fn update(&self, _: &mut (), event: &iced::Event, _: Rectangle, _: mouse::Cursor) -> Option<shader::Action<M>> {
        if let iced::Event::Window(iced::window::Event::RedrawRequested(_)) = event {
            iced_wgpu::frame_stats::mark_frame_start();
        }
        None
    }

    fn draw(&self, _: &(), _: mouse::Cursor, _: Rectangle) -> Layer {
        let c = |c: nice_plug_iced::iced::Color| [c.r, c.g, c.b, c.a];
        Layer { level: self.level.clone(), color_a: c(self.patch.glow.0), color_b: c(self.patch.glow.1) }
    }
}

#[derive(Debug)]
pub struct Layer {
    /// Read in `prepare`, i.e. every rendered frame, without re-running `view`.
    level: Arc<AtomicF32>,
    color_a: [f32; 4],
    color_b: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Inputs {
    rect: [f32; 4],
    color_a: [f32; 4],
    color_b: [f32; 4],
    time: f32,
    level: f32,
    radius: f32,
    scale: f32,
}

pub struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    start: Instant,
    last_frame: Option<Instant>,
    frame_ms: f32,
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stage layer"),
            source: wgpu::ShaderSource::Wgsl(include_str!("stage.wgsl").into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stage inputs"),
            size: std::mem::size_of::<Inputs>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stage layer"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self { pipeline, uniforms, bind_group, start: Instant::now(), last_frame: None, frame_ms: 0.0 }
    }
}

impl shader::Primitive for Layer {
    type Pipeline = Pipeline;

    fn prepare(&self, p: &mut Pipeline, _device: &wgpu::Device, queue: &wgpu::Queue, bounds: &Rectangle, viewport: &shader::Viewport) {
        let now = Instant::now();
        if let Some(last) = p.last_frame {
            let dt = (now - last).as_secs_f32() * 1000.0;
            p.frame_ms = if p.frame_ms == 0.0 { dt } else { p.frame_ms * 0.95 + dt * 0.05 };
            FRAME_MS.store(p.frame_ms.to_bits(), Ordering::Relaxed);
        }
        p.last_frame = Some(now);

        let s = viewport.scale_factor();
        let inputs = Inputs {
            rect: [bounds.x * s, bounds.y * s, bounds.width * s, bounds.height * s],
            color_a: self.color_a,
            color_b: self.color_b,
            time: (now - p.start).as_secs_f32(),
            level: self.level.load(Ordering::Relaxed),
            radius: theme::RADIUS_WINDOW * s,
            scale: s,
        };
        queue.write_buffer(&p.uniforms, 0, bytemuck::bytes_of(&inputs));
    }

    fn draw(&self, p: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pass.set_pipeline(&p.pipeline);
        pass.set_bind_group(0, &p.bind_group, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}
