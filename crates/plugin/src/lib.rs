//! ScriptFX PoC plugin: 16 generic macro slots + a native DSP graph compiled from
//! the patch's declarative `chain`. A Lua patch decides what the macros mean (and
//! are called), which params they drive, the chain and how the panel looks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};

use dsp::{Atomics, Frame, Graph, ModAtomics, SUB_BLOCK};
use include_dir::{Dir, include_dir};
use nice_plug::prelude::*;
use nice_plug_iced::IcedEditorState;
use nice_plug_iced::iced::PollSubNotifier;
use patch::{MACRO_SLOTS, ModEdits, ModSettings, Patch};

mod assistant;
mod captures;
mod diag;
mod listen;
mod shell;
mod user_settings;

/// Listen/capture states (`Shared::listen_state`).
pub const LISTEN_IDLE: u8 = 0;
/// Waiting for the first signal above [`ARM_THRESHOLD`].
pub const LISTEN_ARMED: u8 = 1;
pub const LISTEN_RECORDING: u8 = 2;
pub const LISTEN_DONE: u8 = 3;
const ARM_THRESHOLD: f32 = 0.003_16; // -50 dBFS
/// Blocks quieter than this aren't recorded (skips gaps between takes/sections).
const SIGNAL_THRESHOLD: f32 = 0.001; // -60 dBFS
pub const MAX_CAPTURE_SECONDS: f32 = 30.0;

/// Pre-allocated capture of the chain's input and output.
pub struct CaptureBuf {
    pub input: Vec<Frame>,
    pub output: Vec<Frame>,
    pub len: usize,
    pub target: usize,
}

/// Editor window size (logical px), fixed.
/// 836 × 714: the patch window is exactly the 840×560 panel's 3:2 at 816×544 (see
/// `theme::stage_size`), the mod dock (108) below it. The library floats over it as a sheet, so
/// it takes no column; the notch hangs into it, so it takes no row.
pub const WINDOW_SIZE: (f32, f32) = (836.0, 714.0);
const PEAK_DECAY_MS: f32 = 300.0;
const GRAPH_QUEUE: usize = 16;

/// Factory patches, compiled in so release builds work anywhere.
static BUNDLED_PATCHES: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../patches");
const FACTORY_MANIFEST: &str = ".factory-manifest";

/// `$SCRIPTFX_PATCHES`, else `<user data dir>/ScriptFX/patches`
/// (Linux `~/.local/share`, macOS `~/Library/Application Support`, Windows `%APPDATA%`).
/// Factory patches are installed/updated there (see [`install_factory_patches`]).
/// Resolved once per process.
pub fn patches_root() -> PathBuf {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        if let Some(p) = std::env::var_os("SCRIPTFX_PATCHES") {
            return PathBuf::from(p);
        }
        let root = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("ScriptFX")
            .join("patches");
        install_factory_patches(&root);
        root
    })
    .clone()
}

/// FNV-1a, stable across builds (unlike `DefaultHasher`).
fn fnv(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Writes missing factory files and updates factory files the user hasn't changed.
/// `.factory-manifest` remembers the hash of what was installed last time; a file
/// whose current hash differs from that was edited by the user and is left alone.
fn install_factory_patches(root: &Path) {
    let manifest_path = root.join(FACTORY_MANIFEST);
    let installed: HashMap<String, u64> = std::fs::read_to_string(&manifest_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (h, p) = l.split_once(' ')?;
            Some((p.to_string(), u64::from_str_radix(h, 16).ok()?))
        })
        .collect();
    let mut manifest = String::new();

    fn files<'a>(dir: &'a Dir<'a>, out: &mut Vec<&'a include_dir::File<'a>>) {
        out.extend(dir.files());
        for d in dir.dirs() {
            files(d, out);
        }
    }
    let mut all = Vec::new();
    files(&BUNDLED_PATCHES, &mut all);

    for f in all {
        let rel = f.path().to_string_lossy().replace('\\', "/");
        let path = root.join(f.path());
        let factory_hash = fnv(f.contents());
        let current = std::fs::read(&path).ok().map(|c| fnv(&c));
        let user_modified = match (current, installed.get(&rel)) {
            (None, _) => false,
            (Some(cur), Some(prev)) => cur != *prev && cur != factory_hash,
            // Existing file without a manifest entry: only replace if identical.
            (Some(cur), None) => cur != factory_hash,
        };
        if user_modified {
            nice_log!("keeping user-modified patch file {rel}");
            if let Some(prev) = installed.get(&rel) {
                manifest.push_str(&format!("{prev:016x} {rel}\n"));
            }
            continue;
        }
        if current != Some(factory_hash) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, f.contents());
        }
        manifest.push_str(&format!("{factory_hash:016x} {rel}\n"));
    }
    // Factory files that were removed from the bundle: delete them unless the user changed them.
    let bundled: std::collections::HashSet<String> =
        manifest.lines().filter_map(|l| l.split_once(' ').map(|(_, p)| p.to_string())).collect();
    for (rel, hash) in &installed {
        if bundled.contains(rel) {
            continue;
        }
        let path = root.join(rel);
        if std::fs::read(&path).ok().is_some_and(|c| fnv(&c) == *hash) {
            nice_log!("removing old factory file {rel}");
            let _ = std::fs::remove_file(&path);
        }
    }
    // Lua 5.4 leftovers from before the Luau port (not in any manifest): a `x.lua` next to
    // `x.luau` makes `require("./x")` ambiguous. Keep them, renamed.
    for dir in BUNDLED_PATCHES.dirs() {
        let Ok(entries) = std::fs::read_dir(root.join(dir.path())) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_some_and(|x| x == "lua") && path.with_extension("luau").exists() {
                nice_log!("renaming leftover {} to .lua.bak", path.display());
                let _ = std::fs::rename(&path, path.with_extension("lua.bak"));
            }
        }
    }

    let _ = std::fs::create_dir_all(root);
    let _ = std::fs::write(manifest_path, manifest);
}

/// Arrays of the graph currently playing, for the editor.
struct Live {
    free: Atomics,
    meters: Atomics,
    mods: ModAtomics,
    assignments: ModAtomics,
    mod_out: Atomics,
    mod_phase: Atomics,
}

/// State shared between audio thread, editor and `activate()`.
pub struct Shared {
    pub meter_in: AtomicF32,
    pub meter_out: AtomicF32,
    sample_rate: AtomicF32,
    /// The host's tempo, BPM (for the displays' time axes).
    pub tempo: AtomicF32,
    /// New graphs → audio thread.
    graph_tx: Mutex<rtrb::Producer<Box<Graph>>>,
    /// Replaced graphs ← audio thread (dropped here, never on the audio thread).
    garbage_rx: Mutex<rtrb::Consumer<Box<Graph>>>,
    live: Mutex<Option<Live>>,
    pub listen_state: std::sync::atomic::AtomicU8,
    pub listen_progress: std::sync::atomic::AtomicUsize,
    /// Audio thread only ever `try_lock`s this (never blocks).
    pub capture: Mutex<CaptureBuf>,
    /// The chat and its LLM worker. The audio thread never touches it.
    pub assistant: Mutex<assistant::Assistant>,
}

impl Shared {
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    /// Arms a capture of `seconds` of signal (input + output of the chain).
    pub fn start_listen(&self, seconds: f32) {
        let mut cap = self.capture.lock().unwrap();
        cap.len = 0;
        cap.target = ((seconds * self.sample_rate()) as usize).min(cap.input.len());
        self.listen_progress.store(0, Ordering::Relaxed);
        self.listen_state.store(LISTEN_ARMED, Ordering::Release);
    }

    pub fn cancel_listen(&self) {
        self.listen_state.store(LISTEN_IDLE, Ordering::Release);
    }

    /// Ends a running capture early, keeping what was recorded.
    pub fn finish_listen(&self) {
        let mut cap = self.capture.lock().unwrap();
        if cap.len > 0 {
            // A block the audio thread is just about to record finds the target reached.
            cap.target = cap.len;
            self.recording_done();
        }
    }

    /// Only from recording: a stop and the audio thread reaching the target can't finish the
    /// same capture twice.
    fn recording_done(&self) {
        let _ = self.listen_state.compare_exchange(LISTEN_RECORDING, LISTEN_DONE, Ordering::AcqRel, Ordering::Relaxed);
    }

    /// Takes the finished capture (input, output) and returns to idle.
    pub fn take_capture(&self) -> Option<(Vec<Frame>, Vec<Frame>)> {
        if self.listen_state.load(Ordering::Acquire) != LISTEN_DONE {
            return None;
        }
        let cap = self.capture.lock().unwrap();
        let out = (cap.input[..cap.len].to_vec(), cap.output[..cap.len].to_vec());
        self.listen_state.store(LISTEN_IDLE, Ordering::Release);
        Some(out)
    }

    /// Called from `activate()` (not the audio thread).
    fn allocate_capture(&self, sample_rate: f32) {
        let n = (MAX_CAPTURE_SECONDS * sample_rate) as usize;
        let mut cap = self.capture.lock().unwrap();
        if cap.input.len() != n {
            *cap = CaptureBuf { input: vec![[0.0; 2]; n], output: vec![[0.0; 2]; n], len: 0, target: 0 };
        }
    }

    /// Audio thread: records one sub-block if listening. Never blocks or allocates.
    fn capture_block(&self, input: &[Frame], output: &[Frame]) {
        let mut state = self.listen_state.load(Ordering::Acquire);
        if state != LISTEN_ARMED && state != LISTEN_RECORDING {
            return;
        }
        let peak = input.iter().fold(0.0f32, |p, f| p.max(f[0].abs()).max(f[1].abs()));
        if state == LISTEN_ARMED && peak > ARM_THRESHOLD {
            state = LISTEN_RECORDING;
            self.listen_state.store(state, Ordering::Release);
        }
        if state != LISTEN_RECORDING || peak < SIGNAL_THRESHOLD {
            return;
        }
        let Ok(mut cap) = self.capture.try_lock() else { return };
        let start = cap.len;
        let k = input.len().min(cap.target.saturating_sub(start));
        cap.input[start..start + k].copy_from_slice(&input[..k]);
        cap.output[start..start + k].copy_from_slice(&output[..k]);
        cap.len += k;
        self.listen_progress.store(cap.len, Ordering::Relaxed);
        if cap.len >= cap.target {
            self.recording_done();
        }
    }
    /// Compiles the patch's chain and hands it to the audio thread. Non-audio threads only.
    /// `Patch::empty()` = no patch (passthrough).
    pub fn publish(&self, patch: &Patch, free: &[f32]) {
        let graph = Box::new(Graph::build(patch, self.sample_rate.load(Ordering::Relaxed), free));
        let mut live = self.live.lock().unwrap();
        // The editor reads the new graph's outputs from now on, but it only plays from the next
        // audio buffer: start them at the current values, so scopes and meters don't dip to 0.
        if let Some(old) = &*live {
            for (new, old) in [(graph.modulator_values(), &old.mod_out), (graph.modulator_phases(), &old.mod_phase), (graph.meters(), &old.meters)] {
                if new.len() == old.len() {
                    for (n, o) in new.iter().zip(old.iter()) {
                        n.store(o.load(Ordering::Relaxed), Ordering::Relaxed);
                    }
                }
            }
        }
        *live = Some(Live {
            free: graph.free_values(),
            meters: graph.meters(),
            mods: graph.mod_settings(),
            assignments: graph.assignment_settings(),
            mod_out: graph.modulator_values(),
            mod_phase: graph.modulator_phases(),
        });
        drop(live);
        while self.garbage_rx.lock().unwrap().pop().is_ok() {}
        if self.graph_tx.lock().unwrap().push(graph).is_err() {
            nice_log!("graph queue full, dropping update");
        }
    }

    /// A free param changed (editor): lock-free write into the playing graph.
    pub fn set_free(&self, param: usize, plain: f32) {
        if let Some(live) = &*self.live.lock().unwrap() {
            if let Some(v) = live.free.get(param) {
                v.store(plain, Ordering::Relaxed);
            }
        }
    }

    /// A modulation's settings changed (meta knob): lock-free write into the playing graph.
    /// `index`: [`Patch::mod_index`].
    pub fn set_mod(&self, index: usize, settings: ModSettings) {
        if let Some(live) = &*self.live.lock().unwrap() {
            if let Some(m) = live.mods.get(index) {
                m.store(settings.pack(), Ordering::Relaxed);
            }
        }
    }

    /// A modulator assignment's settings changed: lock-free write into the playing graph.
    pub fn set_assignment(&self, index: usize, settings: ModSettings) {
        if let Some(live) = &*self.live.lock().unwrap() {
            if let Some(a) = live.assignments.get(index) {
                a.store(settings.pack(), Ordering::Relaxed);
            }
        }
    }

    /// Live unipolar `[0, 1]` outputs of Follow, LFO 1–3, Step, and their cycle positions
    /// (`Graph::modulator_phases`).
    pub fn modulator_values(&self) -> ([f32; 5], [f32; 5]) {
        let (mut out, mut phase) = ([0.0; 5], [0.0; 5]);
        if let Some(live) = &*self.live.lock().unwrap() {
            for (dst, src) in out.iter_mut().zip(live.mod_out.iter()) {
                *dst = src.load(Ordering::Relaxed);
            }
            for (dst, src) in phase.iter_mut().zip(live.mod_phase.iter()) {
                *dst = src.load(Ordering::Relaxed);
            }
        }
        (out, phase)
    }

    /// Current block meter values (index = `Patch::meters`).
    pub fn block_meters(&self, out: &mut Vec<f32>) {
        out.clear();
        if let Some(live) = &*self.live.lock().unwrap() {
            out.extend(live.meters.iter().map(|m| m.load(Ordering::Relaxed)));
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
    /// Patch folder name, persisted with the host project. Empty = no patch
    /// (fresh instance or unloaded): the signal passes through.
    #[persist = "patch"]
    pub patch_name: RwLock<String>,

    /// Plain values of the patch's free (not macro-driven) params, by param id.
    #[persist = "param-values"]
    pub param_values: RwLock<HashMap<String, f32>>,

    /// The modulation slots' settings (amount, uni/bi, curve, bypass: the meta knobs), by
    /// [`Patch::mod_key`]. Missing = what the patch defines.
    #[persist = "mod-slots"]
    pub mod_settings: RwLock<HashMap<String, ModSettings>>,

    /// Routes the user added or removed and modulator settings they changed, against the patch
    /// file (`patch::ModEdits`). Empty = the patch as written.
    #[persist = "mod-edits"]
    pub mod_edits: RwLock<ModEdits>,

    /// Passive mode: this instance only listens. No patch is loaded (passthrough, the Lua
    /// runtime and the cached layers are freed) and the controls are disabled. Per instance,
    /// so it belongs to the project, not to the user's settings.
    #[persist = "passive"]
    pub passive: RwLock<bool>,

    /// Generic host-automatable slots. Hosts don't handle parameter lists that
    /// change at runtime, so patches map their macros onto these. Their names
    /// follow the patch via the patched nih-plug (`with_dynamic_name`).
    #[nested(array, group = "Macros")]
    pub macros: [MacroParams; MACRO_SLOTS],

    /// Host-visible names of the macro slots (empty = "Macro N").
    pub macro_names: [Arc<RwLock<String>>; MACRO_SLOTS],

    /// The host's bypass (and the notch's button): the input passes through, delayed by the
    /// chain's latency so it stays aligned; switching crossfades (`BYPASS_FADE_MS`).
    #[id = "bypass"]
    pub bypass: BoolParam,
}

impl Default for FxParams {
    fn default() -> Self {
        let macro_names: [Arc<RwLock<String>>; MACRO_SLOTS] = std::array::from_fn(|_| Arc::default());
        Self {
            patch_name: RwLock::new(String::new()),
            param_values: RwLock::new(HashMap::new()),
            mod_settings: RwLock::new(HashMap::new()),
            mod_edits: RwLock::new(ModEdits::default()),
            passive: RwLock::new(false),
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
            bypass: BoolParam::new("Bypass", false).make_bypass(),
        }
    }
}

impl FxParams {
    /// Plain values for all params of `patch`: persisted values for free
    /// params where available, defaults otherwise.
    pub fn free_values(&self, patch: &Patch) -> Vec<f32> {
        let stored = self.param_values.read().unwrap();
        patch
            .params
            .iter()
            .map(|p| stored.get(&p.id).copied().unwrap_or_else(|| p.default_plain()))
            .collect()
    }

    /// Puts the project's modulation into a freshly loaded patch (before building its graph):
    /// the user's route and modulator edits, then the saved meta-knob settings.
    pub fn apply_mod_settings(&self, patch: &mut Patch) {
        self.mod_edits.read().unwrap().apply(patch);
        let stored = self.mod_settings.read().unwrap();
        for i in 0..patch.modulations.len() {
            for k in 0..patch.modulations[i].len() {
                if let Some(s) = stored.get(&patch.mod_key(i, k)) {
                    patch.modulations[i][k].settings = *s;
                }
            }
        }
    }

    /// Sets the host-visible macro names. Caller must request a rescan.
    pub fn set_macro_names(&self, patch: &Patch) {
        for (i, name) in self.macro_names.iter().enumerate() {
            *name.write().unwrap() = patch.macros.get(i).map(|m| m.name.clone()).unwrap_or_default();
        }
    }
}

pub struct ScriptFx {
    params: Arc<FxParams>,
    shared: Arc<Shared>,
    editor_state: Arc<IcedEditorState>,
    /// Wakes the editor (unused for now: the shell runs its own frame clock).
    notifier: PollSubNotifier,
    graph: Option<Box<Graph>>,
    graph_rx: rtrb::Consumer<Box<Graph>>,
    garbage_tx: rtrb::Producer<Box<Graph>>,
    peak_decay: f32,
    /// Dev aid: `SCRIPTFX_TEST_INPUT=file.wav` replaces the input with this (looped).
    test_input: Option<(Vec<Frame>, usize)>,
    /// Bypass: the input's recent frames (a ring, `BYPASS_DELAY` long), to delay the dry signal by
    /// the chain's latency; where it writes next; how wet the output is now (1 = the chain).
    dry: Vec<Frame>,
    dry_pos: usize,
    wet: f32,
}

/// The longest latency the bypass can line up with (frames; ≈ 1.4 s at 192 kHz).
const BYPASS_DELAY: usize = 1 << 18;
/// The crossfade when bypass switches.
const BYPASS_FADE_MS: f32 = 10.0;

impl Default for ScriptFx {
    fn default() -> Self {
        let (graph_tx, graph_rx) = rtrb::RingBuffer::new(GRAPH_QUEUE);
        let (garbage_tx, garbage_rx) = rtrb::RingBuffer::new(GRAPH_QUEUE);
        Self {
            params: Arc::new(FxParams::default()),
            editor_state: IcedEditorState::from_size(
                nice_plug::editor::dpi::LogicalSize::new(WINDOW_SIZE.0, WINDOW_SIZE.1),
                // The user's UI scale opens the window at that size; the shell changes it live.
                user_settings::UserSettings::load().ui_scale,
            ),
            notifier: PollSubNotifier::new(),
            shared: Arc::new(Shared {
                tempo: AtomicF32::new(120.0),
                meter_in: AtomicF32::new(0.0),
                meter_out: AtomicF32::new(0.0),
                sample_rate: AtomicF32::new(48_000.0),
                graph_tx: Mutex::new(graph_tx),
                garbage_rx: Mutex::new(garbage_rx),
                live: Mutex::new(None),
                listen_state: std::sync::atomic::AtomicU8::new(LISTEN_IDLE),
                listen_progress: std::sync::atomic::AtomicUsize::new(0),
                capture: Mutex::new(CaptureBuf { input: Vec::new(), output: Vec::new(), len: 0, target: 0 }),
                assistant: Mutex::default(),
            }),
            graph: None,
            graph_rx,
            garbage_tx,
            peak_decay: 0.999,
            test_input: None,
            dry: vec![[0.0; 2]; BYPASS_DELAY],
            dry_pos: 0,
            wet: 1.0,
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

    type Editor = nice_plug_iced::IcedEditor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    /// Diagnostics builds log to a file (`diag.rs`); otherwise nice-plug's default.
    fn setup_logger() -> Option<bool> {
        diag::setup_logger()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        shell::create(self.editor_state.clone(), self.notifier.clone(), self.params.clone(), self.shared.clone())
    }

    fn activate(
        &mut self,
        layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl ActivateContext<Self>,
    ) -> bool {
        let sr = buffer_config.sample_rate;
        nice_log!(
            "activate: {sr} Hz, buffers {}..{} samples, {:?} in / {:?} out, mode {:?}",
            buffer_config.min_buffer_size.unwrap_or(0),
            buffer_config.max_buffer_size,
            layout.main_input_channels,
            layout.main_output_channels,
            buffer_config.process_mode
        );
        self.shared.sample_rate.store(sr, Ordering::Relaxed);
        self.shared.allocate_capture(sr);
        if let Some(path) = std::env::var_os("SCRIPTFX_TEST_INPUT") {
            match hound::WavReader::open(&path) {
                Ok(mut r) if r.spec().sample_format == hound::SampleFormat::Float => {
                    let ch = r.spec().channels as usize;
                    let s: Vec<f32> = r.samples::<f32>().filter_map(Result::ok).collect();
                    let frames: Vec<Frame> = s.chunks(ch).map(|c| [c[0], c[ch.min(2) - 1]]).collect();
                    nice_log!("test input: {} frames from {:?}", frames.len(), path);
                    self.test_input = Some((frames, 0));
                }
                _ => nice_log!("SCRIPTFX_TEST_INPUT: need a float WAV at the host sample rate"),
            }
        }
        self.peak_decay = 0.25f32.powf((sr * PEAK_DECAY_MS / 1000.0).recip());
        // Not the audio thread: safe to load Lua and build the graph here, so audio
        // and host names are right even when the editor is never opened.
        while self.graph_rx.pop().is_ok() {} // drop graphs built for an old sample rate
        let name = self.params.patch_name.read().unwrap().clone();
        if name.is_empty() {
            // No patch: passthrough, no latency, default slot names.
            self.graph = None;
            *self.shared.live.lock().unwrap() = None;
            context.set_latency_samples(0);
            self.params.set_macro_names(&Patch::empty());
            context.rescan_param_info();
            return true;
        }
        match Patch::load(patches_root().join(&name)) {
            Ok(mut p) => {
                self.params.apply_mod_settings(&mut p);
                let graph = Box::new(Graph::build(&p, sr, &self.params.free_values(&p)));
                // Oversampled blocks delay the signal; the host compensates.
                context.set_latency_samples(graph.latency() as u32);
                *self.shared.live.lock().unwrap() = Some(Live {
                    free: graph.free_values(),
                    meters: graph.meters(),
                    mods: graph.mod_settings(),
                    assignments: graph.assignment_settings(),
                    mod_out: graph.modulator_values(),
                    mod_phase: graph.modulator_phases(),
                });
                self.graph = Some(graph);
                self.params.set_macro_names(&p);
                context.rescan_param_info();
            }
            Err(e) => nice_log!("patch '{name}' failed to load: {e}"),
        }
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let started = std::time::Instant::now();
        let mut finite = true;
        // Swap in newly published graphs; old ones go back to be dropped elsewhere.
        while let Ok(next) = self.graph_rx.pop() {
            // Lock-free; nih-plug only tells the host if the latency changed.
            context.set_latency_samples(next.latency() as u32);
            let mut next = next;
            if let Some(old) = &mut self.graph {
                // Same chain (a routing or modulator change): keep tails and LFO phases.
                next.adopt(old);
            }
            if let Some(old) = self.graph.replace(next) {
                if let Err(rtrb::PushError::Full(old)) = self.garbage_tx.push(old) {
                    std::mem::forget(old); // never free on the audio thread
                }
            }
        }

        // Synced blocks (`sync = "1/8"`) follow the host tempo; synced LFOs lock to
        // the beat position while the host plays.
        let transport = context.transport();
        let tempo = transport.tempo.map(|t| t as f32);
        if let Some(t) = tempo {
            self.shared.tempo.store(t, Ordering::Relaxed);
        }
        let beat = if transport.playing { transport.pos_beats() } else { None };
        if let Some(g) = &mut self.graph {
            g.set_transport(tempo, beat);
        }

        let mut peak_in = self.shared.meter_in.load(Ordering::Relaxed);
        let mut peak_out = self.shared.meter_out.load(Ordering::Relaxed);
        let decay_per_block = |n: usize, d: f32| d.powi(n as i32);

        for (_, mut block) in buffer.iter_blocks(SUB_BLOCK) {
            let n = block.samples();
            let mut macros = [0.0; MACRO_SLOTS];
            for (m, p) in macros.iter_mut().zip(&self.params.macros) {
                *m = p.value.smoothed.next_step(n as u32);
            }
            let mut frames: [Frame; SUB_BLOCK] = [[0.0; 2]; SUB_BLOCK];
            for ch in 0..2 {
                if let Some(s) = block.get_mut(ch) {
                    for (f, x) in frames.iter_mut().zip(s.iter()) {
                        f[ch] = *x;
                        peak_in = peak_in.max(x.abs());
                    }
                }
            }
            if let Some((test, pos)) = &mut self.test_input {
                for f in frames[..n].iter_mut() {
                    *f = test[*pos];
                    *pos = (*pos + 1) % test.len();
                }
                peak_in = frames[..n].iter().fold(peak_in, |p, f| p.max(f[0].abs()));
            }
            let input = frames;
            if let Some(g) = &mut self.graph {
                g.process(&macros, &mut frames[..n]);
            }
            // Bypass: crossfade to the input, delayed by the chain's latency (the graph keeps
            // running, so tails and LFOs carry on and switching back doesn't jump).
            let latency = self.graph.as_ref().map_or(0, |g| g.latency()).min(BYPASS_DELAY - 1);
            let target = if self.params.bypass.value() { 0.0 } else { 1.0 };
            let step = 1.0 / (BYPASS_FADE_MS / 1000.0 * self.shared.sample_rate().max(1.0));
            if self.wet == 1.0 && target == 1.0 {
                for f in input[..n].iter() {
                    self.dry[self.dry_pos] = *f;
                    self.dry_pos = (self.dry_pos + 1) % BYPASS_DELAY;
                }
            } else {
                for (f, x) in frames[..n].iter_mut().zip(input[..n].iter()) {
                    self.dry[self.dry_pos] = *x;
                    let d = self.dry[(self.dry_pos + BYPASS_DELAY - latency) % BYPASS_DELAY];
                    self.dry_pos = (self.dry_pos + 1) % BYPASS_DELAY;
                    self.wet = if target > self.wet { (self.wet + step).min(1.0) } else { (self.wet - step).max(0.0) };
                    for ch in 0..2 {
                        f[ch] = self.wet * f[ch] + (1.0 - self.wet) * d[ch];
                    }
                }
            }
            finite &= frames[..n].iter().all(|f| f[0].is_finite() && f[1].is_finite());
            self.shared.capture_block(&input[..n], &frames[..n]);
            for ch in 0..2 {
                if let Some(s) = block.get_mut(ch) {
                    for (f, x) in frames.iter().zip(s.iter_mut()) {
                        *x = f[ch];
                        peak_out = peak_out.max(x.abs());
                    }
                }
            }
            let d = decay_per_block(n, self.peak_decay);
            peak_in *= d;
            peak_out *= d;
        }

        self.shared.meter_in.store(peak_in, Ordering::Relaxed);
        self.shared.meter_out.store(peak_out, Ordering::Relaxed);
        // Timing against the buffer's real-time budget (diagnostics builds report it).
        let sr = self.shared.sample_rate().max(1.0);
        let budget = std::time::Duration::from_secs_f64(buffer.samples() as f64 / sr as f64);
        diag::AUDIO.record(started.elapsed(), budget, finite);
        ProcessStatus::Normal
    }
}

impl ClapPlugin for ScriptFx {
    const CLAP_ID: &'static str = "dev.sandbox.scriptfx-poc";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Lua-scripted effect patches (PoC)");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Stereo];
}

impl Vst3Plugin for ScriptFx {
    const VST3_CLASS_ID: [u8; 16] = *b"ScriptFxPoC00001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nice_export_clap!(ScriptFx);
nice_export_vst3!(ScriptFx);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopping_a_listen_keeps_what_was_recorded() {
        let shared = ScriptFx::default().shared;
        shared.allocate_capture(48_000.0);
        shared.start_listen(10.0);
        shared.finish_listen();
        assert!(shared.take_capture().is_none(), "nothing recorded yet");
        let block = [[0.5; 2]; 64];
        shared.capture_block(&block, &block);
        shared.capture_block(&block, &block);
        shared.finish_listen();
        assert_eq!(shared.take_capture().unwrap().0.len(), 128);
        shared.capture_block(&block, &block);
        shared.finish_listen();
        assert!(shared.take_capture().is_none(), "finished only once");
    }
}
