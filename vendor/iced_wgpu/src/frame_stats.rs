//! PATCH(scriptfx): per-frame timing for the whole iced frame, plus the counters, phase marker
//! and GPU report the plugin's diagnostics log needs (`crates/plugin/src/diag.rs`).
//!
//! Timing of one frame:
//!
//! - `ui`: from the frame's start (`mark_frame_start`, called by the app when it starts building
//!   the frame) to the compositor asking for the swapchain image: the app's `view` plus iced's
//!   layout and draw of the widget tree.
//! - `acquire`: waiting for the swapchain image (`get_current_texture`). This is where a blocking
//!   present mode, a busy compositor or a stalled GPU shows up.
//! - `render`: CPU time of `Renderer::present` (prepare, encode, submit).
//! - `swap`: `frame.present()` — handing the finished image to the window system. A compositor
//!   that throttles us, or a driver that waits for the GPU here, shows up in this one.
//! - `gpu`: GPU time between two timestamps written before and after the frame's work. Needs
//!   `TIMESTAMP_QUERY` + `TIMESTAMP_QUERY_INSIDE_ENCODERS`, requested when the adapter has them.
//!   Read back a few frames late, without stalling.
//!
//! [`get`] returns smoothed values for a live readout; [`on_frame`] reports every frame raw, with
//! the image/atlas counters of that frame, for the log. [`phase`] says what the UI thread is doing
//! right now, so a watchdog can name the step a stalled frame is stuck in.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

static EPOCH: OnceLock<Instant> = OnceLock::new();
static FRAME_START: AtomicU64 = AtomicU64::new(0);
static UI_NS: AtomicU64 = AtomicU64::new(0);
static RENDER_NS: AtomicU64 = AtomicU64::new(0);
static GPU_NS: AtomicU64 = AtomicU64::new(u64::MAX);
static LAST_PRESENT: AtomicU64 = AtomicU64::new(0);
static INTERVAL_NS: AtomicU64 = AtomicU64::new(0);

// Raw per-frame values (not smoothed), for [`on_frame`].
static FRAME_INDEX: AtomicU64 = AtomicU64::new(0);
static UI_DONE_AT: AtomicU64 = AtomicU64::new(0);
static UI_RAW_NS: AtomicU64 = AtomicU64::new(0);
static ACQUIRE_RAW_NS: AtomicU64 = AtomicU64::new(0);
static INTERVAL_RAW_NS: AtomicU64 = AtomicU64::new(0);
static RENDER_RAW_NS: AtomicU64 = AtomicU64::new(0);
/// When `end_present` finished, so `swap_done` can time `frame.present()`.
static PRESENT_DONE_AT: AtomicU64 = AtomicU64::new(0);
/// The last GPU timing read back, unsmoothed (the smoothed one is for the live readout).
static GPU_RAW_NS: AtomicU64 = AtomicU64::new(u64::MAX);

// Counted per frame (reset when the frame is reported).
static IMAGE_UPLOADS: AtomicU64 = AtomicU64::new(0);
static IMAGE_BYTES: AtomicU64 = AtomicU64::new(0);
static ATLAS_GROWS: AtomicU64 = AtomicU64::new(0);
// Gauges: the atlas as it stands.
static ATLAS_LAYERS: AtomicU64 = AtomicU64::new(0);
static ATLAS_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
// Cumulative: wgpu's uncaptured errors (the device's error scope), see `window::compositor`.
static WGPU_ERRORS: AtomicU64 = AtomicU64::new(0);

static HOOK: OnceLock<fn(Frame)> = OnceLock::new();
static REPORT: OnceLock<String> = OnceLock::new();

fn now_ns() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// Call at the start of a frame (when the app starts building it).
pub fn mark_frame_start() {
    FRAME_START.store(now_ns(), Ordering::Relaxed);
}

/// What the UI thread is doing, so a stalled frame can be named. The renderer sets the phases
/// from [`ICED_DRAW`](phase::ICED_DRAW) on; everything before that is the app's (the plugin's
/// shell marks its own steps).
pub mod phase {
    use super::{AtomicU64, AtomicU8, Ordering, now_ns};

    pub const IDLE: u8 = 0;
    pub const UPDATE: u8 = 1;
    pub const VIEW: u8 = 2;
    pub const PATCH_RASTER: u8 = 3;
    pub const ICON_RASTER: u8 = 4;
    pub const LUA: u8 = 5;
    pub const ICED_DRAW: u8 = 6;
    pub const ACQUIRE: u8 = 7;
    pub const PRESENT: u8 = 8;
    pub const SWAP: u8 = 9;

    pub const NAMES: [&str; 10] = [
        "idle",
        "app update",
        "app view",
        "patch raster",
        "icon raster",
        "lua",
        "iced layout+draw",
        "acquire swapchain image",
        "encode+submit",
        "swap buffers",
    ];

    static PHASE: AtomicU8 = AtomicU8::new(IDLE);
    static SINCE: AtomicU64 = AtomicU64::new(0);

    /// Enters `p`; returns the phase it replaced (pass it back to `enter` to restore).
    pub fn enter(p: u8) -> u8 {
        SINCE.store(now_ns(), Ordering::Relaxed);
        PHASE.swap(p, Ordering::Relaxed)
    }

    /// The current phase and how long it has been running.
    pub fn current() -> (u8, f32) {
        let p = PHASE.load(Ordering::Relaxed);
        let since = SINCE.load(Ordering::Relaxed);
        (p, now_ns().saturating_sub(since) as f32 / 1.0e6)
    }

    /// The name of a phase id.
    pub fn name(p: u8) -> &'static str {
        NAMES.get(p as usize).copied().unwrap_or("?")
    }
}

/// One finished frame, as [`on_frame`] reports it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Frame {
    /// Frames presented since the renderer was made (this one included).
    pub index: u64,
    /// Since the previous frame was presented: the real frame time.
    pub interval_ms: f32,
    /// The app's `view` plus iced's layout and draw (0 without `mark_frame_start`).
    pub ui_ms: f32,
    /// Waiting for the swapchain image.
    pub acquire_ms: f32,
    /// `Renderer::present`: prepare, encode, submit.
    pub render_ms: f32,
    /// `frame.present()`: handing the image to the window system.
    pub swap_ms: f32,
    /// The GPU's own time, a few frames late; `None` without timestamp queries.
    pub gpu_ms: Option<f32>,
    /// Images uploaded into the texture atlas this frame, and their size.
    pub image_uploads: u32,
    pub image_bytes: u64,
    /// Times the atlas was grown this frame (every grow copies the whole atlas).
    pub atlas_grows: u32,
    /// The atlas as it stands: array layers and live allocations.
    pub atlas_layers: u32,
    pub atlas_allocations: u32,
    /// Was a post-process effect (zoom/soft blur) on?
    pub postfx: bool,
    /// Uncaptured wgpu errors so far (not per frame).
    pub wgpu_errors: u64,
}

/// Reports every presented frame, on the thread that renders it. Set once, before the editor
/// opens; the renderer calls it at the end of each frame.
pub fn on_frame(hook: fn(Frame)) {
    let _ = HOOK.set(hook);
}

/// The GPU as the compositor found it: adapter, backend, driver, surface format, present mode,
/// features, and the adapters it could have picked. `None` until the editor has opened once.
pub fn gpu_report() -> Option<&'static str> {
    REPORT.get().map(String::as_str)
}

pub(crate) fn set_gpu_report(report: String) {
    let _ = REPORT.set(report);
}

/// An image was uploaded into the atlas (`width × height`, 4 bytes per pixel).
pub(crate) fn image_uploaded(width: u32, height: u32) {
    let _ = IMAGE_UPLOADS.fetch_add(1, Ordering::Relaxed);
    let _ = IMAGE_BYTES.fetch_add(u64::from(width) * u64::from(height) * 4, Ordering::Relaxed);
}

/// The atlas grew to `layers` array layers (it copies itself when it does).
pub(crate) fn atlas_grew(layers: usize) {
    let _ = ATLAS_GROWS.fetch_add(1, Ordering::Relaxed);
    ATLAS_LAYERS.store(layers as u64, Ordering::Relaxed);
}

/// The atlas as it stands, after an upload or a removal.
pub(crate) fn atlas_state(layers: usize, allocations: usize) {
    ATLAS_LAYERS.store(layers as u64, Ordering::Relaxed);
    ATLAS_ALLOCATIONS.store(allocations as u64, Ordering::Relaxed);
}

/// An uncaptured wgpu error (validation, out of memory, device lost). Counted so a frame line can
/// say "errors so far"; the message itself is logged where the handler is installed.
pub(crate) fn wgpu_error() {
    let _ = WGPU_ERRORS.fetch_add(1, Ordering::Relaxed);
}

/// Uncaptured wgpu errors since the editor opened.
pub fn wgpu_errors() -> u64 {
    WGPU_ERRORS.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub ui_ms: f32,
    pub render_ms: f32,
    /// `None` if the GPU can't do timestamp queries.
    pub gpu_ms: Option<f32>,
    /// Frames presented per second.
    pub fps: f32,
}

pub fn get() -> Stats {
    let ms = |ns: u64| ns as f32 / 1.0e6;
    let gpu = GPU_NS.load(Ordering::Relaxed);
    Stats {
        ui_ms: ms(UI_NS.load(Ordering::Relaxed)),
        render_ms: ms(RENDER_NS.load(Ordering::Relaxed)),
        gpu_ms: (gpu != u64::MAX).then(|| ms(gpu)),
        fps: match INTERVAL_NS.load(Ordering::Relaxed) {
            0 => 0.0,
            i => 1.0e9 / i as f32,
        },
    }
}

/// Before acquiring the swapchain image: the UI part of the frame is done.
pub(crate) fn ui_done() {
    let now = now_ns();
    UI_DONE_AT.store(now, Ordering::Relaxed);
    let start = FRAME_START.swap(0, Ordering::Relaxed);
    if start != 0 && now >= start {
        smooth(&UI_NS, now - start);
        UI_RAW_NS.store(now - start, Ordering::Relaxed);
    } else {
        UI_RAW_NS.store(0, Ordering::Relaxed);
    }
    let _ = phase::enter(phase::ACQUIRE);
}

pub(crate) fn begin_present() -> u64 {
    let now = now_ns();
    let last = LAST_PRESENT.swap(now, Ordering::Relaxed);
    if last != 0 {
        smooth(&INTERVAL_NS, now - last);
        INTERVAL_RAW_NS.store(now - last, Ordering::Relaxed);
    }
    // Between `ui_done` and here, the swapchain image was waited for.
    let ui_done = UI_DONE_AT.load(Ordering::Relaxed);
    ACQUIRE_RAW_NS.store(if ui_done != 0 { now.saturating_sub(ui_done) } else { 0 }, Ordering::Relaxed);
    let _ = phase::enter(phase::PRESENT);
    now
}

pub(crate) fn end_present(begin: u64) {
    let now = now_ns();
    let render_ns = now.saturating_sub(begin);
    smooth(&RENDER_NS, render_ns);
    RENDER_RAW_NS.store(render_ns, Ordering::Relaxed);
    PRESENT_DONE_AT.store(now, Ordering::Relaxed);
    let _ = phase::enter(phase::SWAP);
}

/// The image is with the window system: the frame is over, so report it (and go idle).
pub(crate) fn swap_done() {
    let swap_ns = now_ns().saturating_sub(PRESENT_DONE_AT.load(Ordering::Relaxed));
    let _ = phase::enter(phase::IDLE);

    let Some(hook) = HOOK.get() else { return };
    let ms = |ns: u64| ns as f32 / 1.0e6;
    let gpu = GPU_RAW_NS.load(Ordering::Relaxed);
    let frame = Frame {
        index: FRAME_INDEX.fetch_add(1, Ordering::Relaxed) + 1,
        interval_ms: ms(INTERVAL_RAW_NS.load(Ordering::Relaxed)),
        ui_ms: ms(UI_RAW_NS.load(Ordering::Relaxed)),
        acquire_ms: ms(ACQUIRE_RAW_NS.load(Ordering::Relaxed)),
        render_ms: ms(RENDER_RAW_NS.load(Ordering::Relaxed)),
        swap_ms: ms(swap_ns),
        gpu_ms: (gpu != u64::MAX).then(|| ms(gpu)),
        image_uploads: IMAGE_UPLOADS.swap(0, Ordering::Relaxed) as u32,
        image_bytes: IMAGE_BYTES.swap(0, Ordering::Relaxed),
        atlas_grows: ATLAS_GROWS.swap(0, Ordering::Relaxed) as u32,
        atlas_layers: ATLAS_LAYERS.load(Ordering::Relaxed) as u32,
        atlas_allocations: ATLAS_ALLOCATIONS.load(Ordering::Relaxed) as u32,
        postfx: crate::postfx::current().is_some(),
        wgpu_errors: WGPU_ERRORS.load(Ordering::Relaxed),
    };
    hook(frame);
}

/// Exponential moving average so the readout is legible.
fn smooth(slot: &AtomicU64, sample: u64) {
    let old = slot.load(Ordering::Relaxed);
    let new = if old == 0 || old == u64::MAX { sample } else { (old * 9 + sample) / 10 };
    slot.store(new, Ordering::Relaxed);
}

pub(crate) const FEATURES: wgpu::Features =
    wgpu::Features::TIMESTAMP_QUERY.union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);

struct Slot {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// `Some(done)` while a readback is in flight.
    pending: Option<Arc<AtomicBool>>,
}

pub(crate) struct GpuTimer {
    slots: Vec<Slot>,
    period_ns: f32,
    current: Option<usize>,
}

impl GpuTimer {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(FEATURES) {
            return None;
        }
        let slots = (0..4)
            .map(|_| Slot {
                queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("frame timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 2,
                }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: 16,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                readback: device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: 16,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                pending: None,
            })
            .collect();
        Some(Self { slots, period_ns: queue.get_timestamp_period(), current: None })
    }

    /// Collect finished readbacks, then write the start timestamp into a free slot (if any).
    pub(crate) fn begin(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) {
        let _ = device.poll(wgpu::PollType::Poll);
        for slot in &mut self.slots {
            if slot.pending.as_ref().is_some_and(|d| d.load(Ordering::Acquire)) {
                {
                    let data = slot.readback.slice(..).get_mapped_range();
                    let t0 = u64::from_le_bytes(data[0..8].try_into().unwrap());
                    let t1 = u64::from_le_bytes(data[8..16].try_into().unwrap());
                    if t1 > t0 {
                        let ns = ((t1 - t0) as f64 * self.period_ns as f64) as u64;
                        smooth(&GPU_NS, ns);
                        GPU_RAW_NS.store(ns, Ordering::Relaxed);
                    }
                }
                slot.readback.unmap();
                slot.pending = None;
            }
        }
        self.current = self.slots.iter().position(|s| s.pending.is_none());
        if let Some(i) = self.current {
            encoder.write_timestamp(&self.slots[i].queries, 0);
        }
    }

    pub(crate) fn end(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(i) = self.current {
            let s = &self.slots[i];
            encoder.write_timestamp(&s.queries, 1);
            encoder.resolve_query_set(&s.queries, 0..2, &s.resolve, 0);
            encoder.copy_buffer_to_buffer(&s.resolve, 0, &s.readback, 0, 16);
        }
    }

    /// After submit: start mapping the readback buffer.
    pub(crate) fn after_submit(&mut self) {
        if let Some(i) = self.current.take() {
            let done = Arc::new(AtomicBool::new(false));
            let flag = done.clone();
            self.slots[i].readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                if r.is_ok() {
                    flag.store(true, Ordering::Release);
                }
            });
            self.slots[i].pending = Some(done);
        }
    }
}
