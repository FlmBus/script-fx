//! A knob drag with the pointer locked in place (as in Serum): the pointer stays where the knob
//! was grabbed, the knob follows the mouse's travel. The cursor is hidden meanwhile
//! (`hide_cursor`); when it reappears, it's where it was.
//!
//! How: [`Drag::start`] remembers the pointer's screen position. Every movement turns into a
//! change of the value, then the pointer is put back there. The value moves by the travel since
//! the last event (not by the distance to the start), so the knob keeps turning however far the
//! mouse goes, also past the screen's edge, and reverses the moment the mouse does.
//!
//! Neither iced nor baseview can move the pointer, so this talks to the system directly:
//! X11 (also under XWayland) via x11rb; Windows via `GetCursorPos` / `SetCursorPos`; macOS via
//! CoreGraphics. Only X11 is tested. Elsewhere, or if the system refuses, the pointer simply
//! moves (the drag works as before).

use std::cell::Cell;
use std::ops::RangeInclusive;

/// A vertical drag on a value.
pub struct Drag {
    value: f32,
    /// The pointer's y at the last event (the caller's coordinates).
    last_y: f32,
    /// Where the pointer was grabbed (the caller's coordinates): where it's put back to.
    anchor_y: f32,
}

impl Drag {
    /// Starts at `value`; `y`: the pointer, in the coordinates later `moved` calls use.
    pub fn start(value: f32, y: f32) -> Self {
        ANCHOR.with(|a| a.set(platform::position()));
        Self { value, last_y: y, anchor_y: y }
    }

    /// The pointer moved to `y`: the value moves by the travel (up = more, `per_px` per pixel),
    /// clamped to `range`; then the pointer goes back to where it was grabbed.
    pub fn moved(&mut self, y: f32, per_px: f32, range: RangeInclusive<f32>) -> f32 {
        self.value = (self.value - (y - self.last_y) * per_px).clamp(*range.start(), *range.end());
        // Put back: the next event then counts from the grab point (the warp's own motion event
        // arrives there and adds nothing).
        let warped = ANCHOR.with(|a| a.get()).is_some_and(platform::warp);
        self.last_y = if warped { self.anchor_y } else { y };
        self.value
    }
}

impl Drop for Drag {
    fn drop(&mut self) {
        ANCHOR.with(|a| a.set(None));
    }
}

thread_local! {
    /// The grab point in screen coordinates (only one drag runs at a time: one mouse).
    static ANCHOR: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
}

// ---- platforms ----------------------------------------------------------------------------------

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "macos")]
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

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod platform {
    pub fn position() -> Option<(i32, i32)> {
        None
    }

    pub fn warp(_: (i32, i32)) -> bool {
        false
    }
}
