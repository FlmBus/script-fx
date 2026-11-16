//! ScriptFX mock plugin: fixed mock DSP + 16 generic macro slots. A Lua preset
//! decides what the macros mean (and are called), which params they drive and
//! how the panel looks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, RwLock};

use include_dir::{Dir, include_dir};
use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use preset::{Control, Curve, MACRO_SLOTS, Preset};

mod editor;

/// Controls of the mock DSP a preset param can drive (`dsp = "..."` in Lua),
/// with the value used when no param drives them.
pub const DSP_TARGETS: [(&str, f32); 4] =
    [("drive", 0.0), ("tone", 20_000.0), ("mix", 100.0), ("output", 0.0)];
pub const DEFAULT_PRESET: &str = "tape-glow";
const PEAK_DECAY_MS: f32 = 300.0;
const DSP_SMOOTHING_MS: f32 = 15.0;

/// Default presets, compiled in so release builds work anywhere.
static BUNDLED_PRESETS: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../presets");

/// `$SCRIPTFX_PRESETS`, else `<user data dir>/ScriptFX/presets`
/// (Linux `~/.local/share`, macOS `~/Library/Application Support`, Windows `%APPDATA%`).
/// Bundled presets are copied there if missing; existing files are never overwritten.
pub fn presets_root() -> PathBuf {
    if let Some(p) = std::env::var_os("SCRIPTFX_PRESETS") {
        return PathBuf::from(p);
    }
    let root = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("ScriptFX")
        .join("presets");
    install_bundled_presets(&root);
    root
}

fn install_bundled_presets(root: &Path) {
    fn write_missing(dir: &Dir, root: &Path) {
        for f in dir.files() {
            let path = root.join(f.path());
            if !path.exists() {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&path, f.contents());
            }
        }
        for d in dir.dirs() {
            write_missing(d, root);
        }
    }
    write_missing(&BUNDLED_PRESETS, root);
}

/// Per DSP target: where its value comes from. Written by non-audio threads on
/// preset (re)load, read lock-free by the audio thread.
///
/// PoC caveat: fields are updated individually, so a preset switch can tear for
/// one block. The real engine should swap an immutable routing/graph snapshot.
struct DspRoute {
    /// 0 = unrouted (neutral value), 1 = macro, 2 = free param value.
    mode: AtomicU8,
    slot: AtomicU8,
    from: AtomicF32,
    to: AtomicF32,
    exp: AtomicU8,
    value: AtomicF32,
}

impl DspRoute {
    fn new() -> Self {
        Self {
            mode: AtomicU8::new(0),
            slot: AtomicU8::new(0),
            from: AtomicF32::new(0.0),
            to: AtomicF32::new(1.0),
            exp: AtomicU8::new(0),
            value: AtomicF32::new(0.0),
        }
    }
}

/// State shared between audio thread, editor and `initialize()`. Lock-free only.
pub struct Shared {
    pub meter_in: AtomicF32,
    pub meter_out: AtomicF32,
    routes: [DspRoute; 4],
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            meter_in: AtomicF32::new(util::MINUS_INFINITY_DB),
            meter_out: AtomicF32::new(util::MINUS_INFINITY_DB),
            routes: std::array::from_fn(|_| DspRoute::new()),
        }
    }
}

impl Shared {
    /// `free` = plain values of all params (only free ones are used).
    pub fn apply_routing(&self, preset: &Preset, free: &[f32]) {
        for (t, (name, _)) in DSP_TARGETS.iter().enumerate() {
            let route = &self.routes[t];
            let Some(i) = preset.params.iter().position(|p| p.dsp.as_deref() == Some(name)) else {
                route.mode.store(0, Ordering::Release);
                continue;
            };
            match preset.controls[i] {
                Control::Macro { slot, from, to, curve } => {
                    route.slot.store(slot as u8, Ordering::Relaxed);
                    route.from.store(from, Ordering::Relaxed);
                    route.to.store(to, Ordering::Relaxed);
                    route.exp.store((curve == Curve::Exp) as u8, Ordering::Relaxed);
                    route.mode.store(1, Ordering::Release);
                }
                Control::Free => {
                    route.value.store(free[i], Ordering::Relaxed);
                    route.mode.store(2, Ordering::Release);
                }
            }
        }
    }

    /// A free param changed (editor).
    pub fn set_free(&self, preset: &Preset, param: usize, plain: f32) {
        if let Some(dsp) = &preset.params[param].dsp {
            if let Some(t) = DSP_TARGETS.iter().position(|(n, _)| n == dsp) {
                self.routes[t].value.store(plain, Ordering::Relaxed);
            }
        }
    }

    fn target_value(&self, t: usize, macros: &[f32; MACRO_SLOTS]) -> f32 {
        let r = &self.routes[t];
        match r.mode.load(Ordering::Acquire) {
            1 => {
                let curve = if r.exp.load(Ordering::Relaxed) == 1 { Curve::Exp } else { Curve::Lin };
                let m = macros[r.slot.load(Ordering::Relaxed) as usize];
                curve.apply(r.from.load(Ordering::Relaxed), r.to.load(Ordering::Relaxed), m)
            }
            2 => r.value.load(Ordering::Relaxed),
            _ => DSP_TARGETS[t].1,
        }
    }
}

#[derive(Params)]
pub struct MacroParams {
    #[id = "macro"]
    pub value: FloatParam,
}

#[derive(Params)]
pub struct FxParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// Preset folder name, persisted with the host project. Empty means a
    /// fresh instance whose macros still need the preset's defaults.
    #[persist = "preset"]
    pub preset_name: RwLock<String>,

    /// Plain values of the preset's free (not macro-driven) params, by param id.
    #[persist = "param-values"]
    pub param_values: RwLock<HashMap<String, f32>>,

    /// Generic host-automatable slots. Hosts don't handle parameter lists that
    /// change at runtime, so presets map their macros onto these. Their names
    /// follow the preset via the patched nih-plug (`with_dynamic_name`).
    #[nested(array, group = "Macros")]
    pub macros: [MacroParams; MACRO_SLOTS],

    /// Host-visible names of the macro slots (empty = "Macro N").
    pub macro_names: [Arc<RwLock<String>>; MACRO_SLOTS],
}

impl Default for FxParams {
    fn default() -> Self {
        let macro_names: [Arc<RwLock<String>>; MACRO_SLOTS] = std::array::from_fn(|_| Arc::default());
        Self {
            editor_state: EguiState::from_size(980, 600),
            preset_name: RwLock::new(String::new()),
            param_values: RwLock::new(HashMap::new()),
            macros: std::array::from_fn(|i| MacroParams {
                value: FloatParam::new(format!("Macro {}", i + 1), 0.0, FloatRange::Linear {
                    min: 0.0,
                    max: 1.0,
                })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(Arc::new(|v| format!("{:.0}%", v * 100.0)))
                .with_dynamic_name(macro_names[i].clone()),
            }),
            macro_names,
        }
    }
}

impl FxParams {
    /// Plain values for all params of `preset`: persisted values for free
    /// params where available, defaults otherwise.
    pub fn free_values(&self, preset: &Preset) -> Vec<f32> {
        let stored = self.param_values.read().unwrap();
        preset
            .params
            .iter()
            .map(|p| stored.get(&p.id).copied().unwrap_or_else(|| p.default_plain()))
            .collect()
    }

    /// Sets the host-visible macro names. Caller must request a rescan.
    pub fn set_macro_names(&self, preset: &Preset) {
        for (i, name) in self.macro_names.iter().enumerate() {
            *name.write().unwrap() = preset.macros.get(i).map(|m| m.name.clone()).unwrap_or_default();
        }
    }
}

pub struct ScriptFx {
    params: Arc<FxParams>,
    shared: Arc<Shared>,
    sample_rate: f32,
    peak_decay: f32,
    smoothing: f32,
    targets: [f32; 4],
    lowpass: [f32; 2],
}

impl Default for ScriptFx {
    fn default() -> Self {
        Self {
            params: Arc::new(FxParams::default()),
            shared: Arc::new(Shared::default()),
            sample_rate: 48_000.0,
            peak_decay: 0.999,
            smoothing: 0.01,
            targets: DSP_TARGETS.map(|(_, v)| v),
            lowpass: [0.0; 2],
        }
    }
}

impl Plugin for ScriptFx {
    const NAME: &'static str = "ScriptFX PoC";
    const VENDOR: &'static str = "sandbox";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.shared.clone())
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        self.peak_decay = 0.25f32.powf((self.sample_rate * PEAK_DECAY_MS / 1000.0).recip());
        self.smoothing = 1.0 - (-1.0 / (DSP_SMOOTHING_MS / 1000.0 * self.sample_rate)).exp();
        // Not the audio thread: safe to load Lua here so routing and host names
        // are right even when the editor is never opened (e.g. state restore).
        let mut name = self.params.preset_name.read().unwrap().clone();
        if name.is_empty() {
            name = DEFAULT_PRESET.into();
        }
        match Preset::load(presets_root().join(&name)) {
            Ok(p) => {
                self.shared.apply_routing(&p, &self.params.free_values(&p));
                self.params.set_macro_names(&p);
                context.rescan_param_info();
            }
            Err(e) => nih_log!("preset '{name}' failed to load: {e}"),
        }
        true
    }

    fn reset(&mut self) {
        self.lowpass = [0.0; 2];
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let shared = &self.shared;
        let mut peak_in = shared.meter_in.load(Ordering::Relaxed);
        let mut peak_out = shared.meter_out.load(Ordering::Relaxed);

        for mut frame in buffer.iter_samples() {
            let macros: [f32; MACRO_SLOTS] =
                std::array::from_fn(|i| self.params.macros[i].value.smoothed.next());
            for t in 0..4 {
                let target = shared.target_value(t, &macros);
                self.targets[t] += self.smoothing * (target - self.targets[t]);
            }
            let [drive_db, tone_hz, mix_pct, out_db] = self.targets;
            let drive = util::db_to_gain(drive_db);
            let mix = (mix_pct / 100.0).clamp(0.0, 1.0);
            let out_gain = util::db_to_gain(out_db);
            let a = 1.0
                - (-std::f32::consts::TAU * tone_hz.clamp(20.0, self.sample_rate * 0.45)
                    / self.sample_rate)
                    .exp();

            for (ch, sample) in frame.iter_mut().enumerate() {
                let x = *sample;
                peak_in = peak_in.max(x.abs());
                let shaped = (x * drive).tanh() / drive.sqrt();
                let lp = &mut self.lowpass[ch.min(1)];
                *lp += a * (shaped - *lp);
                let y = (x * (1.0 - mix) + *lp * mix) * out_gain;
                peak_out = peak_out.max(y.abs());
                *sample = y;
            }
            peak_in *= self.peak_decay;
            peak_out *= self.peak_decay;
        }

        shared.meter_in.store(peak_in, Ordering::Relaxed);
        shared.meter_out.store(peak_out, Ordering::Relaxed);
        ProcessStatus::Normal
    }
}

impl ClapPlugin for ScriptFx {
    const CLAP_ID: &'static str = "dev.sandbox.scriptfx-poc";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Lua-scripted effect presets (PoC)");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Stereo];
}

impl Vst3Plugin for ScriptFx {
    const VST3_CLASS_ID: [u8; 16] = *b"ScriptFxPoC00001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Distortion];
}

nih_export_clap!(ScriptFx);
nih_export_vst3!(ScriptFx);
