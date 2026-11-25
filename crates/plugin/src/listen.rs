//! Listen: capture → background analysis → result (shown in the chat, attached to the next
//! message). The capture itself runs on the audio thread (`Shared::capture_block`).

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use analysis::Analysis;

use crate::captures;
use crate::{LISTEN_ARMED, LISTEN_RECORDING, Shared};

pub struct CaptureResult {
    pub seconds: f32,
    pub input: Analysis,
    pub output: Analysis,
    /// The capture's id on disk. This is what the LLM will name in `get_analysis` to fetch a
    /// section (`captures::read`); the analysis itself never goes into the conversation.
    #[allow(dead_code)]
    pub id: Option<String>,
    /// Where the analyses and WAVs were saved.
    pub dir: Option<PathBuf>,
}

pub struct Listen {
    /// Capture length (10, 20 or 30 s).
    pub seconds: f32,
    pending: Option<Arc<Mutex<Option<CaptureResult>>>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Idle,
    /// Waiting for signal.
    Armed,
    Recording { seconds: f32, of: f32 },
    Analyzing,
}

impl Listen {
    pub fn new() -> Self {
        Self { seconds: 10.0, pending: None }
    }

    /// Picks up finished captures and analyses. Call once per frame; returns a result once,
    /// when its analysis is done. `track` and `patch` only label the capture on disk.
    pub fn poll(&mut self, shared: &Shared, track: Option<&str>, patch: Option<&str>) -> Option<Arc<CaptureResult>> {
        if let Some((input, output)) = shared.take_capture() {
            let sr = shared.sample_rate();
            let slot = Arc::new(Mutex::new(None));
            self.pending = Some(slot.clone());
            let (track, patch) = (track.map(str::to_owned), patch.map(str::to_owned));
            std::thread::spawn(move || {
                let seconds = input.len() as f32 / sr;
                let a_in = analysis::analyze(&input, sr);
                let a_out = analysis::analyze(&output, sr);
                // The analyses go to disk; the chat and the LLM get the digest and fetch
                // sections from there (`captures::read`), so no JSON travels in the context.
                let recording = captures::Recording {
                    name: track,
                    focus: true,
                    input,
                    output,
                    input_analysis: a_in.clone(),
                    output_analysis: a_out.clone(),
                };
                let dir = captures::save(std::slice::from_ref(&recording), sr, patch)
                    .map_err(|e| nice_plug::nice_log!("saving capture failed: {e}"))
                    .ok();
                let id = dir.as_ref().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned());
                *slot.lock().unwrap() = Some(CaptureResult { seconds, input: a_in, output: a_out, id, dir });
            });
        }
        let finished = self.pending.as_ref().and_then(|slot| slot.lock().unwrap().take())?;
        self.pending = None;
        Some(Arc::new(finished))
    }

    pub fn start(&mut self, shared: &Shared) {
        if self.status(shared) == Status::Idle {
            shared.start_listen(self.seconds);
        }
    }

    pub fn cancel(&mut self, shared: &Shared) {
        shared.cancel_listen();
    }

    pub fn status(&self, shared: &Shared) -> Status {
        if self.pending.is_some() {
            return Status::Analyzing;
        }
        match shared.listen_state.load(Ordering::Relaxed) {
            LISTEN_ARMED => Status::Armed,
            LISTEN_RECORDING => {
                let sr = shared.sample_rate();
                let target = shared.capture.try_lock().map(|c| c.target).unwrap_or(1).max(1);
                let got = shared.listen_progress.load(Ordering::Relaxed);
                Status::Recording { seconds: got as f32 / sr, of: target as f32 / sr }
            }
            _ => Status::Idle,
        }
    }
}

impl CaptureResult {
    /// "10.0 s · −14.2 LUFS" (output loudness).
    pub fn label(&self) -> String {
        let lufs = self.output.loudness.integrated_lufs.map_or("—".into(), |v| format!("{v:.1}").replace('-', "−"));
        format!("{:.1} s · {lufs} LUFS", self.seconds)
    }

    /// The analysis as a markdown table (input vs output), for the chat.
    pub fn summary_markdown(&self) -> String {
        let (a, b) = (&self.input, &self.output);
        let mut md = format!("**Capture** · {:.1} s of signal\n\n| | before | after |\n|---|---|---|\n", self.seconds);
        let mut row = |name: &str, f: &dyn Fn(&Analysis) -> String| md.push_str(&format!("| {name} | {} | {} |\n", f(a), f(b)));
        row("integrated", &|x| format!("{} LUFS", fmt(x.loudness.integrated_lufs)));
        row("true peak", &|x| format!("{} dBTP", fmt(x.loudness.true_peak_dbtp)));
        // Derived, so they aren't in the JSON (see `analysis::Loudness`); the table still shows them.
        row("crest / PLR", &|x| format!("{} / {} dB", fmt(x.loudness.crest_factor_db()), fmt(x.loudness.plr_db())));
        row("loudness range", &|x| format!("{} LU", fmt(x.loudness.loudness_range_lu)));
        row("tilt", &|x| format!("{} dB/oct", fmt(x.spectrum.tilt_db_per_octave)));
        row("centroid", &|x| format!("{} Hz", fmt(x.spectrum.centroid_hz)));
        row("attack", &|x| format!("{} ms", fmt(x.dynamics.attack_time_ms)));
        row("stereo", &|x| if x.stereo.mono { "mono".into() } else { format!("corr {}", fmt(x.stereo.correlation)) });
        row("notes", &|x| match (&x.pitch.lowest_note, &x.pitch.highest_note) {
            (Some(l), Some(h)) => format!("{l}–{h}"),
            _ => "—".into(),
        });
        if let Some(dir) = &self.dir {
            md.push_str(&format!("\nsaved to `{}`", dir.display()));
        }
        md
    }
}

fn fmt(v: Option<f32>) -> String {
    v.map_or("—".into(), |v| format!("{v}"))
}

// Where captures live and how they are read back: `crate::captures`.
