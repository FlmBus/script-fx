//! SPIKE (SCRIPTFX_GPU_SPIKE=1): GPU compositing of the panel layers with
//! patch-style WGSL shaders, translated to GLSL by naga. Not production code.

use naga::back::glsl;

/// Inputs every patch shader gets (WGSL side, `@group(0) @binding(0)`).
pub const INPUTS_WGSL: &str = "
struct Inputs { size: vec2<f32>, time: f32, level: f32 }
@group(0) @binding(0) var<uniform> u: Inputs;
";

/// A validated, translated fragment shader.
pub struct Translated {
    pub glsl: String,
    /// GLSL name of the `Inputs` uniform block.
    pub inputs_block: Option<String>,
    /// GLSL sampler names (combined texture + sampler), in declaration order.
    pub samplers: Vec<String>,
}

/// WGSL fragment shader (entry point `main`) → validated → GLSL 330 core.
pub fn translate(wgsl: &str) -> Result<Translated, String> {
    let src = format!("{INPUTS_WGSL}\n{wgsl}");
    let module = naga::front::wgsl::parse_str(&src).map_err(|e| e.emit_to_string(&src))?;
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .map_err(|e| e.emit_to_string(&src))?;
    let mut out = String::new();
    let options = glsl::Options { version: glsl::Version::Desktop(330), ..Default::default() };
    let pipeline = glsl::PipelineOptions { shader_stage: naga::ShaderStage::Fragment, entry_point: "main".into(), multiview: None };
    let reflection = glsl::Writer::new(&mut out, &module, &info, &options, &pipeline, naga::proc::BoundsCheckPolicies::default())
        .map_err(|e| e.to_string())?
        .write()
        .map_err(|e| e.to_string())?;
    let inputs_block = reflection.uniforms.values().next().cloned();
    let mut samplers: Vec<String> = reflection.texture_mapping.keys().cloned().collect();
    samplers.sort();
    Ok(Translated { glsl: out, inputs_block, samplers })
}


// ---------------------------------------------------------------- the patch-side shaders

/// Generated layer: animated plasma that pulses with the output level.
pub const BACKGROUND_WGSL: &str = "
@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let p = uv * vec2<f32>(u.size.x / u.size.y, 1.0) * 3.0;
    let t = u.time * 0.4;
    let v = sin(p.x + t) + sin(p.y * 1.3 - t) + sin((p.x + p.y) * 0.7 + t * 1.7);
    let glow = 0.25 + 0.75 * u.level;
    let c = vec3<f32>(0.10, 0.05, 0.20) + glow * vec3<f32>(0.25 + 0.2 * sin(v), 0.10, 0.35 + 0.2 * cos(v));
    return vec4<f32>(c, 1.0);
}";

/// Adjustment layer: sees everything below; chromatic split + vignette + scanlines, audio-reactive.
pub const ADJUST_WGSL: &str = "
@group(0) @binding(1) var below: texture_2d<f32>;
@group(0) @binding(2) var below_s: sampler;
@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let shift = (0.001 + 0.006 * u.level) * vec2<f32>(1.0, 0.0);
    let r = textureSample(below, below_s, uv + shift).r;
    let g = textureSample(below, below_s, uv).g;
    let b = textureSample(below, below_s, uv - shift).b;
    let d = distance(uv, vec2<f32>(0.5, 0.5));
    let vignette = smoothstep(0.85, 0.35, d);
    let scan = 0.94 + 0.06 * sin(uv.y * u.size.y * 3.14159);
    return vec4<f32>(vec3<f32>(r, g, b) * vignette * scan, 1.0);
}";

// ---------------------------------------------------------------- trusted GLSL (ours)

/// Full-screen triangle (or the quad of `u_rect` for blits). `u_flip` = -1 when
/// rendering into an offscreen texture, so every texture stores its top row at
/// t = 0, like uploaded images; patch shaders always see uv with (0,0) top-left.
const VERTEX_GLSL: &str = "#version 330 core
uniform vec4 u_rect;   // x, y, w, h in 0..1, top-left origin
uniform float u_flip;
smooth out vec2 _vs2fs_location0;
void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float((gl_VertexID >> 1) & 1));
    vec2 uv01 = u_rect.xy + corner * u_rect.zw;        // top-left origin
    _vs2fs_location0 = corner;
    vec2 ndc = vec2(uv01.x * 2.0 - 1.0, 1.0 - uv01.y * 2.0);
    gl_Position = vec4(ndc.x, ndc.y * u_flip, 0.0, 1.0);
}";

const BLIT_FRAG_GLSL: &str = "#version 330 core
uniform sampler2D u_tex;
uniform float u_alpha;
smooth in vec2 _vs2fs_location0;
layout(location = 0) out vec4 o;
void main() { o = texture(u_tex, _vs2fs_location0) * u_alpha; }";

/// Shader folder (the value of SCRIPTFX_GPU_SPIKE if it's a directory):
/// `bg.wgsl` (generated layer), `fx*.wgsl` (adjustment passes, in name order),
/// `config.txt` (`scale = 2`, `content_alpha = 0.6`). Reloaded on change.
fn shader_dir() -> Option<std::path::PathBuf> {
    let v = std::env::var_os("SCRIPTFX_GPU_SPIKE")?;
    let p = std::path::PathBuf::from(v);
    p.is_dir().then_some(p)
}

/// Newest mtime in the shader folder (change detection).
fn dir_stamp(dir: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::read_dir(dir).ok()?.flatten().filter_map(|e| e.metadata().ok()?.modified().ok()).max()
}

struct Config {
    scale: f32,
    content_alpha: f32,
}

fn read_config(dir: Option<&std::path::Path>) -> Config {
    let mut c = Config { scale: 1.0, content_alpha: 1.0 };
    if let Some(text) = dir.and_then(|d| std::fs::read_to_string(d.join("config.txt")).ok()) {
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let v: f32 = v.trim().parse().unwrap_or(1.0);
                match k.trim() {
                    "scale" => c.scale = v.clamp(0.25, 4.0),
                    "content_alpha" => c.content_alpha = v.clamp(0.0, 1.0),
                    _ => {}
                }
            }
        }
    }
    c
}

// ---------------------------------------------------------------- state

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui_glow::glow::{self, HasContext};

/// One content layer for this frame: where it goes (in panel units 0..1) and,
/// if it changed, its new premultiplied RGBA pixels.
pub struct LayerIn {
    pub id: u64,
    pub rect01: [f32; 4],
    pub upload: Option<(u32, u32, Vec<u8>)>,
}

#[derive(Default)]
pub struct Frame {
    pub layers: Vec<LayerIn>,
    pub time: f32,
    pub level: f32,
}

struct Program {
    prog: glow::Program,
    /// Uniform buffer for `Inputs` (patch shaders only).
    ubo: Option<glow::Buffer>,
}

type Fbo = (glow::Framebuffer, glow::Texture, i32, i32);

struct Gl {
    vao: glow::VertexArray,
    background: Program,
    /// Adjustment passes, applied in order.
    fx: Vec<Program>,
    blit: Program,
    fbos: [Option<Fbo>; 2],
    textures: HashMap<u64, glow::Texture>,
    queries: Option<[glow::Query; 2]>,
    frame_no: u64,
    config: Config,
    stamp: Option<std::time::SystemTime>,
    last_check: std::time::Instant,
    /// A shader in the folder failed to compile (the previous ones keep running).
    shader_error: Option<String>,
}

#[derive(Default)]
struct State {
    gl: Option<Gl>,
    frame: Frame,
    pub gpu_ms: Option<f32>,
    pub cpu_ms: f32,
    pub info: String,
    pub error: Option<String>,
}

/// Shared between the editor (fills frames) and the GL paint callback.
#[derive(Clone, Default)]
pub struct Spike(Arc<Mutex<State>>);

impl Spike {
    pub fn enabled() -> bool {
        std::env::var_os("SCRIPTFX_GPU_SPIKE").is_some()
    }

    /// Hands this frame's layers and inputs to the next paint callback.
    pub fn submit(&self, frame: Frame) {
        let mut s = self.0.lock().unwrap();
        // Keep pending uploads that the callback hasn't consumed yet.
        let mut frame = frame;
        for old in s.frame.layers.drain(..) {
            if let (Some(up), Some(new)) = (old.upload, frame.layers.iter_mut().find(|l| l.id == old.id)) {
                if new.upload.is_none() {
                    new.upload = Some(up);
                }
            }
        }
        s.frame = frame;
    }

    /// One line for the overlay.
    pub fn status(&self) -> String {
        let s = self.0.lock().unwrap();
        let (passes, scale, alpha) =
            s.gl.as_ref().map(|g| (g.fx.len(), g.config.scale, g.config.content_alpha)).unwrap_or((0, 1.0, 1.0));
        let shader_error = s.gl.as_ref().and_then(|g| g.shader_error.clone());
        match (&s.error, &shader_error) {
            (Some(e), _) => format!("GPU spike error: {e}"),
            (None, Some(e)) => format!("shader error (previous shaders kept): {}", e.lines().next().unwrap_or("")),
            (None, None) => format!(
                "GPU {} · CPU {:.2} ms · {passes} fx passes · scale {scale} · α {alpha} · {}",
                s.gpu_ms.map(|v| format!("{v:.2} ms")).unwrap_or("–".into()),
                s.cpu_ms,
                s.info,
            ),
        }
    }

    pub fn callback(&self, rect: egui::Rect) -> egui::PaintCallback {
        let me = self.clone();
        egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                let mut s = me.0.lock().unwrap();
                if s.error.is_some() {
                    return;
                }
                let t0 = std::time::Instant::now();
                if let Err(e) = unsafe { paint(&mut s, painter.gl(), &info) } {
                    s.error = Some(e);
                }
                s.cpu_ms = t0.elapsed().as_secs_f32() * 1000.0;
            })),
        }
    }
}

use nih_plug_egui::egui;

unsafe fn compile(gl: &glow::Context, frag: &str) -> Result<glow::Program, String> {
    unsafe {
        let prog = gl.create_program()?;
        let mut shaders = Vec::new();
        for (kind, src) in [(glow::VERTEX_SHADER, VERTEX_GLSL), (glow::FRAGMENT_SHADER, frag)] {
            let sh = gl.create_shader(kind)?;
            gl.shader_source(sh, src);
            gl.compile_shader(sh);
            if !gl.get_shader_compile_status(sh) {
                return Err(format!("compile: {}\n{src}", gl.get_shader_info_log(sh)));
            }
            gl.attach_shader(prog, sh);
            shaders.push(sh);
        }
        gl.link_program(prog);
        if !gl.get_program_link_status(prog) {
            return Err(format!("link: {}", gl.get_program_info_log(prog)));
        }
        for sh in shaders {
            gl.detach_shader(prog, sh);
            gl.delete_shader(sh);
        }
        Ok(prog)
    }
}

/// A patch shader: WGSL → naga → GLSL, then compiled; `Inputs` bound to UBO slot 0,
/// the first sampler to texture unit 0.
unsafe fn patch_program(gl: &glow::Context, wgsl: &str) -> Result<Program, String> {
    let t = translate(wgsl)?;
    unsafe {
        let prog = compile(gl, &t.glsl)?;
        let mut ubo = None;
        if let Some(block) = &t.inputs_block {
            if let Some(idx) = gl.get_uniform_block_index(prog, block) {
                gl.uniform_block_binding(prog, idx, 0);
                let b = gl.create_buffer()?;
                gl.bind_buffer(glow::UNIFORM_BUFFER, Some(b));
                gl.buffer_data_size(glow::UNIFORM_BUFFER, 16, glow::DYNAMIC_DRAW);
                ubo = Some(b);
            }
        }
        gl.use_program(Some(prog));
        if let Some(name) = t.samplers.first() {
            if let Some(loc) = gl.get_uniform_location(prog, name) {
                gl.uniform_1_i32(Some(&loc), 0);
            }
        }
        Ok(Program { prog, ubo })
    }
}

unsafe fn init(gl: &glow::Context) -> Result<(Gl, String), String> {
    unsafe {
        let info = format!(
            "{} · {}",
            gl.get_parameter_string(glow::VERSION),
            gl.get_parameter_string(glow::RENDERER)
        );
        let blit = compile(gl, BLIT_FRAG_GLSL)?;
        gl.use_program(Some(blit));
        if let Some(loc) = gl.get_uniform_location(blit, "u_tex") {
            gl.uniform_1_i32(Some(&loc), 0);
        }
        let queries = if gl.supported_extensions().contains("GL_ARB_timer_query") || gl.version().major >= 4 {
            Some([gl.create_query()?, gl.create_query()?])
        } else {
            None
        };
        let dir = shader_dir();
        let (background, fx) = load_shaders(gl, dir.as_deref())?;
        Ok((
            Gl {
                vao: gl.create_vertex_array()?,
                background,
                fx,
                blit: Program { prog: blit, ubo: None },
                fbos: [None, None],
                textures: HashMap::new(),
                queries,
                frame_no: 0,
                config: read_config(dir.as_deref()),
                stamp: dir.as_deref().and_then(dir_stamp),
                last_check: std::time::Instant::now(),
                shader_error: None,
            },
            info,
        ))
    }
}

/// Background + adjustment programs from the folder (or the built-in ones).
unsafe fn load_shaders(gl: &glow::Context, dir: Option<&std::path::Path>) -> Result<(Program, Vec<Program>), String> {
    unsafe {
        let Some(dir) = dir else {
            return Ok((patch_program(gl, BACKGROUND_WGSL)?, vec![patch_program(gl, ADJUST_WGSL)?]));
        };
        let read = |p: std::path::PathBuf| std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()));
        let bg_src = read(dir.join("bg.wgsl")).unwrap_or_else(|_| BACKGROUND_WGSL.to_string());
        let background = patch_program(gl, &bg_src).map_err(|e| format!("bg.wgsl: {e}"))?;
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("fx") && n.ends_with(".wgsl"))
            .collect();
        names.sort();
        let mut fx = Vec::new();
        for n in names {
            fx.push(patch_program(gl, &read(dir.join(&n))?).map_err(|e| format!("{n}: {e}"))?);
        }
        Ok((background, fx))
    }
}

unsafe fn delete_program(gl: &glow::Context, p: Program) {
    unsafe {
        gl.delete_program(p.prog);
        if let Some(b) = p.ubo {
            gl.delete_buffer(b);
        }
    }
}

unsafe fn make_fbo(gl: &glow::Context, w: i32, h: i32) -> Result<Fbo, String> {
    unsafe {
        let tex = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(tex));
        gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w, h, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None));
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        let fb = gl.create_framebuffer()?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
        gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
        if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
            return Err("offscreen framebuffer incomplete".into());
        }
        Ok((fb, tex, w, h))
    }
}

/// `alpha` only matters for the blit program (content layer opacity).
unsafe fn draw(gl: &glow::Context, p: &Program, rect01: [f32; 4], flip: f32, inputs: Option<[f32; 4]>, alpha: f32) {
    unsafe {
        gl.use_program(Some(p.prog));
        if let Some(loc) = gl.get_uniform_location(p.prog, "u_rect") {
            gl.uniform_4_f32(Some(&loc), rect01[0], rect01[1], rect01[2], rect01[3]);
        }
        if let Some(loc) = gl.get_uniform_location(p.prog, "u_flip") {
            gl.uniform_1_f32(Some(&loc), flip);
        }
        if let Some(loc) = gl.get_uniform_location(p.prog, "u_alpha") {
            gl.uniform_1_f32(Some(&loc), alpha);
        }
        if let (Some(ubo), Some(v)) = (p.ubo, inputs) {
            gl.bind_buffer(glow::UNIFORM_BUFFER, Some(ubo));
            let bytes: Vec<u8> = v.iter().flat_map(|f| f.to_ne_bytes()).collect();
            gl.buffer_sub_data_u8_slice(glow::UNIFORM_BUFFER, 0, &bytes);
            gl.bind_buffer_base(glow::UNIFORM_BUFFER, 0, Some(ubo));
        }
        gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
    }
}

unsafe fn paint(s: &mut State, gl: &glow::Context, info: &egui::PaintCallbackInfo) -> Result<(), String> {
    unsafe {
        if s.gl.is_none() {
            let (g, i) = init(gl)?;
            s.info = i;
            s.gl = Some(g);
        }
        let frame = std::mem::take(&mut s.frame.layers);
        let (time, level) = (s.frame.time, s.frame.level);
        let mut gpu_ms = s.gpu_ms;
        let g = s.gl.as_mut().unwrap();
        let vp = info.viewport_in_pixels();
        let (w, h) = (vp.width_px, vp.height_px);
        let prev_fb = gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING);

        // Timer: read the query from two frames ago, start this frame's.
        if let Some(q) = g.queries {
            let q_now = q[(g.frame_no % 2) as usize];
            let q_old = q[((g.frame_no + 1) % 2) as usize];
            if g.frame_no >= 1 && gl.get_query_parameter_u32(q_old, glow::QUERY_RESULT_AVAILABLE) != 0 {
                let now = gl.get_query_parameter_u32(q_old, glow::QUERY_RESULT) as f32 / 1e6;
                gpu_ms = Some(gpu_ms.map_or(now, |prev| prev + 0.1 * (now - prev)));
            }
            gl.begin_query(glow::TIME_ELAPSED, q_now);
        }

        // Hot reload of the shader folder (checked a few times per second).
        if g.last_check.elapsed().as_millis() > 300 {
            g.last_check = std::time::Instant::now();
            if let Some(dir) = shader_dir() {
                let stamp = dir_stamp(&dir);
                if stamp != g.stamp {
                    g.stamp = stamp;
                    g.config = read_config(Some(&dir));
                    match load_shaders(gl, Some(&dir)) {
                        Ok((bg, fx)) => {
                            delete_program(gl, std::mem::replace(&mut g.background, bg));
                            for p in std::mem::replace(&mut g.fx, fx) {
                                delete_program(gl, p);
                            }
                            g.shader_error = None;
                        }
                        Err(e) => g.shader_error = Some(e),
                    }
                }
            }
        }

        // Offscreen targets at the panel's pixel size × render scale.
        let (ow, oh) = (((w as f32) * g.config.scale) as i32, ((h as f32) * g.config.scale) as i32);
        for slot in &mut g.fbos {
            if slot.map(|f| (f.2, f.3)) != Some((ow, oh)) {
                if let Some((fb, tex, _, _)) = slot.take() {
                    gl.delete_framebuffer(fb);
                    gl.delete_texture(tex);
                }
                *slot = Some(make_fbo(gl, ow, oh)?);
            }
        }
        let fbos = [g.fbos[0].unwrap(), g.fbos[1].unwrap()];

        // Upload changed content layers.
        for l in &frame {
            if let Some((lw, lh, px)) = &l.upload {
                let tex = match g.textures.get(&l.id) {
                    Some(t) => *t,
                    None => {
                        let t = gl.create_texture()?;
                        g.textures.insert(l.id, t);
                        t
                    }
                };
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, *lw as i32, *lh as i32, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(px)));
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            }
        }
        let alive: std::collections::HashSet<u64> = frame.iter().map(|l| l.id).collect();
        g.textures.retain(|id, t| {
            let keep = alive.contains(id);
            if !keep {
                gl.delete_texture(*t);
            }
            keep
        });

        gl.bind_vertex_array(Some(g.vao));
        gl.disable(glow::SCISSOR_TEST);
        gl.active_texture(glow::TEXTURE0);
        let inputs = [ow as f32, oh as f32, time, level];
        let screen = std::num::NonZeroU32::new(prev_fb as u32).map(glow::NativeFramebuffer);

        // Pass 1: generated layer (background) into buffer 0.
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbos[0].0));
        gl.viewport(0, 0, ow, oh);
        gl.disable(glow::BLEND);
        draw(gl, &g.background, [0.0, 0.0, 1.0, 1.0], -1.0, Some(inputs), 1.0);

        // Pass 2: content layers on top (premultiplied alpha, scaled by content_alpha).
        gl.enable(glow::BLEND);
        gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
        for l in &frame {
            if let Some(tex) = g.textures.get(&l.id) {
                gl.bind_texture(glow::TEXTURE_2D, Some(*tex));
                draw(gl, &g.blit, l.rect01, -1.0, None, g.config.content_alpha);
            }
        }
        gl.disable(glow::BLEND);

        // Adjustment passes: ping-pong between the buffers; the last one goes to the screen.
        let n = g.fx.len();
        let mut src = 0;
        for (i, p) in g.fx.iter().enumerate() {
            gl.bind_texture(glow::TEXTURE_2D, Some(fbos[src].1));
            if i + 1 == n {
                gl.bind_framebuffer(glow::FRAMEBUFFER, screen);
                gl.viewport(vp.left_px, vp.from_bottom_px, w, h);
                draw(gl, p, [0.0, 0.0, 1.0, 1.0], 1.0, Some(inputs), 1.0);
            } else {
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbos[1 - src].0));
                gl.viewport(0, 0, ow, oh);
                draw(gl, p, [0.0, 0.0, 1.0, 1.0], -1.0, Some(inputs), 1.0);
                src = 1 - src;
            }
        }
        if n == 0 {
            gl.bind_texture(glow::TEXTURE_2D, Some(fbos[0].1));
            gl.bind_framebuffer(glow::FRAMEBUFFER, screen);
            gl.viewport(vp.left_px, vp.from_bottom_px, w, h);
            draw(gl, &g.blit, [0.0, 0.0, 1.0, 1.0], 1.0, None, 1.0);
        }

        if g.queries.is_some() {
            gl.end_query(glow::TIME_ELAPSED);
        }
        g.frame_no += 1;
        gl.bind_vertex_array(None);
        s.gpu_ms = gpu_ms;
        // Keep the (now uploaded) layer list without pixel data for the next frame.
        s.frame.layers = frame.into_iter().map(|l| LayerIn { upload: None, ..l }).collect();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_a_patch_shader() {
        let t = translate(
            "@group(0) @binding(1) var below: texture_2d<f32>;
             @group(0) @binding(2) var below_s: sampler;
             @fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
                 let c = textureSample(below, below_s, uv);
                 return vec4<f32>(c.rgb * (0.5 + 0.5 * sin(u.time)), c.a);
             }",
        )
        .unwrap();
        println!("{}\nblock {:?} samplers {:?}", t.glsl, t.inputs_block, t.samplers);
        assert!(t.inputs_block.is_some() && t.samplers.len() == 1);
    }

    #[test]
    fn rejects_invalid_shaders() {
        assert!(translate("@fragment fn main() -> @location(0) vec4<f32> { return nope; }").is_err());
    }
}

#[cfg(test)]
mod folder_check {
    /// `SPIKE_CHECK_DIR=<dir> cargo test -p scriptfx folder_check -- --ignored`
    #[test]
    #[ignore]
    fn translate_folder() {
        let dir = std::env::var("SPIKE_CHECK_DIR").unwrap();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "wgsl") {
                match super::translate(&std::fs::read_to_string(&p).unwrap()) {
                    Ok(_) => println!("ok   {}", p.display()),
                    Err(err) => println!("FAIL {}\n{err}", p.display()),
                }
            }
        }
    }
}
