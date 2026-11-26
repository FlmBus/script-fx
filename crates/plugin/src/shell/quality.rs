//! What the editor is allowed to spend per frame: the user's **quality** level and **frame rate**
//! ([`crate::user_settings`]), with the diagnostics switches (`diag.rs`) layered on top, plus the
//! **idle throttle**.
//!
//! Read from several places that have no access to the shell's state (the frame clock's thread,
//! the patch surface's clock, the renderer's blur), so the effective values live here as atomics
//! and the shell calls [`apply`] whenever the settings change. They are per user and per process,
//! like the settings file itself.
//!
//! **Multisampling is not here**: iced takes it when the compositor is made, so it is read once in
//! `shell::create` and a change needs the window reopened.
//!
//! ## The idle throttle
//!
//! The shell draws on a clock (`shell::frames`), not on demand, and until 2026-11-26 it also had
//! iced_baseview's `always_redraw`, so it redrew everything ~66 times a second whether or not
//! anything had changed. Now the clock is the only trigger, and it slows to [`IDLE_FPS`] when
//! nothing has moved for [`IDLE_AFTER`].
//!
//! Anything the user does counts as movement ([`note_activity`], called for every message that
//! isn't the frame tick), as does anything that visibly changes (meters, modulators, a patch
//! component that re-rendered). A case we forget costs frames, not correctness: an animation we
//! failed to notice runs at [`IDLE_FPS`] instead of stopping, and the next event brings it back up
//! immediately.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::Thread;
use std::time::{Duration, Instant};

use crate::user_settings::{Quality, UserSettings};

/// How fast the editor still redraws when nothing is happening.
pub const IDLE_FPS: u32 = 10;

/// Quiet for this long → idle.
pub const IDLE_AFTER: Duration = Duration::from_millis(900);

static FRAME_US: AtomicU64 = AtomicU64::new(16_667);
static ANIMATIONS: AtomicBool = AtomicBool::new(true);
static BLUR: AtomicBool = AtomicBool::new(true);
static MSAA: AtomicBool = AtomicBool::new(true);
/// Milliseconds since [`EPOCH`] at the last sign of life.
static LAST_ACTIVITY: AtomicU64 = AtomicU64::new(0);

static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
/// The frame clock's thread, so leaving idle doesn't have to wait out its long sleep.
static CLOCK: std::sync::OnceLock<Thread> = std::sync::OnceLock::new();

fn now_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Takes the user's settings (and the diagnostics switches over them) into effect.
pub fn apply(settings: &UserSettings) {
    let diag = crate::diag::switches();
    FRAME_US.store(1_000_000 / u64::from(settings.frame_rate.max(1)), Ordering::Relaxed);
    ANIMATIONS.store(settings.quality.animations() && !diag.still, Ordering::Relaxed);
    BLUR.store(settings.quality.blur() && !diag.no_postfx, Ordering::Relaxed);
    MSAA.store(settings.quality.msaa() && !diag.no_msaa, Ordering::Relaxed);
    note_activity();
}

/// Is the patch's own UI allowed to move? Off freezes its clock, so its components stop being
/// re-rendered and re-uploaded — by far the biggest per-frame cost on a weak machine.
pub fn animations() -> bool {
    ANIMATIONS.load(Ordering::Relaxed)
}

/// Is the renderer's blur pass (the block modal's zoom and backdrop) allowed?
pub fn blur() -> bool {
    BLUR.load(Ordering::Relaxed)
}

/// Multisampling for a window opening now. Read once per window by `shell::create`: iced fixes it
/// when it makes the compositor, so a change only shows after the window is reopened.
pub fn msaa() -> bool {
    MSAA.load(Ordering::Relaxed)
}

/// Something happened that the editor should show right away.
pub fn note_activity() {
    let was_idle = idling();
    LAST_ACTIVITY.store(now_ms(), Ordering::Relaxed);
    // Coming out of idle, draw now instead of after up to a tenth of a second. While not idle the
    // clock is left alone, or input would push the frame rate past the limit the user set.
    if was_idle && let Some(clock) = CLOCK.get() {
        clock.unpark();
    }
}

/// The frame clock announces itself here, then waits with [`wait`].
pub fn register_clock() {
    let _ = CLOCK.set(std::thread::current());
}

/// Sleeps until the next frame is due, or until [`note_activity`] ends an idle stretch.
pub fn wait() {
    std::thread::park_timeout(frame_interval());
}


/// How long the frame clock should sleep before the next frame: the user's rate, or the idle rate
/// when nothing has moved for a while.
pub fn frame_interval() -> Duration {
    let quiet = now_ms().saturating_sub(LAST_ACTIVITY.load(Ordering::Relaxed));
    if quiet >= IDLE_AFTER.as_millis() as u64 {
        Duration::from_micros(1_000_000 / u64::from(IDLE_FPS))
    } else {
        Duration::from_micros(FRAME_US.load(Ordering::Relaxed))
    }
}

/// Is the editor idling right now (for the diagnostics log)?
pub fn idling() -> bool {
    now_ms().saturating_sub(LAST_ACTIVITY.load(Ordering::Relaxed)) >= IDLE_AFTER.as_millis() as u64
}

/// The numbers a frame draws from, as one value: when it changes, something moved. Cheap enough
/// to do every frame, and it misses nothing that is driven by a value (meters, modulators, a
/// re-rendered patch component). Things driven only by the clock keep animating at [`IDLE_FPS`].
pub fn fingerprint(values: &patch::Values, surface_rasters: usize, extra: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |bits: u32| {
        h ^= u64::from(bits);
        h = h.wrapping_mul(0x1000_0000_01b3);
    };
    mix(values.meter_in.to_bits());
    mix(values.meter_out.to_bits());
    for v in values.modulators.iter().chain(values.modulator_phases.iter()).chain(values.blocks.iter()) {
        mix(v.to_bits());
    }
    for v in values.macros.iter().chain(values.params.iter()) {
        mix(v.to_bits());
    }
    for v in extra {
        mix(v.to_bits());
    }
    mix(surface_rasters as u32);
    h
}

/// A level's own description, for the settings sheet.
pub fn hint(q: Quality) -> &'static str {
    match q {
        Quality::High => "everything on: smooth edges, the modal's blur, the patch's animation",
        Quality::Medium => "no smoothing (needs a reopen); blur and animation stay",
        Quality::Low => "no smoothing or blur, the patch's animation frozen: for weak machines",
    }
}
