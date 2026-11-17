//! Lua formats numbers with the C library, which follows the *process* locale.
//! Hosts like Carla (Qt) switch the process to the user's locale, where e.g. German
//! uses a decimal comma: `string.format("%.2f", 0.5)` → "0,50", `tostring(0.5)` →
//! "0,5". That silently breaks every number in generated SVG.
//!
//! [`CNumeric`] switches only the *current thread* to the "C" number format while
//! Lua runs and restores the previous setting afterwards; the host is unaffected.

#[cfg(unix)]
mod imp {
    use std::sync::OnceLock;

    struct Loc(libc::locale_t);
    // The C locale object is immutable once created.
    unsafe impl Send for Loc {}
    unsafe impl Sync for Loc {}

    fn c_numeric() -> libc::locale_t {
        static LOC: OnceLock<Loc> = OnceLock::new();
        LOC.get_or_init(|| unsafe {
            // Base on a copy of the current global locale, override LC_NUMERIC only.
            let base = libc::duplocale(-1isize as libc::locale_t); // LC_GLOBAL_LOCALE
            Loc(libc::newlocale(libc::LC_NUMERIC_MASK, c"C".as_ptr(), base))
        })
        .0
    }

    pub struct CNumeric(libc::locale_t);

    impl CNumeric {
        pub fn enter() -> Self {
            let loc = c_numeric();
            if loc.is_null() {
                return CNumeric(std::ptr::null_mut());
            }
            CNumeric(unsafe { libc::uselocale(loc) })
        }
    }

    impl Drop for CNumeric {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { libc::uselocale(self.0) };
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::{CStr, CString};

    const ENABLE_PER_THREAD_LOCALE: i32 = 1;
    unsafe extern "C" {
        fn _configthreadlocale(flag: i32) -> i32;
        fn setlocale(category: i32, locale: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    }
    const LC_NUMERIC: i32 = 4;

    pub struct CNumeric {
        prev_mode: i32,
        prev: Option<CString>,
    }

    impl CNumeric {
        pub fn enter() -> Self {
            unsafe {
                let prev_mode = _configthreadlocale(ENABLE_PER_THREAD_LOCALE);
                let cur = setlocale(LC_NUMERIC, std::ptr::null());
                let prev = (!cur.is_null()).then(|| CStr::from_ptr(cur).to_owned());
                setlocale(LC_NUMERIC, c"C".as_ptr());
                CNumeric { prev_mode, prev }
            }
        }
    }

    impl Drop for CNumeric {
        fn drop(&mut self) {
            unsafe {
                if let Some(p) = &self.prev {
                    setlocale(LC_NUMERIC, p.as_ptr());
                }
                _configthreadlocale(self.prev_mode);
            }
        }
    }
}

pub use imp::CNumeric;
