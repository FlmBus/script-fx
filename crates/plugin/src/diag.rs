//! Diagnostics builds (`--features diagnostics`): for test sessions on machines without developer
//! tools, where the only way back is a file. Everything goes into one log file per process:
//!
//! - **where:** `~/Library/Logs/ScriptFX/` (macOS), `%LOCALAPPDATA%\ScriptFX\logs\` (Windows),
//!   `~/.local/share/ScriptFX/logs/` (Linux); name `scriptfx-<unix time>-<pid>.log`;
//! - **what:** a system report at start (versions, OS, CPU, the host process, paths), everything
//!   `nice_log!` and the `log` crate say (iced, wgpu: GPU adapter, warnings, errors) with time and
//!   thread, panics with a backtrace (nice-plug's panic hook logs into the same file), and a
//!   **heartbeat every 10 s**: audio callback timing (calls, worst time against the buffer's
//!   real-time budget, late callbacks, non-finite output) and the editor's frame timing.
//!
//! Hard crashes (a segfault in a GPU driver, an abort) bypass this; macOS writes its own crash
//! report then (`~/Library/Logs/DiagnosticReports/`, the host's name + `.ips`).
//!
//! Without the feature, [`setup_logger`] returns `None` and nice-plug logs as usual (stderr);
//! the audio statistics are still counted (a few atomics), nothing reads them.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

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
        let ok = tracing_subscriber::fmt()
            .with_writer(Mutex::new(file))
            .with_ansi(false)
            .with_thread_names(true)
            .with_target(true)
            .with_max_level(tracing::Level::INFO)
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
            let f = iced_wgpu::frame_stats::get();
            nice_log!(
                "heartbeat · editor: {:.0} fps, render cpu {:.2} ms, gpu {} (stale while the editor is closed)",
                f.fps,
                f.render_ms,
                f.gpu_ms.map_or("n/a".to_string(), |g| format!("{g:.2} ms"))
            );
        });
    }
}
