//! A knob drag with the pointer locked in place (as in Serum): the pointer stays near where the
//! knob was grabbed, the knob follows the mouse's travel. The cursor is hidden meanwhile
//! (`hide_cursor`); when it reappears, it's where it was ([`Drag`]'s `Drop` puts it back).
//!
//! How: [`Drag::start`] remembers the pointer's screen position. Every movement turns into a
//! change of the value, by the travel **since the previous event** (not by the distance to the
//! start), so the knob keeps turning however far the mouse goes and reverses the moment the mouse
//! does. When the pointer has wandered more than [`PIN_AT`] from the grab point it is put back
//! there, so a drag can't run out of screen.
//!
//! ## Why this is careful about slow frames
//!
//! iced hands a frame **the whole batch** of events that piled up since the last one (see
//! `docs/research/render-performance.md`). So `moved` is called several times in a row for
//! movements that already happened, and the pointer we warp has long moved on. Three rules keep
//! that honest:
//!
//! 1. the travel is always measured between **consecutive events**, never against the grab point
//!    — the earlier code reset the reference to the grab point after every warp, so each queued
//!    event contributed the *whole* travel again and a stuttering drag slammed the knob into min
//!    or max (reported on Windows, 2026-11-26);
//! 2. a warp makes the system send a motion event of its own, which is **recognised and
//!    swallowed** ([`PIN_EPS`]) instead of counting as travel;
//! 3. one event can move the value by at most [`MAX_STEP`] pixels of travel — a backstop, so no
//!    single confused event can cross the whole range.
//!
//! Releases are counted window-wide ([`note_release`]), so a drag whose release got lost in a
//! batch can tell ([`Drag::ended`]) instead of turning forever.
//!
//! Neither iced nor baseview can move the pointer, so this talks to the system directly:
//! X11 (also under XWayland) via x11rb; Windows via `GetCursorPos` / `SetCursorPos`; macOS via
//! CoreGraphics. Only X11 is tested. Elsewhere, or if the system refuses, the pointer simply
//! moves (the drag still works, it can just reach the screen's edge).

use std::cell::Cell;
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicU64, Ordering};

/// How far the pointer may wander from the grab point before it is put back. Large enough that a
/// normal drag never warps at all (200 px is a knob's whole range), small enough to stay on screen.
const PIN_AT: f32 = 120.0;

/// A motion event this close to the grab point, right after a warp, is the warp's own.
const PIN_EPS: f32 = 2.0;

/// The most travel one event may carry (pixels). A fast flick is a few hundred pixels per frame;
/// anything beyond this is a confused event, not a gesture.
const MAX_STEP: f32 = 300.0;

/// A vertical drag on a value.
pub struct Drag {
    value: f32,
    /// The pointer's y at the last event (the caller's coordinates).
    last_y: f32,
    /// Where the pointer was grabbed (the caller's coordinates): where it's put back to.
    anchor_y: f32,
    /// Warps whose own motion event we are still expecting.
    pending_warps: u32,
    /// [`RELEASES`] when the drag started: a change means the button has gone up since.
    releases: u64,
}

impl Drag {
    /// Starts at `value`; `y`: the pointer, in the coordinates later `moved` calls use.
    pub fn start(value: f32, y: f32) -> Self {
        ANCHOR.with(|a| a.set(platform::position()));
        Self {
            value,
            last_y: y,
            anchor_y: y,
            pending_warps: 0,
            releases: RELEASES.load(Ordering::Relaxed),
        }
    }

    /// The pointer moved to `y`: the value moves by the travel since the previous event (up =
    /// more, `per_px` per pixel), clamped to `range`.
    pub fn moved(&mut self, y: f32, per_px: f32, range: RangeInclusive<f32>) -> f32 {
        // Our own warp reports back: that is not the user moving the mouse.
        if self.pending_warps > 0 && (y - self.anchor_y).abs() <= PIN_EPS {
            self.pending_warps -= 1;
            self.last_y = y;
            return self.value;
        }

        let travel = (y - self.last_y).clamp(-MAX_STEP, MAX_STEP);
        self.last_y = y;
        self.value = (self.value - travel * per_px).clamp(*range.start(), *range.end());

        // Far from the grab point: put the pointer back, so the drag can't run out of screen.
        // Rare by design — every warp costs an event we have to recognise again.
        if (y - self.anchor_y).abs() > PIN_AT
            && let Some(anchor) = ANCHOR.with(|a| a.get())
            && platform::warp(anchor)
        {
            crate::diag::note_warp();
            self.pending_warps += 1;
            // The pointer is at the grab point now, so that is where the next event comes from.
            self.last_y = self.anchor_y;
        }
        self.value
    }

    /// Has the left button gone up since this drag started? True when the release never reached
    /// the control that owns the drag (it can be swallowed by a batch, see the module docs): the
    /// owner should end the drag — and its host gesture — itself.
    pub fn ended(&self) -> bool {
        RELEASES.load(Ordering::Relaxed) != self.releases
    }
}

impl Drop for Drag {
    fn drop(&mut self) {
        // The cursor reappears where it was grabbed, however far the mouse travelled.
        if let Some(anchor) = ANCHOR.with(|a| a.take()) {
            let _ = platform::warp(anchor);
        }
    }
}

/// A left-button release, wherever it landed in the window (`hide_cursor` sees them all).
pub fn note_release() {
    RELEASES.fetch_add(1, Ordering::Relaxed);
}

/// Left-button releases seen in this process. Shared by every window: one mouse, one button, so a
/// release anywhere means no drag can still be held.
static RELEASES: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// The grab point in screen coordinates (only one drag runs at a time: one mouse).
    static ANCHOR: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
}

// ---- platforms ----------------------------------------------------------------------------------

#[cfg(all(not(test), target_os = "linux"))]
mod platform {
    use std::cell::OnceCell;

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, Window};
    use x11rb::rust_connection::RustConnection;

    thread_local! {
        /// Our own connection to the X server (the editor's belongs to baseview).
        static X: OnceCell<Option<(RustConnection, Window)>> = const { OnceCell::new() };
    }

    fn with<T>(f: impl FnOnce(&RustConnection, Window) -> Option<T>) -> Option<T> {
        X.with(|x| {
            let x = x.get_or_init(|| {
                let (conn, screen) = x11rb::connect(None).ok()?;
                let root = conn.setup().roots.get(screen)?.root;
                Some((conn, root))
            });
            let (conn, root) = x.as_ref()?;
            f(conn, *root)
        })
    }

    pub fn position() -> Option<(i32, i32)> {
        with(|conn, root| {
            let p = conn.query_pointer(root).ok()?.reply().ok()?;
            Some((p.root_x as i32, p.root_y as i32))
        })
    }

    pub fn warp((x, y): (i32, i32)) -> bool {
        with(|conn, root| {
            conn.warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, x as i16, y as i16).ok()?;
            conn.flush().ok()
        })
        .is_some()
    }
}

#[cfg(all(not(test), target_os = "windows"))]
mod platform {
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetCursorPos(p: *mut Point) -> i32;
        fn SetCursorPos(x: i32, y: i32) -> i32;
    }

    pub fn position() -> Option<(i32, i32)> {
        let mut p = Point { x: 0, y: 0 };
        (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x, p.y))
    }

    pub fn warp((x, y): (i32, i32)) -> bool {
        unsafe { SetCursorPos(x, y) != 0 }
    }
}

#[cfg(all(not(test), target_os = "macos"))]
mod platform {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
        fn CGWarpMouseCursorPosition(p: CGPoint) -> i32;
        fn CGAssociateMouseAndMouseCursorPosition(connected: u8) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    pub fn position() -> Option<(i32, i32)> {
        unsafe {
            let e = CGEventCreate(std::ptr::null());
            if e.is_null() {
                return None;
            }
            let p = CGEventGetLocation(e);
            CFRelease(e);
            Some((p.x.round() as i32, p.y.round() as i32))
        }
    }

    pub fn warp((x, y): (i32, i32)) -> bool {
        unsafe {
            let ok = CGWarpMouseCursorPosition(CGPoint { x: x as f64, y: y as f64 }) == 0;
            // A warp freezes the mouse for a moment unless re-associated right away.
            CGAssociateMouseAndMouseCursorPosition(1);
            ok
        }
    }
}

#[cfg(all(not(test), not(any(target_os = "linux", target_os = "windows", target_os = "macos"))))]
mod platform {
    pub fn position() -> Option<(i32, i32)> {
        None
    }

    pub fn warp(_: (i32, i32)) -> bool {
        false
    }
}

/// In tests the pointer is ours to move: a thread-local position and a count of the warps, so the
/// batched-event cases below can be played through exactly as a stuttering frame delivers them.
#[cfg(test)]
mod platform {
    use std::cell::Cell;

    thread_local! {
        static POSITION: Cell<(i32, i32)> = const { Cell::new((0, 0)) };
        static WARPS: Cell<u32> = const { Cell::new(0) };
    }

    pub fn position() -> Option<(i32, i32)> {
        Some(POSITION.with(Cell::get))
    }

    pub fn warp(to: (i32, i32)) -> bool {
        POSITION.with(|p| p.set(to));
        WARPS.with(|w| w.set(w.get() + 1));
        true
    }

    /// Warps so far on this thread.
    pub fn warps() -> u32 {
        WARPS.with(Cell::get)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 200 px for the whole range, as the knobs use.
    const PER_PX: f32 = 1.0 / 200.0;

    /// The travel of each event counts once: a batch of small moves adds up to the distance the
    /// mouse actually went, not to a multiple of it.
    #[test]
    fn consecutive_events_add_up_to_the_travel() {
        let mut d = Drag::start(0.5, 300.0);
        d.moved(290.0, PER_PX, 0.0..=1.0);
        d.moved(280.0, PER_PX, 0.0..=1.0);
        let v = d.moved(265.0, PER_PX, 0.0..=1.0);
        // 35 px up from 300.
        assert!((v - (0.5 + 35.0 * PER_PX)).abs() < 1e-6, "{v}");
    }

    /// The reported Windows bug: when frames stall, iced delivers the whole batch of moves at
    /// once. Each must still count only its own step — measured against the grab point instead,
    /// they summed to far more than the mouse travelled and the knob slammed into the rail.
    #[test]
    fn a_batch_of_stale_moves_does_not_multiply_the_travel() {
        let mut d = Drag::start(0.5, 300.0);
        // The mouse went 30 px up over four events that all arrive in one frame.
        let mut v = 0.0;
        for y in [295.0, 290.0, 285.0, 270.0] {
            v = d.moved(y, PER_PX, 0.0..=1.0);
        }
        assert!((v - (0.5 + 30.0 * PER_PX)).abs() < 1e-6, "{v}");
        // What the old code did (each event against the grab point) would have been 5+10+15+30 px.
        assert!(v < 0.5 + 60.0 * PER_PX);
    }

    /// Beyond `PIN_AT` the pointer is put back to the grab point, and the motion event that warp
    /// produces is recognised: it must not count as travel (that would undo the drag).
    #[test]
    fn the_warps_own_event_is_swallowed() {
        let warps_before = platform::warps();
        let mut d = Drag::start(0.5, 300.0);
        // One big move, past the pin distance.
        let after_move = d.moved(300.0 - PIN_AT - 10.0, PER_PX / 10.0, 0.0..=1.0);
        assert_eq!(platform::warps(), warps_before + 1, "the pointer should have been pinned back");
        assert!(after_move > 0.5);

        // The warp reports the pointer at the grab point: no change.
        let after_warp_event = d.moved(300.0, PER_PX / 10.0, 0.0..=1.0);
        assert!((after_warp_event - after_move).abs() < 1e-6, "{after_warp_event} != {after_move}");

        // Real movement from there counts again.
        let after_next = d.moved(290.0, PER_PX / 10.0, 0.0..=1.0);
        assert!((after_next - (after_move + 10.0 * PER_PX / 10.0)).abs() < 1e-6, "{after_next}");
    }

    /// Without a warp pending, an event at the grab point is the user moving back there.
    #[test]
    fn an_event_at_the_grab_point_counts_when_no_warp_is_pending() {
        let mut d = Drag::start(0.5, 300.0);
        d.moved(280.0, PER_PX, 0.0..=1.0);
        let v = d.moved(300.0, PER_PX, 0.0..=1.0);
        assert!((v - 0.5).abs() < 1e-6, "{v}");
    }

    /// A single event can't carry more than `MAX_STEP` of travel, however confused it is.
    #[test]
    fn one_event_is_capped() {
        let per_px = 1.0 / 2000.0;
        let mut d = Drag::start(0.5, 3000.0);
        let v = d.moved(-2000.0, per_px, 0.0..=1.0);
        assert!((v - (0.5 + MAX_STEP * per_px)).abs() < 1e-6, "{v}");
    }

    /// A release that never reached the control still ends its drag.
    #[test]
    fn a_missed_release_ends_the_drag() {
        let d = Drag::start(0.5, 300.0);
        assert!(!d.ended());
        note_release();
        assert!(d.ended());
        // A drag started afterwards is not affected by that release.
        assert!(!Drag::start(0.5, 300.0).ended());
    }
}
