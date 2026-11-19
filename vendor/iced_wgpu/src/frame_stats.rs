//! PATCH(scriptfx): per-frame timing for the whole iced frame.
//!
//! - `ui`: from the frame's start (`mark_frame_start`, called by the app when it sees the
//!   `RedrawRequested` event) to the compositor asking for the swapchain image: iced's
//!   update/draw of the widget tree. (Asking for the image is where vsync blocks.)
//! - `render`: CPU time of `Renderer::present` (prepare, encode, submit). Excludes waiting for
//!   the swapchain image (vsync), which happens before it.
//! - `gpu`: GPU time between two timestamps written before and after the frame's work. Needs
//!   `TIMESTAMP_QUERY` + `TIMESTAMP_QUERY_INSIDE_ENCODERS`, requested when the adapter has them.
//!   Read back a few frames late, without stalling.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

static EPOCH: OnceLock<Instant> = OnceLock::new();
static FRAME_START: AtomicU64 = AtomicU64::new(0);
static UI_NS: AtomicU64 = AtomicU64::new(0);
static RENDER_NS: AtomicU64 = AtomicU64::new(0);
static GPU_NS: AtomicU64 = AtomicU64::new(u64::MAX);
static LAST_PRESENT: AtomicU64 = AtomicU64::new(0);
static INTERVAL_NS: AtomicU64 = AtomicU64::new(0);

fn now_ns() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// Call at the start of a frame (the `RedrawRequested` event).
pub fn mark_frame_start() {
    FRAME_START.store(now_ns(), Ordering::Relaxed);
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
    let start = FRAME_START.swap(0, Ordering::Relaxed);
    if start != 0 && now >= start {
        smooth(&UI_NS, now - start);
    }
}

pub(crate) fn begin_present() -> u64 {
    let now = now_ns();
    let last = LAST_PRESENT.swap(now, Ordering::Relaxed);
    if last != 0 {
        smooth(&INTERVAL_NS, now - last);
    }
    now
}

pub(crate) fn end_present(begin: u64) {
    smooth(&RENDER_NS, now_ns().saturating_sub(begin));
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
                        smooth(&GPU_NS, ((t1 - t0) as f64 * self.period_ns as f64) as u64);
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
