//! Listen feature: capture → background analysis → result screen → prompt attachment.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use analysis::{Analysis, Frame};
use nih_plug_egui::egui::{self, Pos2, Rect, Stroke, vec2};

use crate::{LISTEN_ARMED, LISTEN_IDLE, LISTEN_RECORDING, Shared};

pub struct CaptureResult {
    pub seconds: f32,
    pub input: Analysis,
    pub output: Analysis,
    /// Everything that goes to the LLM.
    pub json: String,
    /// Where JSON + WAVs were saved.
    pub dir: Option<PathBuf>,
}

pub struct Listen {
    seconds: f32,
    pending: Option<Arc<Mutex<Option<CaptureResult>>>>,
    shown: Option<Arc<CaptureResult>>,
    /// Attached to the prompt (sent with the next generation).
    pub attached: Option<Arc<CaptureResult>>,
}

impl Listen {
    pub fn new() -> Self {
        Self { seconds: 10.0, pending: None, shown: None, attached: None }
    }

    /// While true, the listen screen replaces the preset panel.
    pub fn overlay_active(&self, shared: &Shared) -> bool {
        shared.listen_state.load(Ordering::Relaxed) != LISTEN_IDLE || self.pending.is_some() || self.shown.is_some()
    }

    /// Picks up finished captures and analyses. Call once per frame.
    pub fn poll(&mut self, shared: &Shared) {
        if let Some((input, output)) = shared.take_capture() {
            let sr = shared.sample_rate();
            let slot = Arc::new(Mutex::new(None));
            self.pending = Some(slot.clone());
            std::thread::spawn(move || {
                let seconds = input.len() as f32 / sr;
                let a_in = analysis::analyze(&input, sr);
                let a_out = analysis::analyze(&output, sr);
                let json = serde_json::to_string_pretty(&serde_json::json!({
                    "capture": {
                        "seconds_of_signal": (seconds * 10.0).round() / 10.0,
                        "note": "input = the track before the ScriptFX chain, output = after it (current preset)",
                    },
                    "input": a_in,
                    "output": a_out,
                }))
                .unwrap_or_default();
                let dir = save(&input, &output, &json, sr).map_err(|e| nih_plug::nih_log!("saving capture failed: {e}")).ok();
                *slot.lock().unwrap() = Some(CaptureResult { seconds, input: a_in, output: a_out, json, dir });
            });
        }
        let finished = self.pending.as_ref().and_then(|slot| slot.lock().unwrap().take());
        if let Some(result) = finished {
            self.shown = Some(Arc::new(result));
            self.pending = None;
        }
    }

    /// Listen button + duration, for the prompt row.
    pub fn button(&mut self, ui: &mut egui::Ui, shared: &Shared) {
        let busy = shared.listen_state.load(Ordering::Relaxed) != LISTEN_IDLE || self.pending.is_some();
        egui::ComboBox::from_id_salt("listen-seconds")
            .selected_text(format!("{:.0} s", self.seconds))
            .width(56.0)
            .show_ui(ui, |ui| {
                for s in [10.0, 20.0, 30.0] {
                    ui.selectable_value(&mut self.seconds, s, format!("{s:.0} s"));
                }
            });
        if ui.add_enabled(!busy, egui::Button::new("🎧 Listen")).clicked() {
            self.shown = None;
            shared.start_listen(self.seconds);
        }
    }

    /// Chip for an attached capture (above the prompt).
    pub fn chip(&mut self, ui: &mut egui::Ui) {
        let Some(a) = self.attached.clone() else { return };
        ui.horizontal(|ui| {
            let text = format!(
                "🎧 capture · {:.1} s · in {} / out {} LUFS",
                a.seconds,
                fmt(a.input.loudness.integrated_lufs),
                fmt(a.output.loudness.integrated_lufs)
            );
            if ui.button(text).on_hover_text("show analysis").clicked() {
                self.shown = Some(a.clone());
            }
            if ui.small_button("✖").on_hover_text("detach").clicked() {
                self.attached = None;
            }
        });
    }

    pub fn overlay(&mut self, ui: &mut egui::Ui, shared: &Shared, meter_in: f32) {
        let state = shared.listen_state.load(Ordering::Relaxed);
        let accent = ui.visuals().selection.bg_fill;
        if state == LISTEN_ARMED || state == LISTEN_RECORDING {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.25);
                if state == LISTEN_ARMED {
                    ui.heading("Play your track");
                    ui.label("Listening starts with the first signal. Silent parts are skipped.");
                } else {
                    ui.heading("Listening…");
                    let sr = shared.sample_rate();
                    let target = shared.capture.try_lock().map(|c| c.target).unwrap_or(1).max(1);
                    let got = shared.listen_progress.load(Ordering::Relaxed);
                    ui.add_space(8.0);
                    ui.add(
                        egui::ProgressBar::new(got as f32 / target as f32)
                            .desired_width(320.0)
                            .text(format!("{:.1} / {:.0} s", got as f32 / sr, target as f32 / sr)),
                    );
                }
                ui.add_space(12.0);
                ui.add(egui::ProgressBar::new(meter_in).desired_width(320.0).fill(accent).text("input level"));
                ui.add_space(16.0);
                if ui.button("Cancel").clicked() {
                    shared.cancel_listen();
                }
            });
            return;
        }
        if self.pending.is_some() {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.35);
                ui.heading("Analyzing…");
                ui.add(egui::Spinner::new().size(28.0));
            });
            return;
        }
        let Some(result) = self.shown.clone() else { return };
        self.result_view(ui, &result);
    }

    fn result_view(&mut self, ui: &mut egui::Ui, r: &Arc<CaptureResult>) {
        ui.horizontal(|ui| {
            ui.heading(format!("Capture · {:.1} s of signal", r.seconds));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    self.shown = None;
                }
                let attached = self.attached.as_ref().is_some_and(|a| Arc::ptr_eq(a, r));
                if !attached && ui.button("📎 Attach to prompt").clicked() {
                    self.attached = Some(r.clone());
                    self.shown = None;
                }
            });
        });
        if let Some(dir) = &r.dir {
            ui.weak(format!("saved: {}", dir.display()));
        }
        ui.add_space(4.0);

        let (a, b) = (&r.input, &r.output);
        egui::ScrollArea::vertical().show(ui, |ui| {
            spectrum_plot(ui, a, b);
            ui.add_space(6.0);
            egui::Grid::new("analysis").striped(true).num_columns(3).spacing(vec2(24.0, 4.0)).show(ui, |ui| {
                ui.strong("");
                ui.strong("input (before chain)");
                ui.strong("output (after chain)");
                ui.end_row();
                let mut row = |name: &str, f: &dyn Fn(&Analysis) -> String| {
                    ui.label(name);
                    ui.label(f(a));
                    ui.label(f(b));
                    ui.end_row();
                };
                row("integrated", &|x| format!("{} LUFS", fmt(x.loudness.integrated_lufs)));
                row("short-term max", &|x| format!("{} LUFS", fmt(x.loudness.short_term_max_lufs)));
                row("true peak", &|x| format!("{} dBTP", fmt(x.loudness.true_peak_dbtp)));
                row("RMS / K-20", &|x| format!("{} dBFS / {} dB", fmt(x.loudness.rms_dbfs), fmt(x.loudness.k20_db)));
                row("crest factor / PLR", &|x| format!("{} / {} dB", fmt(x.loudness.crest_factor_db), fmt(x.loudness.plr_db)));
                row("loudness range", &|x| format!("{} LU", fmt(x.loudness.loudness_range_lu)));
                row("level spread (p95−p10)", &|x| format!("{} dB", fmt(x.dynamics.level_spread_db)));
                row("band spread low/mid/high", &|x| {
                    format!(
                        "{} / {} / {} dB",
                        fmt(x.dynamics.low_below_250hz.spread_db),
                        fmt(x.dynamics.mid_250_2khz.spread_db),
                        fmt(x.dynamics.high_above_2khz.spread_db)
                    )
                });
                row("onsets · attack", &|x| format!("{:.1}/s · {} ms", x.dynamics.onsets_per_second, fmt(x.dynamics.attack_time_ms)));
                row("tilt · centroid", &|x| {
                    format!("{} dB/oct · {} Hz", fmt(x.spectrum.tilt_db_per_octave), fmt(x.spectrum.centroid_hz))
                });
                row("energy sub/low/lowmid/mid/pres/air", &|x| {
                    let s = &x.spectrum.energy_share_percent;
                    format!(
                        "{:.0}/{:.0}/{:.0}/{:.0}/{:.0}/{:.0} %",
                        s.sub_20_60hz, s.low_60_250hz, s.low_mid_250_500hz, s.mid_500_2khz, s.presence_2_6khz, s.air_6_20khz
                    )
                });
                row("resonances", &|x| {
                    let v: Vec<String> =
                        x.spectrum.resonances.iter().take(3).map(|r| format!("{:.0} Hz +{:.1}", r.hz, r.prominence_db)).collect();
                    if v.is_empty() { "—".into() } else { v.join(", ") }
                });
                row("stereo corr · side/mid", &|x| {
                    if x.stereo.mono {
                        "mono".into()
                    } else {
                        format!("{} · {} dB", fmt(x.stereo.correlation), fmt(x.stereo.side_to_mid_db))
                    }
                });
                row("notes", &|x| match (&x.pitch.lowest_note, &x.pitch.highest_note) {
                    (Some(l), Some(h)) => format!("{l}–{h} ({:.0} % pitched)", x.pitch.pitched_percent),
                    _ => "no clear pitch".into(),
                });
            });
            ui.add_space(8.0);
            egui::CollapsingHeader::new(format!("JSON sent to the LLM ({} chars)", r.json.len())).show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&r.json).monospace().size(10.0)).wrap());
            });
        });
    }
}

fn fmt(v: Option<f32>) -> String {
    v.map_or("—".into(), |v| format!("{v}"))
}

/// 1/3-octave bars (input) with the output as a line on top.
fn spectrum_plot(ui: &mut egui::Ui, a: &Analysis, b: &Analysis) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(720.0), 130.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let vis = ui.visuals();
    p.rect_filled(rect, 4.0, vis.extreme_bg_color);
    let bands_a = &a.spectrum.third_octave;
    let bands_b = &b.spectrum.third_octave;
    let top = bands_a
        .iter()
        .chain(bands_b)
        .filter_map(|x| x.db)
        .fold(f32::NEG_INFINITY, f32::max)
        .max(-120.0)
        + 3.0;
    let range = 60.0;
    let n = bands_a.len().max(1);
    let w = rect.width() / n as f32;
    let y = |db: Option<f32>| {
        let t = ((top - db.unwrap_or(-200.0)) / range).clamp(0.0, 1.0);
        rect.top() + t * rect.height()
    };
    let dim = vis.weak_text_color().gamma_multiply(0.6);
    for (i, band) in bands_a.iter().enumerate() {
        let x0 = rect.left() + i as f32 * w;
        p.rect_filled(Rect::from_min_max(Pos2::new(x0 + 1.0, y(band.db)), Pos2::new(x0 + w - 1.0, rect.bottom())), 1.0, dim);
    }
    let line: Vec<Pos2> =
        bands_b.iter().enumerate().map(|(i, band)| Pos2::new(rect.left() + (i as f32 + 0.5) * w, y(band.db))).collect();
    p.add(egui::Shape::line(line, Stroke::new(2.0, vis.selection.bg_fill)));
    for (i, band) in bands_a.iter().enumerate() {
        if [31.5f32, 125.0, 500.0, 2000.0, 8000.0].iter().any(|f| (band.hz - f).abs() / f < 0.05) {
            let label = if band.hz >= 1000.0 { format!("{}k", band.hz / 1000.0) } else { format!("{}", band.hz) };
            p.text(
                Pos2::new(rect.left() + (i as f32 + 0.5) * w, rect.bottom() - 2.0),
                egui::Align2::CENTER_BOTTOM,
                label,
                egui::FontId::proportional(10.0),
                vis.text_color(),
            );
        }
    }
    p.text(
        rect.left_top() + vec2(6.0, 4.0),
        egui::Align2::LEFT_TOP,
        "1/3-octave: bars = input, line = output",
        egui::FontId::proportional(10.0),
        vis.weak_text_color(),
    );
}

/// `<data dir>/ScriptFX/captures/capture-<unix time>/{input,output}.wav + analysis.json`
fn save(input: &[Frame], output: &[Frame], json: &str, sr: f32) -> std::io::Result<PathBuf> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("ScriptFX")
        .join("captures")
        .join(format!("capture-{stamp}"));
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("analysis.json"), json)?;
    let spec = hound::WavSpec { channels: 2, sample_rate: sr as u32, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    for (name, frames) in [("input.wav", input), ("output.wav", output)] {
        let mut w = hound::WavWriter::create(dir.join(name), spec).map_err(std::io::Error::other)?;
        for f in frames {
            w.write_sample(f[0]).map_err(std::io::Error::other)?;
            w.write_sample(f[1]).map_err(std::io::Error::other)?;
        }
        w.finalize().map_err(std::io::Error::other)?;
    }
    Ok(dir)
}
