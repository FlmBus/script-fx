//! Diagnostics: what the plugin can say about itself on a machine we can't attach a debugger to
//! (a tester's Mac, the Windows side of a dual boot). Two layers:
//!
//! **1. The log file** (`--features diagnostics`, what CI ships):
//!
//! - **where:** `~/Library/Logs/ScriptFX/` (macOS), `%LOCALAPPDATA%\ScriptFX\logs\` (Windows),
//!   `~/.local/share/ScriptFX/logs/` (Linux); name `scriptfx-<unix time>-<pid>.log`;
//! - **what:** a system report at start (versions, OS, CPU, the host process, paths), everything
//!   `nice_log!` and the `log` crate say (iced, wgpu: GPU adapter, warnings, errors) with time and
//!   thread, panics with a backtrace (nice-plug's panic hook logs into the same file), and a
//!   **heartbeat every 10 s**: audio callback timing (calls, worst time against the buffer's
//!   real-time budget, late callbacks, non-finite output) and the editor's frame timing.
//! - **level:** `SCRIPTFX_LOG` (default `info`), a `tracing` filter, so a run can ask for more
//!   from one place: `SCRIPTFX_LOG=info,iced_wgpu::image=debug` logs every atlas allocation.
//!
//! **2. The render diagnostics** (always compiled, logged through `nice_log!`, so they also show
//! up on the standalone's stderr in a plain release build):
//!
//! - [`install`] hooks every presented frame ([`iced_wgpu::frame_stats`]) and logs a **frame
//!   line** with the frame split into its steps — the app's view + iced's layout/draw, waiting for
//!   the swapchain image, encode/submit, the GPU's own time — plus what the frame cost us in
//!   images (atlas uploads, bytes, atlas growth), the patch surface's rasterisation, icon
//!   rasterisations and wgpu errors. Logged for the first [`FIRST_FRAMES`] frames (the opening
//!   cost), for every frame slower than `SCRIPTFX_SLOW_FRAME_MS` (default 50, rate-limited), and
//!   as a **percentile summary every 5 s**. `SCRIPTFX_DIAG_FRAMES=all|off` overrides.
//! - a **stall watchdog**: [`phase`](iced_wgpu::frame_stats::phase) marks what the UI thread is
//!   doing (app update, app view, patch raster, icon raster, Lua, iced layout+draw, acquiring the
//!   swapchain image, encode+submit); a frame that takes longer than `SCRIPTFX_STALL_MS`
//!   (default 1000) is logged *while it hangs*, naming the step. That is the difference between
//!   "10 s per frame" and "10 s per frame **in `acquire swapchain image`**".
//! - **bisect switches** ([`switches`]): turn the suspects off one at a time without a build —
//!   patch layer images, icons, the post-process pass, MSAA, animation. Every run logs which were
//!   on, so a log says what produced it.
//! - `SCRIPTFX_DIAG_EXIT_AFTER=<secs>` ends the **standalone** after N seconds (a summary first),
//!   so a tester can double-click one script that runs the whole matrix unattended.
//!
//! Hard crashes (a segfault in a GPU driver, an abort) bypass this; macOS writes its own crash
//! report then (`~/Library/Logs/DiagnosticReports/`, the host's name + `.ips`).
//!
//! Without the feature, [`setup_logger`] returns `None` and nice-plug logs as usual (stderr).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use nice_plug::prelude::nice_log;

use iced_wgpu::frame_stats::{self, Frame, phase};

/// Audio callback statistics since the last heartbeat (all instances in this process).
pub struct AudioStats {
    calls: AtomicU64,
    max_ns: AtomicU64,
    /// The worst callback's budget (the buffer's duration), for the report.
    budget_ns: AtomicU64,
    /// Callbacks that took longer than their buffer lasts (would drop out).
    late: AtomicU64,
    /// Callbacks whose output contained NaN or infinity.
    nonfinite: AtomicU64,
}

pub static AUDIO: AudioStats = AudioStats {
    calls: AtomicU64::new(0),
    max_ns: AtomicU64::new(0),
    budget_ns: AtomicU64::new(0),
    late: AtomicU64::new(0),
    nonfinite: AtomicU64::new(0),
};

impl AudioStats {
    /// One callback: how long it took, how long its buffer lasts, whether the output was finite.
    /// Real-time safe (atomics only).
    pub fn record(&self, took: Duration, budget: Duration, finite: bool) {
        let (took, budget) = (took.as_nanos() as u64, budget.as_nanos() as u64);
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.max_ns.fetch_max(took, Ordering::Relaxed) < took {
            self.budget_ns.store(budget, Ordering::Relaxed);
        }
        if took > budget {
            self.late.fetch_add(1, Ordering::Relaxed);
        }
        if !finite {
            self.nonfinite.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The numbers since the last call, as one line; resets them.
    #[cfg_attr(not(feature = "diagnostics"), allow(dead_code))]
    fn take_report(&self) -> String {
        let calls = self.calls.swap(0, Ordering::Relaxed);
        let max = self.max_ns.swap(0, Ordering::Relaxed) as f64 / 1e6;
        let budget = self.budget_ns.swap(0, Ordering::Relaxed) as f64 / 1e6;
        let late = self.late.swap(0, Ordering::Relaxed);
        let nonfinite = self.nonfinite.swap(0, Ordering::Relaxed);
        if calls == 0 {
            return "audio: no callbacks (stopped or bypassed)".into();
        }
        let load = if budget > 0.0 { max / budget * 100.0 } else { 0.0 };
        format!("audio: {calls} callbacks, worst {max:.3} ms of {budget:.3} ms ({load:.0} %), {late} late, {nonfinite} with NaN/inf output")
    }
}

// ---- bisect switches ----------------------------------------------------------------------------

/// What a run has turned off (environment variables, so a tester needs no build). Every one of
/// these removes one suspect from the rendering path; the startup report lists the active set.
#[derive(Debug, Clone, Copy)]
pub struct Switches {
    /// `SCRIPTFX_DIAG_NO_PATCH_LAYERS`: don't draw the patch surface's rasterised layers (the
    /// biggest images we upload, and the only ones that change every frame).
    pub no_patch_layers: bool,
    /// `SCRIPTFX_DIAG_NO_ICONS`: draw no icon images (they share the texture atlas with the
    /// layers; "icons are pink squares" points at that atlas).
    pub no_icons: bool,
    /// `SCRIPTFX_DIAG_NO_POSTFX`: never set a post-process effect (zoom / soft blur).
    pub no_postfx: bool,
    /// `SCRIPTFX_DIAG_NO_MSAA`: antialiasing off (iced defaults to MSAA ×4, and the shell is
    /// almost entirely canvases).
    pub no_msaa: bool,
    /// `SCRIPTFX_DIAG_STILL`: freeze animation (the patch surface gets a constant time, so its
    /// layers stop changing and stop being re-rasterised and re-uploaded).
    pub still: bool,
    /// `SCRIPTFX_DIAG_FRAMES=all|auto|off`: log every frame, the default selection, or none.
    pub frames: FrameLog,
    /// `SCRIPTFX_SLOW_FRAME_MS` (default 50): a frame at least this slow is logged.
    pub slow_frame_ms: f32,
    /// `SCRIPTFX_STALL_MS` (default 1000): a frame still unfinished after this is logged while it
    /// hangs, with the step it is stuck in.
    pub stall_ms: f32,
    /// `SCRIPTFX_DIAG_EXIT_AFTER` (standalone only): end the process after this long.
    pub exit_after: Option<Duration>,
    /// `SCRIPTFX_DIAG_RUN`: a name for this run, so a matrix of runs is readable in the logs.
    pub run: Option<&'static str>,
    /// `SCRIPTFX_DIAG_SLOW_MS`: sleep this long in every frame, to reproduce a stuttering
    /// machine here. Input then arrives in batches, which is what breaks drags
    /// (`shell/pointer_lock.rs`, `docs/research/render-performance.md`).
    pub slow_ms: Option<Duration>,
}

/// How much of the frame log a run wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameLog {
    /// The first frames, the slow ones, and a summary every 5 s.
    Auto,
    /// Every frame (short, targeted runs).
    All,
    /// Summaries only.
    Off,
}

/// The switches this process runs with (read once).
pub fn switches() -> &'static Switches {
    static S: LazyLock<Switches> = LazyLock::new(|| {
        let flag = |k: &str| std::env::var_os(k).is_some_and(|v| v != "0" && !v.is_empty());
        let num = |k: &str, default: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
        Switches {
            no_patch_layers: flag("SCRIPTFX_DIAG_NO_PATCH_LAYERS"),
            no_icons: flag("SCRIPTFX_DIAG_NO_ICONS"),
            no_postfx: flag("SCRIPTFX_DIAG_NO_POSTFX"),
            no_msaa: flag("SCRIPTFX_DIAG_NO_MSAA"),
            still: flag("SCRIPTFX_DIAG_STILL"),
            frames: match std::env::var("SCRIPTFX_DIAG_FRAMES").unwrap_or_default().as_str() {
                "all" => FrameLog::All,
                "off" => FrameLog::Off,
                _ => FrameLog::Auto,
            },
            slow_frame_ms: num("SCRIPTFX_SLOW_FRAME_MS", 50.0),
            stall_ms: num("SCRIPTFX_STALL_MS", 1000.0),
            exit_after: std::env::var("SCRIPTFX_DIAG_EXIT_AFTER")
                .ok()
                .and_then(|v| v.parse().ok())
                .map(Duration::from_secs_f32),
            run: std::env::var("SCRIPTFX_DIAG_RUN").ok().map(|v| &*v.leak()),
            slow_ms: std::env::var("SCRIPTFX_DIAG_SLOW_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .map(Duration::from_millis),
        }
    });
    &S
}

/// One line naming the switches that are on (`"none"` when the run is unmodified).
fn switch_line() -> String {
    let s = switches();
    let mut on: Vec<&str> = Vec::new();
    for (name, set) in [
        ("no-patch-layers", s.no_patch_layers),
        ("no-icons", s.no_icons),
        ("no-postfx", s.no_postfx),
        ("no-msaa", s.no_msaa),
        ("still", s.still),
    ] {
        if set {
            on.push(name);
        }
    }
    let mut active = if on.is_empty() { "none".to_string() } else { on.join(", ") };
    if let Some(slow) = s.slow_ms {
        active += &format!(" + {} ms sleep per frame", slow.as_millis());
    }
    format!(
        "switches: run \"{}\" · {active} · frames={:?} · slow frame ≥ {:.0} ms · stall ≥ {:.0} ms · exit after {}",
        s.run.unwrap_or("-"),
        s.frames,
        s.slow_frame_ms,
        s.stall_ms,
        s.exit_after.map_or("never".to_string(), |d| format!("{:.0} s", d.as_secs_f32()))
    )
}

// ---- the editor's frames ------------------------------------------------------------------------

/// Frames logged in full when the editor opens: the first frame builds every cache (fonts, the
/// patch's layers, the atlas) and is the one a tester sees as "it took a while".
pub const FIRST_FRAMES: u64 = 20;

/// How often the summary line goes out.
const SUMMARY_EVERY: Duration = Duration::from_secs(5);

/// At most one slow-frame line per this long (a 10 s frame logs every time; 60 fps of 51 ms
/// frames doesn't drown the file).
const SLOW_EVERY: Duration = Duration::from_millis(250);

/// What a frame cost outside the renderer: the parts only we can measure.
#[derive(Debug, Clone, Copy, Default)]
struct Ours {
    /// The patch surface's components: rasterisation time, how many layers it redrew, and how
    /// many pixels that was (the number that quadruples on a Retina screen).
    surface_us: f32,
    surface_rasters: u32,
    surface_px: u64,
    /// Icons rasterised (resvg) during the frame: should be 0 in a steady state.
    icon_rasters: u32,
    /// Events the frame's batch carried, and of those, pointer moves.
    events: u32,
    moves: u32,
    /// Pointer warps the knob drag asked for (`shell/pointer_lock.rs`).
    warps: u32,
}

#[derive(Default)]
struct Samples {
    interval: Vec<f32>,
    ui: Vec<f32>,
    acquire: Vec<f32>,
    render: Vec<f32>,
    swap: Vec<f32>,
    gpu: Vec<f32>,
    surface: Vec<f32>,
    images: u64,
    image_bytes: u64,
    grows: u64,
    icon_rasters: u64,
    events: u64,
}

/// A frame that hasn't finished for far too long.
#[derive(Debug, Clone, Copy)]
struct Stall {
    /// The step it was stuck in when we first noticed.
    phase: u8,
    /// When we first noticed.
    since: Instant,
    /// When we last logged it (once a second is enough).
    reported: Instant,
}

struct State {
    since: Instant,
    samples: Samples,
    /// The worst frame of the current window.
    worst: Option<(Frame, Ours)>,
    last_slow: Option<Instant>,
    last_frame_at: Option<Instant>,
    /// A stall we have already reported, so recovery can be logged once.
    stalled: Option<Stall>,
    /// Frames presented when the window started (for "N frames in 5 s").
    window_start_index: u64,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        since: Instant::now(),
        samples: Samples::default(),
        worst: None,
        last_slow: None,
        last_frame_at: None,
        stalled: None,
        window_start_index: 0,
    })
});

/// Counted as the frame runs, then folded into the frame's line (atomics: the shell writes them
/// from the UI thread, the watchdog may read them from another).
static SURFACE_US: AtomicU64 = AtomicU64::new(0);
static SURFACE_RASTERS: AtomicU64 = AtomicU64::new(0);
static SURFACE_PX: AtomicU64 = AtomicU64::new(0);
static ICON_RASTERS: AtomicU64 = AtomicU64::new(0);
static EVENTS: AtomicU64 = AtomicU64::new(0);
static MOVES: AtomicU64 = AtomicU64::new(0);
static WARPS: AtomicU64 = AtomicU64::new(0);
/// The editor is drawing at the idle rate because nothing is moving (`shell::quality`), not
/// because it can't keep up — without this the log can't tell "slow" from "quiet".
static IDLING: AtomicBool = AtomicBool::new(false);
static EDITOR_OPEN: AtomicBool = AtomicBool::new(false);
static ALLOW_EXIT: AtomicBool = AtomicBool::new(false);
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// The patch surface re-rendered its components (`shell/stage.rs`, every frame).
pub fn note_surface(total_us: f32, rasters: u32, raster_px: u64) {
    SURFACE_US.store(total_us as u64, Ordering::Relaxed);
    SURFACE_RASTERS.store(u64::from(rasters), Ordering::Relaxed);
    SURFACE_PX.store(raster_px, Ordering::Relaxed);
}

/// An icon was rasterised with resvg (a cache miss in `shell/icons.rs`).
pub fn note_icon_raster() {
    ICON_RASTERS.fetch_add(1, Ordering::Relaxed);
}

/// The window saw an input event (`shell/hide_cursor.rs` sees them all); `is_move`: a pointer
/// move. iced hands a frame the whole batch that piled up since the last one, so "moves per
/// frame" is how far input is behind.
pub fn note_event(is_move: bool) {
    EVENTS.fetch_add(1, Ordering::Relaxed);
    if is_move {
        MOVES.fetch_add(1, Ordering::Relaxed);
    }
}

/// Whether the editor is idling (drawing at the idle rate because nothing moved).
pub fn note_idling(idling: bool) {
    IDLING.store(idling, Ordering::Relaxed);
}

/// A knob drag put the pointer back where it was grabbed (`shell/pointer_lock.rs`).
pub fn note_warp() {
    WARPS.fetch_add(1, Ordering::Relaxed);
}

/// Whether the editor is open, so the watchdog knows that no frames means something is wrong.
/// The GPU report isn't logged here — the window boots before the compositor exists, so it goes
/// out with the first frame ([`report_gpu`]).
pub fn editor_open(open: bool) {
    EDITOR_OPEN.store(open, Ordering::Relaxed);
    if open {
        nice_log!("editor: opened · {}", switch_line());
    } else {
        nice_log!("editor: closed · {}", summary_line());
    }
}

/// The GPU as the compositor found it, once per process (the first frame): adapter, backend,
/// driver, whether it is a software adapter, surface format, present mode, antialiasing, and the
/// adapters it could have chosen instead.
fn report_gpu() {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::Relaxed) {
        return;
    }
    match frame_stats::gpu_report() {
        Some(report) => {
            for line in report.lines() {
                nice_log!("{line}");
            }
        }
        None => nice_log!("gpu: no report (the compositor did not record one)"),
    }
}

/// The standalone may end itself (`SCRIPTFX_DIAG_EXIT_AFTER`); inside a host we never would.
pub fn allow_exit() {
    ALLOW_EXIT.store(true, Ordering::Relaxed);
}

/// Starts the render diagnostics: a line per frame (as the switches ask) and the stall watchdog.
/// Called when the editor is created; does its work once per process.
pub fn install() {
    if INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }
    LazyLock::force(&STATE);
    frame_stats::on_frame(on_frame);
    watchdog();
}

/// Every presented frame, on the thread that rendered it.
fn on_frame(f: Frame) {
    if f.index == 1 {
        report_gpu();
    }
    let ours = Ours {
        surface_us: SURFACE_US.swap(0, Ordering::Relaxed) as f32,
        surface_rasters: SURFACE_RASTERS.swap(0, Ordering::Relaxed) as u32,
        surface_px: SURFACE_PX.swap(0, Ordering::Relaxed),
        icon_rasters: ICON_RASTERS.swap(0, Ordering::Relaxed) as u32,
        events: EVENTS.swap(0, Ordering::Relaxed) as u32,
        moves: MOVES.swap(0, Ordering::Relaxed) as u32,
        warps: WARPS.swap(0, Ordering::Relaxed) as u32,
    };
    let Ok(mut st) = STATE.lock() else { return };
    let now = Instant::now();
    st.last_frame_at = Some(now);

    let s = switches();
    let total = f.ui_ms + f.acquire_ms + f.render_ms + f.swap_ms;
    let slow = f.interval_ms >= s.slow_frame_ms || total >= s.slow_frame_ms;

    // A stall we reported is over: say how long it lasted and what the frame ended up costing.
    if let Some(stall) = st.stalled.take() {
        nice_log!(
            "STALL over · frame #{} finished after {:.0} ms stuck in \"{}\" · frame {}",
            f.index,
            stall.since.elapsed().as_secs_f32() * 1000.0,
            phase::name(stall.phase),
            frame_line(&f, &ours)
        );
    }

    let log_this = match s.frames {
        FrameLog::All => true,
        FrameLog::Off => false,
        FrameLog::Auto => {
            f.index <= FIRST_FRAMES
                || (slow && st.last_slow.is_none_or(|t| t.elapsed() >= SLOW_EVERY))
        }
    };
    if log_this {
        if slow {
            st.last_slow = Some(now);
        }
        nice_log!("frame {}", frame_line(&f, &ours));
    }

    // Collect for the summary.
    let sa = &mut st.samples;
    sa.interval.push(f.interval_ms);
    sa.ui.push(f.ui_ms);
    sa.acquire.push(f.acquire_ms);
    sa.render.push(f.render_ms);
    sa.swap.push(f.swap_ms);
    if let Some(g) = f.gpu_ms {
        sa.gpu.push(g);
    }
    sa.surface.push(ours.surface_us / 1000.0);
    sa.images += u64::from(f.image_uploads);
    sa.image_bytes += f.image_bytes;
    sa.grows += u64::from(f.atlas_grows);
    sa.icon_rasters += u64::from(ours.icon_rasters);
    sa.events += u64::from(ours.events);
    if st.worst.is_none_or(|(w, _)| f.interval_ms > w.interval_ms) {
        st.worst = Some((f, ours));
    }

    if st.since.elapsed() >= SUMMARY_EVERY {
        let line = summary(&mut st, f.index);
        nice_log!("frames {line}");
    }
}

/// One frame as one line: where its time went, and what it cost in images.
fn frame_line(f: &Frame, ours: &Ours) -> String {
    let gpu = f.gpu_ms.map_or("n/a".to_string(), |g| format!("{g:.2} ms"));
    // The five steps plus "idle" (waiting for the next frame's trigger) add up to the frame
    // time, so a line says which step a slow frame spent it in.
    let busy = f.ui_ms + f.acquire_ms + f.render_ms + f.swap_ms;
    let idle = (f.interval_ms - busy).max(0.0);
    let mut s = format!(
        "#{} · {:.1} ms ({:.1} fps) · view+iced {:.1} · acquire {:.1} · encode {:.1} \
         · swap {:.1} · idle {:.1} · gpu {gpu}",
        f.index,
        f.interval_ms,
        if f.interval_ms > 0.0 { 1000.0 / f.interval_ms } else { 0.0 },
        f.ui_ms,
        f.acquire_ms,
        f.render_ms,
        f.swap_ms,
        idle,
    );
    if f.image_uploads > 0 || f.atlas_grows > 0 {
        s += &format!(
            " · images {}/{:.2} MiB",
            f.image_uploads,
            f.image_bytes as f32 / (1 << 20) as f32
        );
    }
    s += &format!(" · atlas {} layers/{} allocs", f.atlas_layers, f.atlas_allocations);
    if f.atlas_grows > 0 {
        s += &format!(", {} grows", f.atlas_grows);
    }
    if ours.surface_rasters > 0 || ours.surface_us > 0.0 {
        s += &format!(
            " · surface {:.2} ms/{} rasters/{:.2} Mpx",
            ours.surface_us / 1000.0,
            ours.surface_rasters,
            ours.surface_px as f32 / 1.0e6
        );
    }
    if ours.icon_rasters > 0 {
        s += &format!(" · icon rasters {}", ours.icon_rasters);
    }
    if ours.events > 0 {
        s += &format!(" · events {} ({} moves, {} warps)", ours.events, ours.moves, ours.warps);
    }
    if f.postfx {
        s += " · postfx";
    }
    if IDLING.load(Ordering::Relaxed) {
        s += " · idle (throttled on purpose)";
    }
    if f.wgpu_errors > 0 {
        s += &format!(" · WGPU ERRORS {}", f.wgpu_errors);
    }
    s
}

/// Percentiles of the window that just ended; resets it.
fn summary(st: &mut State, index: u64) -> String {
    let secs = st.since.elapsed().as_secs_f32();
    let frames = index.saturating_sub(st.window_start_index);
    let p = |v: &mut Vec<f32>, q: f32| -> f32 {
        if v.is_empty() {
            return 0.0;
        }
        v.sort_by(f32::total_cmp);
        v[((v.len() - 1) as f32 * q).round() as usize]
    };
    let sa = &mut st.samples;
    let mut line = format!(
        "· {secs:.1} s, {frames} frames, {:.1} fps · interval p50 {:.1} p95 {:.1} max {:.1} ms \
         · view+iced p50 {:.1} p95 {:.1} · acquire p50 {:.1} p95 {:.1} · encode p50 {:.1} p95 {:.1} \
         · swap p50 {:.1} p95 {:.1}",
        frames as f32 / secs.max(0.001),
        p(&mut sa.interval, 0.5),
        p(&mut sa.interval, 0.95),
        p(&mut sa.interval, 1.0),
        p(&mut sa.ui, 0.5),
        p(&mut sa.ui, 0.95),
        p(&mut sa.acquire, 0.5),
        p(&mut sa.acquire, 0.95),
        p(&mut sa.render, 0.5),
        p(&mut sa.render, 0.95),
        p(&mut sa.swap, 0.5),
        p(&mut sa.swap, 0.95),
    );
    if !sa.gpu.is_empty() {
        line += &format!(" · gpu p50 {:.2} p95 {:.2}", p(&mut sa.gpu, 0.5), p(&mut sa.gpu, 0.95));
    }
    line += &format!(
        " · surface p50 {:.2} ms · images {:.1}/frame, {:.1} MiB/s · atlas grows {} · icon rasters {} \
         · events {} · wgpu errors {}",
        p(&mut sa.surface, 0.5),
        sa.images as f32 / frames.max(1) as f32,
        sa.image_bytes as f32 / (1 << 20) as f32 / secs.max(0.001),
        sa.grows,
        sa.icon_rasters,
        sa.events,
        frame_stats::wgpu_errors(),
    );
    if let Some((w, ours)) = st.worst.take() {
        line += &format!("\n    worst {}", frame_line(&w, &ours));
    }
    st.samples = Samples::default();
    st.since = Instant::now();
    st.window_start_index = index;
    line
}

/// The heartbeat's and the close line's view of the editor, without resetting anything.
fn summary_line() -> String {
    let f = frame_stats::get();
    format!(
        "editor: {:.0} fps, view+iced {:.2} ms, render cpu {:.2} ms, gpu {} · wgpu errors {} \
         (stale while the editor is closed)",
        f.fps,
        f.ui_ms,
        f.render_ms,
        f.gpu_ms.map_or("n/a".to_string(), |g| format!("{g:.2} ms")),
        frame_stats::wgpu_errors()
    )
}

/// Watches the UI thread: a frame that hangs is logged *while* it hangs, naming the step it is
/// stuck in — the one thing a log can say that a tester's "it freezes" can't. Also ends the
/// standalone when `SCRIPTFX_DIAG_EXIT_AFTER` says so.
fn watchdog() {
    let s = *switches();
    let _ = std::thread::Builder::new().name("scriptfx-watchdog".into()).spawn(move || {
        let start = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(250));

            if let Some(after) = s.exit_after
                && ALLOW_EXIT.load(Ordering::Relaxed)
                && start.elapsed() >= after
            {
                nice_log!("exit: SCRIPTFX_DIAG_EXIT_AFTER reached · {}", summary_line());
                std::process::exit(0);
            }

            if !EDITOR_OPEN.load(Ordering::Relaxed) {
                continue;
            }
            let (p, ms) = phase::current();
            let Ok(mut st) = STATE.lock() else { continue };
            let idle_ms = st.last_frame_at.map_or(0.0, |t| t.elapsed().as_secs_f32() * 1000.0);
            // Stuck inside a frame, or no frame at all for as long.
            let stuck = (p != phase::IDLE && ms >= s.stall_ms).then_some(p);
            let starved = (p == phase::IDLE && st.last_frame_at.is_some() && idle_ms >= s.stall_ms)
                .then_some(phase::IDLE);
            let Some(p) = stuck.or(starved) else { continue };
            let now = Instant::now();
            match &mut st.stalled {
                // Already reported: once a second while it lasts.
                Some(stall) if stall.reported.elapsed() < Duration::from_secs(1) => continue,
                Some(stall) => stall.reported = now,
                none => *none = Some(Stall { phase: p, since: now, reported: now }),
            }
            nice_log!(
                "STALL · in \"{}\" for {:.0} ms · no frame for {:.0} ms · {}",
                phase::name(p),
                ms,
                idle_ms,
                summary_line()
            );
        }
    });
}

#[cfg(feature = "diagnostics")]
pub use enabled::setup_logger;

/// Without the feature: nice-plug's own logger.
#[cfg(not(feature = "diagnostics"))]
pub fn setup_logger() -> Option<bool> {
    None
}

#[cfg(feature = "diagnostics")]
mod enabled {
    use std::fs::{self, File};
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use nice_plug::prelude::*;
    use tracing_subscriber::util::SubscriberInitExt;

    /// Installs the file logger, writes the system report, starts the heartbeat. `Some(true)`
    /// makes nice-plug route panics into it.
    pub fn setup_logger() -> Option<bool> {
        let dir = log_dir();
        let _ = fs::create_dir_all(&dir);
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let path = dir.join(format!("scriptfx-{stamp}-{}.log", std::process::id()));
        let file = File::create(&path).ok()?;
        // `SCRIPTFX_LOG` is a tracing filter, so one run can ask for more without a build:
        // `SCRIPTFX_LOG=info,iced_wgpu::image=debug` logs every texture atlas allocation.
        let spec = std::env::var("SCRIPTFX_LOG").unwrap_or_else(|_| "info".into());
        let filter = tracing_subscriber::EnvFilter::try_new(&spec)
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
        let ok = tracing_subscriber::fmt()
            .with_writer(Mutex::new(file))
            .with_ansi(false)
            .with_thread_names(true)
            .with_target(true)
            .with_env_filter(filter)
            .finish()
            .try_init()
            .is_ok();
        if !ok {
            return Some(false);
        }
        report_system(&path);
        heartbeat();
        Some(true)
    }

    fn log_dir() -> PathBuf {
        #[cfg(target_os = "macos")]
        if let Some(home) = dirs::home_dir() {
            return home.join("Library/Logs/ScriptFX");
        }
        dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("ScriptFX").join("logs")
    }

    fn report_system(path: &std::path::Path) {
        nice_log!("==== ScriptFX diagnostics build ====");
        nice_log!("log file: {}", path.display());
        nice_log!(
            "version {} · commit {} · {} {} · debug assertions {}",
            env!("CARGO_PKG_VERSION"),
            option_env!("SCRIPTFX_GIT_SHA").unwrap_or("local"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            cfg!(debug_assertions)
        );
        nice_log!("os: {}", os_version());
        nice_log!("cpus: {}", std::thread::available_parallelism().map_or(0, |n| n.get()));
        nice_log!("process: pid {} · {:?}", std::process::id(), std::env::current_exe().ok());
        nice_log!("args: {:?}", std::env::args().collect::<Vec<_>>());
        nice_log!("patches: {}", crate::patches_root().display());
        nice_log!("data dir: {:?}", dirs::data_dir());
        nice_log!("LANG={:?} LC_ALL={:?}", std::env::var("LANG").ok(), std::env::var("LC_ALL").ok());
        nice_log!("{}", super::switch_line());
        // The renderer's own environment overrides, so a log says which backend produced it.
        nice_log!(
            "wgpu env: WGPU_BACKEND={:?} WGPU_POWER_PREF={:?} ICED_PRESENT_MODE={:?} SCRIPTFX_FRAME_US={:?}",
            std::env::var("WGPU_BACKEND").ok(),
            std::env::var("WGPU_POWER_PREF").ok(),
            std::env::var("ICED_PRESENT_MODE").ok(),
            std::env::var("SCRIPTFX_FRAME_US").ok()
        );
    }

    fn os_version() -> String {
        #[cfg(target_os = "macos")]
        {
            let run = |cmd: &str, args: &[&str]| {
                std::process::Command::new(cmd).args(args).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            };
            format!(
                "macOS {} ({}) · {}",
                run("sw_vers", &["-productVersion"]).unwrap_or_default(),
                run("sw_vers", &["-buildVersion"]).unwrap_or_default(),
                run("sysctl", &["-n", "machdep.cpu.brand_string"]).unwrap_or_default()
            )
        }
        #[cfg(target_os = "linux")]
        {
            fs::read_to_string("/etc/os-release")
                .ok()
                .and_then(|s| s.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_string())))
                .unwrap_or_else(|| "Linux".into())
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            std::env::consts::OS.to_string()
        }
    }

    /// Every 10 s: the audio callback numbers and the editor's frame timing.
    fn heartbeat() {
        let _ = std::thread::Builder::new().name("scriptfx-diag".into()).spawn(|| loop {
            std::thread::sleep(Duration::from_secs(10));
            nice_log!("heartbeat · {}", super::AUDIO.take_report());
            nice_log!("heartbeat · {}", super::summary_line());
        });
    }
}
