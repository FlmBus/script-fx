//! Patch model + sandboxed Lua loader.
//!
//! A patch is a folder containing `init.luau` (Luau) plus the UI files it
//! `require`s (components, see the `ui-components` crate). Lua only *describes*
//! the patch, using the API from `prelude.luau` (runtime: [`runtime`]):
//!
//! - Params are the blocks' knobs: every numeric block setting is a param
//!   `"<block id>.<setting>"` with the unit, range and default its block type declares
//!   (`prelude.luau`); the patch gives start values (`Blocks.Delay.new("echo", { time = 250 })`)
//!   and reaches them as `echo.params.time`. There is no `Param.new`.
//! - `Macro.new(id, name, {...}):drives(param, {...})`: up to [`MACRO_SLOTS`]
//!   host-automatable controls. Each drives one or more params with its own range
//!   and curve (Ableton-rack style). A param driven by a macro is "locked" to it;
//!   all others are free and keep their own value (persisted with the host project).
//! - `Chain.new { Blocks.<Type>.new(id, {...}), ... }`: the effect chain (see [`chain`]),
//!   compiled to native DSP by the `dsp` crate. Lua never runs on the audio thread.
//! - `Patch.new(name, { chain, macros, theme, author, surface })` is what `init.luau` returns;
//!   `surface` is the root component instance of the patch's own surface.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

pub mod chain;
pub mod locale;
pub mod routes;
pub mod runtime;

pub use routes::{ModEdits, Route, RouteSource};
pub use chain::BlockSpec;

use mlua::{Lua, LuaSerdeExt};
use serde::Deserialize;

/// Number of host-automatable macro slots the plugin exposes.
pub const MACRO_SLOTS: usize = 16;


#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("lua: {0}")]
    Lua(#[from] mlua::Error),
    #[error("invalid patch: {0}")]
    Invalid(String),
}

fn invalid(msg: impl Into<String>) -> PatchError {
    PatchError::Invalid(msg.into())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub min: f32,
    #[serde(default = "one")]
    pub max: f32,
    /// Default base value in plain units (defaults to `min`); macros add offsets to it.
    pub default: Option<f32>,
    #[serde(default)]
    pub unit: String,
    #[serde(default = "two", deserialize_with = "whole_number")]
    pub decimals: usize,
    /// How the 0..1 knob travel maps onto min..max.
    #[serde(default)]
    pub scale: Scale,
    /// Only multiples of this from `min` (a filter's slope: 12, 24, 36, 48). The knob still
    /// turns smoothly; values snap.
    #[serde(default)]
    pub step: Option<f32>,
}

/// Knob travel → value: even steps (`lin`), or even ratios (`log`: frequencies, times; needs
/// min and max of the same sign, not 0).
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Scale {
    #[default]
    Lin,
    Log,
}

fn one() -> f32 {
    1.0
}
fn two() -> usize {
    2
}

/// An optional [`whole_number`].
pub(crate) fn opt_whole_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    match Option::<f64>::deserialize(d)? {
        None => Ok(None),
        Some(v) if v >= 0.0 && v.fract() == 0.0 && v <= u32::MAX as f64 => Ok(Some(v as u32)),
        Some(v) => Err(serde::de::Error::custom(format!("expected a whole number, got {v}"))),
    }
}

/// Luau has only floating-point numbers: accept `2.0` for integer fields.
pub(crate) fn whole_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
    let v = f64::deserialize(d)?;
    if v >= 0.0 && v.fract() == 0.0 {
        Ok(v as usize)
    } else {
        Err(serde::de::Error::custom(format!("expected a whole number, got {v}")))
    }
}

impl ParamSpec {
    pub fn default_plain(&self) -> f32 {
        self.default.unwrap_or(self.min).clamp(self.min.min(self.max), self.min.max(self.max))
    }
    pub fn normalize(&self, plain: f32) -> f32 {
        let t = match self.scale {
            Scale::Lin => (plain - self.min) / (self.max - self.min),
            Scale::Log => (plain / self.min).ln() / (self.max / self.min).ln(),
        };
        if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 }
    }
    pub fn plain(&self, normalized: f32) -> f32 {
        let t = normalized.clamp(0.0, 1.0);
        let v = match self.scale {
            Scale::Lin => self.min + t * (self.max - self.min),
            Scale::Log => self.min * (self.max / self.min).powf(t),
        };
        self.snap(v)
    }

    /// `v` on the nearest step (unchanged without `step`), kept in range.
    pub fn snap(&self, v: f32) -> f32 {
        match self.step.filter(|s| *s > 0.0) {
            Some(s) => (self.min + ((v - self.min) / s).round() * s).clamp(self.min.min(self.max), self.min.max(self.max)),
            None => v,
        }
    }
    /// `"6.0 dB"`, `"40%"`, `"4.0:1"`: a space only before units that start with a letter.
    pub fn display_plain(&self, plain: f32) -> String {
        let sep = if self.unit.starts_with(|c: char| c.is_alphabetic()) { " " } else { "" };
        format!("{:.*}{sep}{}", self.decimals, plain, self.unit)
    }
    pub fn display(&self, normalized: f32) -> String {
        self.display_plain(self.plain(normalized))
    }
    /// Typed text → plain value in range: `"6"`, `"6 dB"`, `"-3,5"`, `"−12dB"`. `None` if there
    /// is no number.
    pub fn parse_plain(&self, text: &str) -> Option<f32> {
        let v = parse_number(text)?;
        Some(self.snap(v.clamp(self.min.min(self.max), self.min.max(self.max))))
    }
}

/// The leading number of typed text: sign, digits, one decimal separator (`.` or `,`); the
/// unit after it is ignored. Accepts the typographic minus (`−`) the displays use.
pub fn parse_number(text: &str) -> Option<f32> {
    let t = text.trim().replace('−', "-").replace(',', ".");
    let end = t
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || ((c == '-' || c == '+') && i == 0)))
        .map_or(t.len(), |(i, _)| i);
    let v: f32 = t[..end].parse().ok()?;
    v.is_finite().then_some(v)
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Curve {
    #[default]
    Lin,
    /// Exponential interpolation (`from * (to/from)^v`), good for frequencies.
    /// Requires `from` and `to` to be non-zero with the same sign.
    Exp,
}

impl Curve {
    pub fn apply(self, from: f32, to: f32, v: f32) -> f32 {
        let v = v.clamp(0.0, 1.0);
        match self {
            Curve::Lin => from + (to - from) * v,
            Curve::Exp => from * (to / from).powf(v),
        }
    }
    /// Inverse of [`apply`](Self::apply), clamped to 0..1.
    pub fn invert(self, from: f32, to: f32, plain: f32) -> f32 {
        let v = match self {
            Curve::Lin => (plain - from) / (to - from),
            Curve::Exp => (plain / from).ln() / (to / from).ln(),
        };
        if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacroTarget {
    pub param: String,
    /// Offset reference at macro = 0 (defaults to the param's min).
    pub from: Option<f32>,
    /// End of the modulation range at macro = 1 (defaults to the param's max).
    /// Actual output also depends on the editable base and any other modulations.
    pub to: Option<f32>,
    #[serde(default)]
    pub curve: Curve,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacroSpec {
    pub id: String,
    pub name: String,
    /// Normalized 0..1. If omitted: derived from the first target's default
    /// (inverting the curve), else 0.
    pub default: Option<f32>,
    #[serde(deserialize_with = "list")]
    pub targets: Vec<MacroTarget>,
}

/// One modulation slot (Serum's model, `docs/research/chain-editor.md`): a source (a macro, for
/// now) moving one param. Everything happens in the param's **normalized** space: each slot
/// adds a delta to the knob's base value, the deltas sum, the sum is clamped to 0..1, and only
/// then the param's scale (lin/log) turns it into Hz, dB, … So a slot's amount means the same
/// share of the knob's travel wherever the knob sits, and modulations add up predictably.
///
/// `settings` is what applies (amount, polarity, curve, bypass: the meta knob); `patch` what the
/// patch defines (reset goes back to it). Settings are saved per project.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Modulation {
    pub slot: usize,
    pub settings: ModSettings,
    pub patch: ModSettings,
}

/// A slot's settings.
///
/// The amount is the size of the range the knob moves in, the same for both polarities: unipolar
/// moves from the base to base + amount, bipolar swings ±amount/2 around the base. Its sign is
/// the direction (there is no separate invert: one control for both, decided 2026-11-21).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, Deserialize)]
#[serde(from = "StoredModSettings")]
pub struct ModSettings {
    /// −1..+1: the share of the knob's travel the source covers (negative = the other way).
    pub amount: f32,
    /// Bipolar: the source's centre is no change, its ends ±amount/2. Unipolar: 0..amount.
    pub bipolar: bool,
    /// Tension −1..+1 applied to the source first: 0 = linear, > 0 slow start, < 0 fast start.
    pub curve: f32,
    /// Muted, settings kept.
    pub bypass: bool,
}

impl Default for ModSettings {
    fn default() -> Self {
        Self { amount: 0.0, bipolar: false, curve: 0.0, bypass: false }
    }
}

/// [`ModSettings`] as saved, including older projects' separate `invert` flag (now the amount's
/// sign).
#[derive(Deserialize)]
#[serde(default)]
struct StoredModSettings {
    amount: f32,
    bipolar: bool,
    curve: f32,
    bypass: bool,
    invert: bool,
}

impl Default for StoredModSettings {
    fn default() -> Self {
        let d = ModSettings::default();
        Self { amount: d.amount, bipolar: d.bipolar, curve: d.curve, bypass: d.bypass, invert: false }
    }
}

impl From<StoredModSettings> for ModSettings {
    fn from(s: StoredModSettings) -> Self {
        Self { amount: if s.invert { -s.amount } else { s.amount }, bipolar: s.bipolar, curve: s.curve, bypass: s.bypass }
    }
}

impl ModSettings {
    /// The normalized delta at source value `source` (0..1).
    pub fn delta(self, source: f32) -> f32 {
        if self.bypass {
            return 0.0;
        }
        let s = tension(source.clamp(0.0, 1.0), self.curve);
        let p = if self.bipolar { s - 0.5 } else { s };
        p * self.amount
    }

    /// Packed into one word, for a lock-free hand-over to the audio thread: amount (f32 bits),
    /// curve (i16 fixed point), flags (bit 2 is free: it was invert).
    pub fn pack(self) -> u64 {
        let curve = (self.curve.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16 as u16 as u64;
        (self.amount.to_bits() as u64) << 32 | curve << 16 | (self.bypass as u64) << 1 | self.bipolar as u64
    }

    pub fn unpack(w: u64) -> Self {
        Self {
            amount: f32::from_bits((w >> 32) as u32),
            curve: ((w >> 16) as u16 as i16) as f32 / i16::MAX as f32,
            bypass: w & 2 != 0,
            bipolar: w & 1 != 0,
        }
    }
}

/// The tension curve: `x^(2^(3c))` (c = 0 linear; ±1 ≈ x⁸ / x^⅛).
pub fn tension(x: f32, c: f32) -> f32 {
    if c == 0.0 { x } else { x.powf((3.0 * c).exp2()) }
}

/// Fixed modulation source identity. Separate from UI [`Source`] and macro slot numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModulatorId {
    Follow,
    Lfo1,
    Lfo2,
    Lfo3,
    Step,
}

impl ModulatorId {
    pub const ALL: [Self; 5] = [Self::Follow, Self::Lfo1, Self::Lfo2, Self::Lfo3, Self::Step];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Follow => "follow",
            Self::Lfo1 => "lfo1",
            Self::Lfo2 => "lfo2",
            Self::Lfo3 => "lfo3",
            Self::Step => "step",
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Follow => 0,
            Self::Lfo1 => 1,
            Self::Lfo2 => 2,
            Self::Lfo3 => 3,
            Self::Step => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModulatorLfoShape {
    Sine,
    Triangle,
    Square,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum ModulatorSettings {
    Follow { attack_ms: f32, release_ms: f32 },
    /// `sync`: the cycle as a note length in beats (quarter notes), overriding `rate_hz`.
    Lfo {
        rate_hz: f32,
        #[serde(default)]
        sync: Option<f32>,
        phase: f32,
        shape: ModulatorLfoShape,
    },
    /// `sync`: one step's length in beats, overriding `rate_hz` (steps per second).
    Step {
        rate_hz: f32,
        #[serde(default)]
        sync: Option<f32>,
        values: Vec<f32>,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulatorSpec {
    pub id: ModulatorId,
    pub settings: ModulatorSettings,
}

impl ModulatorSpec {
    pub fn default_for(id: ModulatorId) -> Self {
        let settings = match id {
            ModulatorId::Follow => ModulatorSettings::Follow { attack_ms: 10.0, release_ms: 150.0 },
            ModulatorId::Step => ModulatorSettings::Step { rate_hz: 2.0, sync: None, values: vec![0.5; 8] },
            _ => ModulatorSettings::Lfo { rate_hz: 1.0, sync: None, phase: 0.0, shape: ModulatorLfoShape::Sine },
        };
        Self { id, settings }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum ModDestination {
    Param(String),
    Macro(String),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModAssignment {
    pub source: ModulatorId,
    pub destination: ModDestination,
    pub settings: ModSettings,
}

/// A value the UI can read: what a component's `bind` / `source` string resolves to.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Normalized 0..1 macro value (slot index).
    Macro(usize),
    /// Normalized 0..1 param value (param index).
    Param(usize),
    /// Output/input level mapped from -60..0 dBFS to 0..1.
    MeterIn,
    MeterOut,
    /// Meter of a chain block (index into [`Patch::meters`]), 0..1:
    /// compressors = gain reduction 0..24 dB; rider/transient = gain, 0.5 = unity.
    Block(usize),
}

/// The patch's design tokens (`theme` in `Patch.new`). `colors`, `fonts` and `images` are the
/// patch's own names (fonts/images as paths relative to the patch folder); `library` overrides
/// them for the library card. The shell reads `colors.accent/bg/ink`, `fonts.title` and
/// `images.logo` unless `library` says otherwise.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub colors: BTreeMap<String, String>,
    pub fonts: BTreeMap<String, String>,
    pub images: BTreeMap<String, String>,
    pub library: LibraryTheme,
}

/// What the library card uses where it differs from the shared tokens.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LibraryTheme {
    pub accent: Option<String>,
    pub bg: Option<String>,
    pub ink: Option<String>,
    /// Font path.
    pub font: Option<String>,
    /// Image path.
    pub logo: Option<String>,
}

impl Theme {
    pub fn color(&self, name: &str) -> Option<&str> {
        self.colors.get(name).map(String::as_str)
    }

    pub fn library_accent(&self) -> Option<&str> {
        self.library.accent.as_deref().or(self.color("accent"))
    }

    pub fn library_bg(&self) -> Option<&str> {
        self.library.bg.as_deref().or(self.color("bg"))
    }

    pub fn library_ink(&self) -> Option<&str> {
        self.library.ink.as_deref().or(self.color("ink"))
    }

    /// The library card's title font (path).
    pub fn title_font(&self) -> Option<&str> {
        self.library.font.as_deref().or(self.fonts.get("title").map(String::as_str))
    }

    /// The library card's logo (path).
    pub fn logo(&self) -> Option<&str> {
        self.library.logo.as_deref().or(self.images.get("logo").map(String::as_str))
    }

    /// Every asset path with where it's used, for validation.
    fn assets(&self) -> Vec<(String, &str, &'static [&'static str])> {
        const FONT: &[&str] = &["ttf", "otf", "svg", "fnt"];
        const IMAGE: &[&str] = &["svg", "png"];
        let mut out: Vec<(String, &str, &'static [&'static str])> = Vec::new();
        out.extend(self.fonts.iter().map(|(k, v)| (format!("theme.fonts.{k}"), v.as_str(), FONT)));
        out.extend(self.images.iter().map(|(k, v)| (format!("theme.images.{k}"), v.as_str(), IMAGE)));
        out.extend(self.library.font.as_deref().map(|v| ("theme.library.font".to_string(), v, FONT)));
        out.extend(self.library.logo.as_deref().map(|v| ("theme.library.logo".to_string(), v, IMAGE)));
        out
    }
}

/// Everything a UI needs for one frame.
#[derive(Debug, Clone, Default)]
pub struct Values {
    pub macros: [f32; MACRO_SLOTS],
    /// Normalized param values (index = param index), see [`Patch::compute_params`].
    pub params: Vec<f32>,
    /// Normalized base values (the knob's own value, before macro offsets).
    pub bases: Vec<f32>,
    pub meter_in: f32,
    pub meter_out: f32,
    /// Block meters, index = [`Patch::meters`].
    pub blocks: Vec<f32>,
    /// Live unipolar `[0, 1]` outputs of Follow, LFO 1–3, Step (in [`ModulatorId::ALL`] order).
    pub modulators: [f32; 5],
    /// Where each modulator is in its cycle: LFOs 0–1, Step the step index, Follow 0.
    pub modulator_phases: [f32; 5],
    pub time: f32,
    /// The host's tempo, BPM (0 until it's known).
    pub tempo: f32,
}

impl Values {
    pub fn get(&self, source: &Source) -> f32 {
        match source {
            Source::Macro(i) => self.macros[*i],
            Source::Param(i) => self.params.get(*i).copied().unwrap_or(0.0),
            Source::MeterIn => self.meter_in,
            Source::MeterOut => self.meter_out,
            Source::Block(i) => self.blocks.get(*i).copied().unwrap_or(0.0),
        }
    }

    /// The knob's own (normalized) value, before macro modulation.
    pub fn base(&self, index: usize) -> f32 {
        self.bases.get(index).copied().unwrap_or_else(|| self.params.get(index).copied().unwrap_or(0.0))
    }
}

pub struct Patch {
    pub name: String,
    pub author: Option<String>,
    pub dir: PathBuf,
    pub params: Vec<ParamSpec>,
    pub macros: Vec<MacroSpec>,
    /// One entry per param: the macros that modulate it (any number, offsets sum).
    pub modulations: Vec<Vec<Modulation>>,
    /// The five fixed modulation sources, in [`ModulatorId::ALL`] order.
    pub modulators: Vec<ModulatorSpec>,
    /// Modulator-to-param and modulator-to-macro assignments.
    pub assignments: Vec<ModAssignment>,
    pub chain: Vec<BlockSpec>,
    /// Ids of metered chain blocks (compressors/rider/transient), in DSP meter-slot order.
    pub meters: Vec<String>,
    pub theme: Theme,
    /// `Patch.new` got a `surface` (the patch's own UI, built by `ui_components::Ui`).
    pub has_surface: bool,
}

impl Patch {
    /// No patch: an empty chain (the signal passes through), no params or macros.
    pub fn empty() -> Self {
        Patch {
            name: String::new(),
            author: None,
            dir: PathBuf::new(),
            params: Vec::new(),
            macros: Vec::new(),
            modulations: Vec::new(),
            modulators: ModulatorId::ALL.into_iter().map(ModulatorSpec::default_for).collect(),
            assignments: Vec::new(),
            chain: Vec::new(),
            meters: Vec::new(),
            theme: Theme::default(),
            has_surface: false,
        }
    }

    /// Loads `<dir>/init.luau` for the DSP side: evaluates it (including the UI
    /// files it requires, but without rendering anything) and turns the returned
    /// `Patch` object into plain data via the prelude's export function.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, PatchError> {
        let dir = dir.as_ref().to_path_buf();
        let rt = runtime::Runtime::new(&dir)?;
        let patch = rt.eval_patch()?;
        let root = rt.export(&patch)?;
        let lua = &rt.lua;

        let has_surface = !patch.get::<mlua::Value>("surface")?.is_nil();
        let name: String = root.get("name")?;
        let author: Option<String> = root.get("author")?;
        let params: Vec<ParamSpec> = opt_from_lua(lua, root.get("params")?)?;
        let macros: Vec<MacroSpec> = opt_from_lua(lua, root.get("macros")?)?;
        let theme: Theme = opt_from_lua(lua, root.get("theme")?)?;
        for (key, rel, exts) in theme.assets() {
            let path = resolve_inside(&dir, rel).map_err(|e| invalid(format!("{key} '{rel}': {e}")))?;
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if !exts.contains(&ext.as_str()) {
                return Err(invalid(format!("{key} '{rel}': must be {}", exts.join(" or "))));
            }
        }
        let modulations = resolve_modulations(&params, &macros)?;
        let raw_modulators: Vec<ModulatorSpec> = opt_from_lua(lua, root.get("modulators")?)?;
        let modulators = validate_modulators(raw_modulators)?;
        let assignments: Vec<ModAssignment> = opt_from_lua(lua, root.get("assignments")?)?;
        validate_assignments(&assignments, &params, &macros)?;
        let chain: Vec<BlockSpec> = opt_from_lua(lua, root.get("chain")?)?;
        let meters = chain::validate(&chain, &params)?;
        Ok(Patch { name, author, dir, params, macros, modulations, modulators, assignments, chain, meters, theme, has_surface })
    }

    pub fn macro_index(&self, id: &str) -> Option<usize> {
        self.macros.iter().position(|m| m.id == id)
    }

    pub fn param_index(&self, id: &str) -> Option<usize> {
        self.params.iter().position(|p| p.id == id)
    }

    pub fn resolve_source(&self, s: &str) -> Result<Source, PatchError> {
        let unknown = || invalid(format!("unknown source '{s}'"));
        Ok(match s.split_once('.') {
            Some(("macro", id)) => Source::Macro(self.macro_index(id).ok_or_else(unknown)?),
            Some(("param", id)) => Source::Param(self.param_index(id).ok_or_else(unknown)?),
            Some(("meter", "in")) => Source::MeterIn,
            Some(("meter", "out")) => Source::MeterOut,
            Some(("meter", id)) => Source::Block(self.meters.iter().position(|m| m == id).ok_or_else(unknown)?),
            _ => return Err(unknown()),
        })
    }

    /// Does any macro modulate this param?
    pub fn is_modulated(&self, index: usize) -> bool {
        self.modulations.get(index).is_some_and(|m| !m.is_empty())
    }

    /// Plain value of a param: the knob's base value plus every macro's offset (VCA style).
    pub fn param_plain(&self, index: usize, macros: &[f32; MACRO_SLOTS], free: &[f32]) -> f32 {
        let spec = &self.params[index];
        let own = free.get(index).copied().unwrap_or_else(|| spec.default_plain());
        let mods = self.modulations.get(index).map(Vec::as_slice).unwrap_or_default();
        if mods.is_empty() {
            // Unmodulated: the knob's own value, exactly (no normalize/plain round-trip drift).
            return own;
        }
        let delta: f32 = mods.iter().map(|m| m.settings.delta(macros[m.slot])).sum();
        spec.plain((spec.normalize(own) + delta).clamp(0.0, 1.0))
    }

    /// Fills `out` with normalized values of all params (macro modulation only).
    pub fn compute_params(&self, macros: &[f32; MACRO_SLOTS], free: &[f32], out: &mut Vec<f32>) {
        self.compute_params_with_modulators(macros, &[0.0; 5], free, out);
    }

    /// Fills `out` with normalized values of all params, including live modulator outputs
    /// (Follow, LFO 1–3, Step in [`ModulatorId::ALL`] order).
    pub fn compute_params_with_modulators(
        &self,
        macros: &[f32; MACRO_SLOTS],
        modulators: &[f32; 5],
        free: &[f32],
        out: &mut Vec<f32>,
    ) {
        let mut eff_macros = *macros;
        if !self.assignments.is_empty() {
            for a in &self.assignments {
                if let ModDestination::Macro(id) = &a.destination {
                    if let Some(slot) = self.macro_index(id) {
                        eff_macros[slot] += a.settings.delta(modulators[a.source.index()]);
                    }
                }
            }
            for v in &mut eff_macros {
                *v = v.clamp(0.0, 1.0);
            }
        }
        out.clear();
        out.extend((0..self.params.len()).map(|i| {
            let spec = &self.params[i];
            let own = free.get(i).copied().unwrap_or_else(|| spec.default_plain());
            let base = spec.normalize(own);
            let mut delta: f32 = self.modulations[i].iter().map(|m| m.settings.delta(eff_macros[m.slot])).sum();
            for a in &self.assignments {
                if let ModDestination::Param(id) = &a.destination {
                    if id == &spec.id {
                        delta += a.settings.delta(modulators[a.source.index()]);
                    }
                }
            }
            (base + delta).clamp(0.0, 1.0)
        }));
    }

    /// Normalized default of a macro slot (see [`MacroSpec::default`]). The prelude derives it
    /// from the first target's start value when the patch doesn't set one.
    pub fn macro_default(&self, slot: usize) -> f32 {
        self.macros.get(slot).and_then(|m| m.default).unwrap_or(0.0).clamp(0.0, 1.0)
    }

    /// Default values of all params (plain units).
    pub fn default_param_values(&self) -> Vec<f32> {
        self.params.iter().map(ParamSpec::default_plain).collect()
    }

    /// Text for a macro: the driven param's value if it drives exactly one, else percent.
    pub fn macro_display(&self, slot: usize, values: &Values) -> String {
        self.macro_text(slot, values.macros[slot])
    }

    /// The param a macro drives and the modulation doing it (with its current settings).
    fn target_modulation(&self, slot: usize, param: &str) -> Option<(usize, Modulation)> {
        let i = self.param_index(param)?;
        let m = *self.modulations.get(i)?.iter().find(|m| m.slot == slot)?;
        Some((i, m))
    }

    /// Where param `param`'s `k`-th modulation sits in one flat list of all modulations (the
    /// audio thread's settings slots).
    pub fn mod_index(&self, param: usize, k: usize) -> usize {
        self.modulations[..param].iter().map(Vec::len).sum::<usize>() + k
    }

    /// The key a modulation's settings are saved under: "param id<macro id".
    pub fn mod_key(&self, param: usize, k: usize) -> String {
        format!("{}<{}", self.params[param].id, self.macros[self.modulations[param][k].slot].id)
    }

    /// Text for a macro at normalized value `v` (see [`macro_display`](Self::macro_display)).
    /// One target: the param's value with this macro at `v` (base + offset). Otherwise percent.
    pub fn macro_text(&self, slot: usize, v: f32) -> String {
        let Some([t]) = self.macros.get(slot).map(|m| m.targets.as_slice()) else {
            return format!("{:.0}%", v * 100.0);
        };
        let Some((i, m)) = self.target_modulation(slot, &t.param) else { return format!("{:.0}%", v * 100.0) };
        let spec = &self.params[i];
        let base = spec.normalize(spec.default_plain());
        spec.display_plain(spec.plain((base + m.settings.delta(v)).clamp(0.0, 1.0)))
    }

    /// Typed text → normalized macro value, the inverse of [`macro_text`](Self::macro_text):
    /// in the driven param's unit if the macro drives exactly one, else percent.
    pub fn macro_from_text(&self, slot: usize, text: &str) -> Option<f32> {
        let [t] = self.macros.get(slot)?.targets.as_slice() else {
            return Some((parse_number(text)? / 100.0).clamp(0.0, 1.0));
        };
        let (i, m) = self.target_modulation(slot, &t.param)?;
        let spec = &self.params[i];
        let want = spec.normalize(parse_number(text)?);
        // Bisection: the curve (and the param's scale) needn't be linear in v.
        let target = want - spec.normalize(spec.default_plain());
        let delta = |v: f32| m.settings.delta(v);
        let rising = delta(1.0) >= delta(0.0);
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..32 {
            let mid = (lo + hi) / 2.0;
            if (delta(mid) < target) == rising {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(((lo + hi) / 2.0).clamp(0.0, 1.0))
    }
}

fn opt_from_lua<T: serde::de::DeserializeOwned + Default>(
    lua: &Lua,
    v: mlua::Value,
) -> Result<T, PatchError> {
    Ok(match v {
        mlua::Value::Nil => T::default(),
        // `{}` is ambiguous in Lua (empty list or empty map): treat as "nothing".
        mlua::Value::Table(t) if t.is_empty() => T::default(),
        v => lua.from_value(v)?,
    })
}

/// Deserializes a list, also accepting `{}` (an empty Lua table looks like a map).
pub(crate) fn list<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either<T> {
        Seq(Vec<T>),
        Map(BTreeMap<String, serde::de::IgnoredAny>),
    }
    match Either::deserialize(d)? {
        Either::Seq(v) => Ok(v),
        Either::Map(m) if m.is_empty() => Ok(Vec::new()),
        Either::Map(_) => Err(serde::de::Error::custom("expected a list")),
    }
}

fn resolve_modulations(params: &[ParamSpec], macros: &[MacroSpec]) -> Result<Vec<Vec<Modulation>>, PatchError> {
    if macros.len() > MACRO_SLOTS {
        return Err(invalid(format!("{} macros declared, only {MACRO_SLOTS} slots available", macros.len())));
    }
    let mut seen = std::collections::HashSet::new();
    for p in params {
        if !seen.insert(p.id.as_str()) {
            return Err(invalid(format!("duplicate param id '{}'", p.id)));
        }
        if p.max == p.min {
            return Err(invalid(format!("param '{}' has min == max", p.id)));
        }
    }

    let mut macro_ids = std::collections::HashSet::new();
    let mut modulations = vec![Vec::new(); params.len()];
    for (slot, m) in macros.iter().enumerate() {
        if !macro_ids.insert(m.id.as_str()) {
            return Err(invalid(format!("duplicate macro id '{}'", m.id)));
        }
        if m.targets.is_empty() {
            return Err(invalid(format!("macro '{}' has no targets", m.id)));
        }
        for t in &m.targets {
            let i = params
                .iter()
                .position(|p| p.id == t.param)
                .ok_or_else(|| invalid(format!("macro '{}' targets unknown param '{}'", m.id, t.param)))?;
            let (from, to) = (t.from.unwrap_or(params[i].min), t.to.unwrap_or(params[i].max));
            if t.curve == Curve::Exp && (from == 0.0 || to == 0.0 || from.signum() != to.signum()) {
                return Err(invalid(format!(
                    "macro '{}' → '{}': exp curve needs non-zero from/to with the same sign",
                    m.id, t.param
                )));
            }
            // `from`..`to` in units → the normalized amount (the prelude made `from` the knob's
            // base). An exp curve is the same as linear on a log knob, so it needs nothing more.
            let p = &params[i];
            let settings = ModSettings { amount: p.normalize(to) - p.normalize(from), ..ModSettings::default() };
            // Several macros may drive one param: their deltas sum onto the knob's base.
            modulations[i].push(Modulation { slot, settings, patch: settings });
        }
    }
    Ok(modulations)
}

/// A synced rate: a note length from 1/64 (1/16 beat) to 16 bars (64 beats).
fn check_sync(id: ModulatorId, sync: Option<f32>) -> Result<(), PatchError> {
    match sync {
        Some(b) if !b.is_finite() || !(1.0 / 16.0..=64.0).contains(&b) => {
            Err(invalid(format!("modulator '{}': sync must be 1/64 .. 16 bars, got {b} beats", id.as_str())))
        }
        _ => Ok(()),
    }
}

pub(crate) fn validate_modulators(raw: Vec<ModulatorSpec>) -> Result<Vec<ModulatorSpec>, PatchError> {
    let mut by_id: [Option<ModulatorSpec>; 5] = std::array::from_fn(|_| None);
    for spec in raw {
        let idx = spec.id.index();
        if by_id[idx].is_some() {
            return Err(invalid(format!("duplicate modulator '{}'", spec.id.as_str())));
        }
        match (&spec.id, &spec.settings) {
            (ModulatorId::Follow, ModulatorSettings::Follow { attack_ms, release_ms }) => {
                if !attack_ms.is_finite() || !(0.0..=1000.0).contains(attack_ms) {
                    return Err(invalid(format!("modulator 'follow': attack_ms must be 0..1000, got {attack_ms}")));
                }
                if !release_ms.is_finite() || !(0.0..=5000.0).contains(release_ms) {
                    return Err(invalid(format!("modulator 'follow': release_ms must be 0..5000, got {release_ms}")));
                }
            }
            (ModulatorId::Lfo1 | ModulatorId::Lfo2 | ModulatorId::Lfo3, ModulatorSettings::Lfo { rate_hz, sync, phase, .. }) => {
                if !rate_hz.is_finite() || !(0.01..=50.0).contains(rate_hz) {
                    return Err(invalid(format!("modulator '{}': rate_hz must be 0.01..50, got {rate_hz}", spec.id.as_str())));
                }
                check_sync(spec.id, *sync)?;
                if !phase.is_finite() || !(0.0..=1.0).contains(phase) {
                    return Err(invalid(format!("modulator '{}': phase must be 0..1, got {phase}", spec.id.as_str())));
                }
            }
            (ModulatorId::Step, ModulatorSettings::Step { rate_hz, sync, values }) => {
                if !rate_hz.is_finite() || !(0.01..=50.0).contains(rate_hz) {
                    return Err(invalid(format!("modulator 'step': rate_hz must be 0.01..50, got {rate_hz}")));
                }
                check_sync(spec.id, *sync)?;
                if values.is_empty() || values.len() > 16 {
                    return Err(invalid(format!("modulator 'step': values must have 1..16 steps, got {}", values.len())));
                }
                for (i, v) in values.iter().enumerate() {
                    if !v.is_finite() || !(0.0..=1.0).contains(v) {
                        return Err(invalid(format!("modulator 'step': value {} must be 0..1, got {v}", i + 1)));
                    }
                }
            }
            _ => {
                return Err(invalid(format!("modulator '{}': settings kind does not match modulator", spec.id.as_str())));
            }
        }
        by_id[idx] = Some(spec);
    }
    Ok(ModulatorId::ALL
        .into_iter()
        .enumerate()
        .map(|(i, id)| by_id[i].take().unwrap_or_else(|| ModulatorSpec::default_for(id)))
        .collect())
}

fn validate_assignments(
    assignments: &[ModAssignment],
    params: &[ParamSpec],
    macros: &[MacroSpec],
) -> Result<(), PatchError> {
    let mut seen = HashSet::new();
    for a in assignments {
        if !a.settings.amount.is_finite() || !(-1.0..=1.0).contains(&a.settings.amount) {
            return Err(invalid(format!("assignment from '{}': amount must be -1..1", a.source.as_str())));
        }
        if !a.settings.curve.is_finite() || !(-1.0..=1.0).contains(&a.settings.curve) {
            return Err(invalid(format!("assignment from '{}': curve must be -1..1", a.source.as_str())));
        }
        match &a.destination {
            ModDestination::Param(id) => {
                if !params.iter().any(|p| p.id == *id) {
                    return Err(invalid(format!("assignment from '{}': unknown param '{id}'", a.source.as_str())));
                }
            }
            ModDestination::Macro(id) => {
                if !macros.iter().any(|m| m.id == *id) {
                    return Err(invalid(format!("assignment from '{}': unknown macro '{id}'", a.source.as_str())));
                }
            }
        }
        if !seen.insert((a.source, a.destination.clone())) {
            return Err(invalid(format!(
                "duplicate assignment from '{}' to {:?}",
                a.source.as_str(),
                a.destination
            )));
        }
    }
    Ok(())
}

/// Joins `rel` onto `dir` and rejects anything that escapes the patch folder.
pub fn resolve_inside(dir: &Path, rel: &str) -> Result<PathBuf, PatchError> {
    let base = dir.canonicalize()?;
    let full = base.join(rel).canonicalize()?;
    if !full.starts_with(&base) {
        return Err(invalid(format!("'{rel}' is outside the patch folder")));
    }
    Ok(full)
}

/// Lists patch folders (those containing `init.luau`) in `root`, sorted.
pub fn discover(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| runtime::ENTRY_FILES.iter().any(|f| p.join(f).is_file()))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn with_patch(src: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "patch-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("init.luau"), src).unwrap();
            TempDir(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn load(src: &str) -> Result<Patch, PatchError> {
        let d = TempDir::with_patch(src);
        Patch::load(&d.0)
    }

    const BASE: &str = r##"
        local c1 = Blocks.FETCompressor.new("c1", { threshold = -6, ratio = 4 })
        local d  = Blocks.Drive.new("d", { amount = 6 })
        local e  = Blocks.PeakEq.new("e", { freq = 100, gain = 5 })
        local lp = Blocks.LowPass.new("lp", { freq = 4000 })
        local s  = Blocks.BandSplit.new("s", {
          crossovers = { 250 },
          bands = {
            Chain.new { Blocks.Rider.new("r", { target = -18, range = 6 }) },
            Chain.new { d },
          },
        })

        local character = Macro.new("character", "Character", { default = 0.5 })
          :drives(d.params.amount, { from = 0, to = 12 })
          :drives(lp.params.freq,  { from = 16000, to = 1000, curve = "exp" })
        local drv = Macro.new("drv", "Drv"):drives(e.params.gain, { from = 0, to = 10 })

        local chain = Chain.new {
          c1,
          s,
          Blocks.Parallel.new("par", { mix = 50, branches = { Chain.new(), Chain.new { Blocks.Transient.new("t", { attack = 3 }) } } }),
        }
        chain:add(e)
        chain:add(lp)

        return Patch.new("T", { chain = chain, macros = { character, drv }, theme = { colors = { accent = "#ff0000" } } })
    "##;

    #[test]
    fn builds_params_macros_and_chain() {
        let p = load(BASE).unwrap();
        let i = |id: &str| p.param_index(id).unwrap_or_else(|| panic!("no param {id}"));
        // Every block setting is a param "<block>.<setting>", in chain order.
        assert_eq!(p.params[0].id, "c1.threshold");
        assert_eq!(p.params[i("s.crossover_1")].name, "Crossover 1");
        assert_eq!(p.params[i("lp.freq")].scale, Scale::Log);
        assert_eq!(p.params.len(), 14 + 2 + 3 + 2 + 2 + 1 + 3 + 2 + 1); // compressor 14, band split crossover + slope, the low-pass with slope 3
        // A stepped param snaps: the knob between 24 and 36 dB/oct plays one of them.
        let slope = &p.params[i("lp.slope")];
        assert_eq!((slope.default_plain(), slope.step), (12.0, Some(12.0)));
        assert_eq!(slope.plain(0.4), 24.0);
        assert_eq!(slope.display(0.6), "36 dB/oct");
        assert_eq!(slope.parse_plain("40"), Some(36.0));
        assert_eq!(p.meters, vec!["c1", "r", "t"]);
        assert_eq!(p.resolve_source("meter.r").unwrap(), Source::Block(1));
        assert_eq!(p.resolve_source("param.d.amount").unwrap(), Source::Param(i("d.amount")));
        assert_eq!(p.chain.len(), 5);
        assert_eq!(p.theme.color("accent"), Some("#ff0000"));
        let (gain, m) = (&p.params[i("e.gain")], p.modulations[i("e.gain")][0]);
        assert_eq!((p.modulations[i("e.gain")].len(), m.slot), (1, 1));
        assert!((m.settings.amount - (gain.normalize(10.0) - gain.normalize(0.0))).abs() < 1e-6);
        assert!(p.modulations[i("s.crossover_1")].is_empty());
        let mut macros = [0.0; MACRO_SLOTS];
        macros[0] = 0.5;
        let free = p.default_param_values();
        assert_eq!(p.param_plain(i("d.amount"), &macros, &free), 6.0); // lin 0..12
        assert!((p.param_plain(i("lp.freq"), &macros, &free) - 4000.0).abs() < 1.0); // exp 16k..1k
        assert_eq!(p.param_plain(i("e.gain"), &macros, &free), 0.0); // driven by macro 1 (= 0)
        assert_eq!(p.param_plain(i("s.crossover_1"), &macros, &free), 250.0); // free: its start value
        assert_eq!(p.param_plain(i("d.mix"), &macros, &free), 100.0); // not set: the block's default
        assert_eq!(p.macro_default(0), 0.5); // explicit
        assert_eq!(p.macro_default(1), 0.5); // derived from e.gain's start value 5 of 0..10

        let mut v = Values { macros, ..Default::default() };
        p.compute_params(&v.macros.clone(), &free, &mut v.params);
        assert_eq!(p.params[i("d.amount")].display(v.params[i("d.amount")]), "6.0 dB");
        assert_eq!(p.macro_display(0, &v), "50%");
    }

    #[test]
    fn blocks_list_their_own_knobs() {
        let p = load(BASE).unwrap();
        let ids = |b: &BlockSpec| b.params.iter().map(|r| p.params[r.0].id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids(&p.chain[0]), [
            "c1.threshold", "c1.ratio", "c1.attack", "c1.release", "c1.knee", "c1.makeup", "c1.mix", "c1.detector", "c1.window",
            "c1.feedback", "c1.ratio_curve", "c1.program", "c1.link", "c1.saturation",
        ]);
        assert_eq!(ids(&p.chain[1]), ["s.crossover_1", "s.slope"]); // the bands' blocks have their own
        let chain::BlockKind::Drive { amount, .. } = &p.chain[1].kind.sub_chains()[1].0[0].kind else { panic!() };
        assert_eq!(p.params[amount.0].id, "d.amount"); // settings point at their param
    }

    #[test]
    fn range_narrows_a_knob() {
        let src = |r: &str| format!("local g = Blocks.Gain.new('g', {{ db = -6 }})\n{r}\nreturn Patch.new('T', {{ chain = Chain.new {{ g }} }})");
        let p = load(&src("g.params.db:range(-24, 6)")).unwrap();
        assert_eq!((p.params[0].min, p.params[0].max), (-24.0, 6.0));
        for (r, msg) in [
            ("g.params.db:range(-24, 48)", "must stay within -120..24 dB"),
            ("g.params.db:range(0, 6)", "the start value -6 is outside"),
            ("g.params.db.range(-24, 6)", "with a colon"),
        ] {
            let err = load(&src(r)).err().unwrap().to_string();
            assert!(err.contains(msg) && err.contains("init.luau:2:"), "{r}: {err}");
        }
    }

    #[test]
    fn log_params_move_in_even_ratios() {
        let p = load(BASE).unwrap();
        let f = &p.params[p.param_index("lp.freq").unwrap()]; // 20..20000 Hz
        assert!((f.plain(0.5) - 632.46).abs() < 0.1, "{}", f.plain(0.5));
        assert!((f.normalize(2000.0) - 2.0 / 3.0).abs() < 1e-4);
        assert_eq!(f.normalize(f.plain(0.25)), 0.25);
    }

    #[test]
    fn several_macros_sum_around_an_editable_base() {
        let p = load(
            "local g = Blocks.Gain.new('g', { db = -12 })
             local a = Macro.new('a', 'A', { default = 0 }):drives(g.params.db, { from = -12, to = 12 })
             local b = Macro.new('b', 'B', { default = 0 }):drives(g.params.db, { from = -12, to = 12 })
             return Patch.new('T', { chain = Chain.new { g }, macros = { a, b } })",
        )
        .unwrap();
        let i = p.param_index("g.db").unwrap();
        // The first macro made the knob's base -12 dB; both macros can drive it.
        assert_eq!(p.params[i].default_plain(), -12.0);
        assert_eq!(p.modulations[i].len(), 2);
        let free = vec![-12.0];
        let mut macros = [0.0; MACRO_SLOTS];
        assert_eq!(p.param_plain(i, &macros, &free), -12.0);
        macros[0] = 1.0;
        assert!((p.param_plain(i, &macros, &free) - 12.0).abs() < 0.01);
        macros[1] = 1.0;
        assert!((p.param_plain(i, &macros, &free) - 24.0).abs() < 0.01, "{}", p.param_plain(i, &macros, &free)); // clamped at the top (Gain is -120..24 dB)
        // The base stays editable: -6 dB with both macros at 0.
        let free = vec![-6.0];
        macros = [0.0; MACRO_SLOTS];
        assert!((p.param_plain(i, &macros, &free) + 6.0).abs() < 0.01);
        macros[0] = 0.5;
        assert!((p.param_plain(i, &macros, &free) - 6.0).abs() < 0.01);
    }

    #[test]
    fn slots_add_normalized_deltas() {
        let p = load(
            "local g = Blocks.Gain.new('g', { db = 0 })
             local a = Macro.new('a', 'A', { default = 0 }):drives(g.params.db, { from = 0, to = 12 })
             return Patch.new('T', { chain = Chain.new { g }, macros = { a } })",
        )
        .unwrap();
        let spec = &p.params[0]; // -120..24 dB, lin
        let m = p.modulations[0][0];
        assert!((m.settings.amount - 12.0 / 144.0).abs() < 1e-6);
        let at = |s: ModSettings, v: f32| spec.plain(spec.normalize(0.0) + s.delta(v));
        let d = m.settings;
        assert!((at(d, 1.0) - 12.0).abs() < 0.01);
        assert!((at(ModSettings { amount: d.amount / 2.0, ..d }, 1.0) - 6.0).abs() < 0.01);
        assert!((at(ModSettings { amount: -d.amount, ..d }, 1.0) + 12.0).abs() < 0.01);
        assert_eq!(at(ModSettings { bypass: true, ..d }, 1.0), 0.0);
        // Bipolar: centre = no change, ends = ±amount/2: the same 12 dB range as unipolar.
        let bi = ModSettings { bipolar: true, ..d };
        assert!(at(bi, 0.5).abs() < 0.01 && (at(bi, 1.0) - 6.0).abs() < 0.01 && (at(bi, 0.0) + 6.0).abs() < 0.01);
        // Tension bends the middle, not the ends.
        assert!(at(ModSettings { curve: 0.5, ..d }, 0.5) < 5.0 && at(ModSettings { curve: -0.5, ..d }, 0.5) > 7.0);
        assert!((at(ModSettings { curve: 0.7, ..d }, 1.0) - 12.0).abs() < 0.01);
        for s in [d, bi, ModSettings { amount: -0.3, curve: -0.25, bypass: true, bipolar: false }] {
            let u = ModSettings::unpack(s.pack());
            assert!(u.amount == s.amount && u.bipolar == s.bipolar && u.bypass == s.bypass && (u.curve - s.curve).abs() < 1e-4);
        }
        // An older project's invert flag becomes the amount's sign.
        let old: ModSettings = serde_json::from_str(r#"{ "amount": 0.4, "invert": true }"#).unwrap();
        assert_eq!(old, ModSettings { amount: -0.4, ..ModSettings::default() });
    }

    #[test]
    fn exp_curves_are_exact_on_log_knobs() {
        // A low-pass freq is a log knob: exp from/to = an even share of its travel.
        let p = load(
            "local f = Blocks.LowPass.new('f', { freq = 1000 })
             local m = Macro.new('m', 'M', { default = 0 }):drives(f.params.freq, { from = 200, to = 2000, curve = 'exp' })
             return Patch.new('T', { chain = Chain.new { f }, macros = { m } })",
        )
        .unwrap();
        let i = p.param_index("f.freq").unwrap();
        let mut macros = [0.0; MACRO_SLOTS];
        macros[0] = 0.5;
        let hz = p.param_plain(i, &macros, &[200.0]);
        assert!((hz - 632.46).abs() < 0.5, "{hz}"); // √(200·2000)
    }

    #[test]
    fn exponential_default_handles_start_outside_endpoint_sign() {
        for (start, from, to, expected) in [(0, 1, 10, 0.0), (0, -10, -1, 1.0), (5, -10, -1, 1.0), (-5, 1, 10, 0.0)] {
            let src = format!("local g = Blocks.Gain.new('g', {{ db = {start} }})\n\
                local m = Macro.new('m', 'M'):drives(g.params.db, {{ from = {from}, to = {to}, curve = 'exp' }})\n\
                return Patch.new('T', {{ chain = Chain.new {{ g }}, macros = {{ m }} }})");
            let p = load(&src).unwrap();
            assert_eq!(p.macro_default(0), expected, "{src}");
            assert!(p.param_plain(0, &[expected; MACRO_SLOTS], &p.default_param_values()).is_finite());
        }
    }

    #[test]
    fn typed_values_parse_back() {
        for (text, want) in [("6", Some(6.0)), ("6.5 dB", Some(6.5)), ("-3,5dB", Some(-3.5)), ("−12 dB", Some(-12.0)), ("+2", Some(2.0)), ("dB", None), ("", None)] {
            assert_eq!(parse_number(text), want, "{text:?}");
        }
        let p = load(BASE).unwrap();
        // the crossover: a free param, 20..20000 Hz, clamped into range
        let x = &p.params[p.param_index("s.crossover_1").unwrap()];
        assert_eq!(x.parse_plain("300 Hz"), Some(300.0));
        assert_eq!(x.parse_plain("99999"), Some(20000.0));
        // drv drives one param (0..10): typed in that param's unit
        let v = p.macro_from_text(1, "5").unwrap();
        assert!((v - 0.5).abs() < 1e-5, "{v}");
        assert!((p.macro_from_text(1, "20").unwrap() - 1.0).abs() < 1e-5);
        // character drives two: percent, like its display
        assert_eq!(p.macro_from_text(0, "25%"), Some(0.25));
        assert_eq!(p.macro_from_text(0, "abc"), None);
    }

    #[test]
    fn loads_and_validates_modulators_and_assignments() {
        let p = load(
            "local g = Blocks.Gain.new('g', { db = 0 })
             local m = Macro.new('m', 'M', { default = 0.5 }):drives(g.params.db, { from = -12, to = 12 })
             return Patch.new('T', {
               chain = Chain.new { g },
               macros = { m },
               modulators = {
                 { id = 'follow', attack_ms = 5, release_ms = 80 },
                 { id = 'lfo1', rate_hz = 2, phase = 0.25, shape = 'triangle' },
                 { id = 'step', rate_hz = 4, values = { 0, 0.5, 1 } },
               },
               assignments = {
                 { source = 'follow', destination = g.params.db, amount = 0.25, bipolar = true },
                 { source = 'lfo1', destination = m, amount = -0.5, curve = 0.2 },
               },
             })",
        )
        .unwrap();
        assert_eq!(p.modulators.len(), 5);
        assert_eq!(p.modulators[0].id, ModulatorId::Follow);
        assert_eq!(p.modulators[1].id, ModulatorId::Lfo1);
        assert_eq!(p.modulators[4].id, ModulatorId::Step);
        assert_eq!(p.assignments.len(), 2);
        assert_eq!(p.assignments[0].destination, ModDestination::Param("g.db".into()));
        assert_eq!(p.assignments[1].destination, ModDestination::Macro("m".into()));

        // Reject duplicate assignment and out-of-range settings.
        let dup = load(
            "local g = Blocks.Gain.new('g', { db = 0 })
             return Patch.new('T', {
               chain = Chain.new { g },
               assignments = {
                 { source = 'lfo1', destination = g.params.db, amount = 0.5 },
                 { source = 'lfo1', destination = g.params.db, amount = -0.5 },
               },
             })",
        );
        assert!(dup.is_err());

        let bad_rate = load(
            "local g = Blocks.Gain.new('g', { db = 0 })
             return Patch.new('T', {
               chain = Chain.new { g },
               modulators = { { id = 'lfo1', rate_hz = 100 } },
             })",
        );
        assert!(bad_rate.is_err());
    }

    /// Each case: (replace this, with that, expected error substring).
    #[test]
    fn rejects_mistakes_with_useful_messages() {
        for (from, to, msg) in [
            ("ratio = 4 }", "ratio = 4, treshold = 3 }", "unknown option 'treshold'"),
            ("threshold = -6", "threshold = 6", "threshold must be -60..0 dB, got 6"),
            ("threshold = -6", r#"threshold = "param.x""#, "must be a number (the start value"),
            ("Blocks.FETCompressor.new", "Blocks.FetCompressor.new", "'Blocks.FetCompressor' does not exist"),
            (r#"Blocks.LowPass.new("lp""#, r#"Blocks.LowPass.new("c1""#, "duplicate block id 'c1'"),
            ("crossovers = { 250 }", "crossovers = { 250, 800 }", "2 crossover(s) need 3 bands"),
            ("crossovers = { 250 }", "crossovers = { 5 }", "crossovers must be 20..20000 Hz"),
            ("mix = 50, branches = { Chain.new(),", "mix = 50, branches = { Chain.new(), Chain.new(),", "'mix' needs exactly 2 branches"),
            (r#"{ from = 16000, to = 1000, curve = "exp" }"#, r#"{ from = 0, to = 1000, curve = "exp" }"#, "non-zero from/to"),
            (r#"curve = "exp""#, r#"curve = "log""#, r#"must be "lin" or "exp""#),
            ("d.params.amount, { from", "d.params.amont, { from", "'d.params.amont' does not exist"),
            ("local drv = Macro", "local x = Param.new('x', 'X', {}) local drv = Macro", "every param belongs to a block"),
            ("macros = { character, drv }", "macros = { character, drv, character }", "listed twice"),
            (":drives(e.params.gain, { from = 0, to = 10 })", "", "drives nothing"),
            ("Chain.new { Blocks.Rider", "Chain.new { nil, Blocks.Rider", "holes"),
            (r#"return Patch.new("T","#, r#"return { name = "T" } -- "#, "must end with `return Patch.new(...)`"),
        ] {
            assert!(BASE.contains(from), "test case doesn't apply: {from}");
            let src = BASE.replace(from, to);
            let err = match load(&src) {
                Ok(_) => panic!("should fail: {from} -> {to}"),
                Err(e) => e.to_string(),
            };
            assert!(err.contains(msg), "expected '{msg}', got: {err}");
        }
    }

    #[test]
    fn utility_and_routing_blocks_check_their_options() {
        for (src, msg) in [
            ("Blocks.Haas.new('h', { delay = 50 })", "delay must be -40..40 ms"),
            ("Blocks.Haas.new('h', { delay = 5, channel = 'both' })", r#"'channel' must be "left" or "right""#),
            ("Blocks.Invert.new('i', { channels = 'middle' })", r#"must be "both", "left" or "right""#),
            ("Blocks.MidSide.new('ms', {})", "needs a 'mid' and/or a 'side' chain"),
            ("Blocks.LeftRight.new('lr', { left = 3 })", "'left' must be a Chain"),
            ("Blocks.DCBlock.new('dc', { freq = 5 })", "unknown option 'freq'"),
        ] {
            let err = load(&format!("return Patch.new('T', {{ chain = Chain.new {{ {src} }} }})")).err().unwrap().to_string();
            assert!(err.contains(msg), "{src}: expected '{msg}', got: {err}");
        }
        let err = load("return Patch.new('T', { chain = Chain.new { Blocks.Gain.new('g', { db = 0, oversampling = 3 }) } })")
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("'oversampling' must be 1, 2, 4 or 8"), "{err}");
        let err = load("return Patch.new('T', { chain = Chain.new { Blocks.MidSide.new('ms', { oversampling = 4,
            side = Chain.new { Blocks.Drive.new('d', { amount = 6, oversampling = 4 }) } }) } })")
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("nested oversampling multiplies to 16x"), "{err}");
        let p = load("return Patch.new('T', { chain = Chain.new { Blocks.MidSide.new('ms', { oversampling = 4,
            side = Chain.new { Blocks.Meter.new('m', { oversampling = 2 }) } }) } })").unwrap();
        assert_eq!(p.chain[0].oversampling, Some(4));
        assert_eq!(p.meters, ["m"]);
    }

    #[test]
    fn notes_sync_and_curves_become_plain_data() {
        let p = load(
            "return Patch.new('T', { chain = Chain.new {
                Blocks.Resonator.new('r', { notes = { 'A4', 'C#3', 110, 'Bb1' } }),
                Blocks.Delay.new('d', { sync = '1/8.' }),
                Blocks.Chorus.new('c', { sync = '1/4t' }),
                Blocks.Waveshaper.new('w', { curve = function(x) return x * x end }),
                Blocks.Waveshaper.new('w2', { curve = { -1, 1 } }),
            } })",
        )
        .unwrap();
        use chain::BlockKind as K;
        let K::Resonator { notes, .. } = &p.chain[0].kind else { panic!() };
        let expect = [440.0, 138.59, 110.0, 58.27];
        assert!(notes.iter().zip(expect).all(|(a, b)| (a - b).abs() < 0.01), "{notes:?}");
        let K::Delay { sync, .. } = &p.chain[1].kind else { panic!() };
        assert_eq!(*sync, Some(0.75));
        let K::Chorus { sync, .. } = &p.chain[2].kind else { panic!() };
        assert!((sync.unwrap() - 2.0 / 3.0).abs() < 1e-6);
        let K::Waveshaper { curve, .. } = &p.chain[3].kind else { panic!() };
        assert_eq!(curve.len(), 1025);
        assert!((curve[0] - 1.0).abs() < 1e-6 && curve[512].abs() < 1e-6);
        let K::Waveshaper { curve, .. } = &p.chain[4].kind else { panic!() };
        assert_eq!(curve, &[-1.0, 1.0]);
    }

    #[test]
    fn effect_blocks_check_their_options() {
        for (src, msg) in [
            ("Blocks.Resonator.new('r', { notes = { 'H2' } })", "note names like"),
            ("Blocks.Delay.new('d', { sync = '1/0' })", "a note length like"),
            ("Blocks.Delay.new('d', { time = 9000 })", "time must be 1..4000 ms"),
            ("Blocks.Waveshaper.new('w', { curve = function(x) return 'no' end })", "must return a finite number"),
            ("Blocks.Waveshaper.new('w', { curve = { 1 } })", "at least 2 numbers"),
            ("Blocks.Rectifier.new('r', { mode = 'quarter' })", r#""half", "full""#),
            ("Blocks.Phaser.new('p', { stages = 5 })", "stages must be 2, 4, ..., 12"),
            ("Blocks.Limiter.new('l', { lookahead = 50 })", "lookahead must be 0..20 ms"),
            ("Blocks.Tremolo.new('t', { shape = 'saw' })", r#""sine", "triangle", "square""#),
            ("Blocks.PitchShift.new('p', { semitones = 30 })", "semitones must be -24..24"),
        ] {
            let err = load(&format!("return Patch.new('T', {{ chain = Chain.new {{ {src} }} }})")).err().unwrap().to_string();
            assert!(err.contains(msg), "{src}: expected '{msg}', got: {err}");
            assert!(err.contains("init.luau:1:"), "{src}: error should point at the patch: {err}");
        }
    }

    #[test]
    fn errors_point_at_the_patch_line() {
        let err = load("local x = 1\nlocal g = Blocks.Gain.new('g', { db = 99 })\nreturn Patch.new(\"T\")")
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("init.luau:2:"), "{err}");
        assert!(err.contains("db must be -120..24 dB"), "{err}");
    }

    #[test]
    fn a_block_belongs_to_one_chain() {
        let err = load(
            "local g = Blocks.Gain.new('g', { db = 0 })\nlocal a = Chain.new { g }\nlocal b = Chain.new { g }\nreturn Patch.new('T', { chain = a })",
        )
        .err()
        .unwrap()
        .to_string();
        assert!(err.contains("already part of a chain"), "{err}");
    }

    #[test]
    fn sandbox_blocks_io_loops_and_builtin_changes() {
        assert!(load(r#"io.open("/etc/passwd") return Patch.new("T")"#).is_err());
        let err = load("while true do end").err().unwrap().to_string();
        assert!(err.contains("budget"), "{err}");
        let err = load("string.format = nil return Patch.new('T')").err().unwrap().to_string();
        assert!(err.contains("readonly"), "{err}");
        let err = load("Macro.new = nil return Patch.new('T')").err().unwrap().to_string();
        assert!(err.contains("readonly"), "{err}");
        for f in ["loadstring", "getfenv", "setfenv", "dofile", "loadfile"] {
            let err = load(&format!("assert({f} == nil, '{f} exists') return Patch.new('T')"));
            assert!(err.is_ok(), "{f}: {:?}", err.err());
        }
    }

    #[test]
    fn theme_holds_tokens_and_library_overrides() {
        let src = |theme: &str| {
            format!(
                "return Patch.new('T', {{ chain = Chain.new({{ Blocks.Gain.new('out', {{ db = 0 }}) }}), theme = {theme} }})"
            )
        };
        let good = r##"{
            colors = { accent = "#ff0000", bg = "#101010", rust = "#8a3b1c" },
            fonts = { title = Font.new("fonts/a.ttf"), body = Font.new("fonts/b.svg") },
            images = { logo = Image.new("logo.svg") },
            library = { bg = "#202020", font = Font.new("fonts/b.svg") },
        }"##;
        let d = TempDir::with_patch(&src(good));
        std::fs::create_dir_all(d.0.join("fonts")).unwrap();
        for f in ["fonts/a.ttf", "fonts/b.svg", "logo.svg", "logo.gif"] {
            std::fs::write(d.0.join(f), b"x").unwrap();
        }
        let p = Patch::load(&d.0).unwrap();
        let t = &p.theme;
        assert_eq!(t.color("rust"), Some("#8a3b1c"));
        assert_eq!(t.library_accent(), Some("#ff0000")); // shared token
        assert_eq!(t.library_bg(), Some("#202020")); // library override
        assert_eq!(t.title_font(), Some("fonts/b.svg")); // library.font wins over fonts.title
        assert_eq!(t.logo(), Some("logo.svg"));
        assert_eq!(t.fonts.get("body").map(String::as_str), Some("fonts/b.svg"));

        for (theme, msg) in [
            (r##"{ colours = {} }"##, "theme: unknown key 'colours'"),
            (r##"{ colors = { accent = "red" } }"##, "theme.colors.accent must be a colour"),
            (r##"{ fonts = { title = "fonts/a.ttf" } }"##, "theme.fonts.title must be a Font"),
            (r##"{ images = { logo = Font.new("fonts/a.ttf") } }"##, "theme.images.logo must be an Image (Image.new(path)), got a Font"),
            (r##"{ library = { fnt = Font.new("fonts/a.ttf") } }"##, "theme.library: unknown key 'fnt'"),
            (r##"{ library = { logo = Image.new("logo.svg"), ink = "#fff" } }"##, "theme.library.ink must be a colour"),
            (r##"{ fonts = { title = Font.new("fonts/missing.ttf") } }"##, "theme.fonts.title 'fonts/missing.ttf'"),
            (r##"{ images = { logo = Image.new("logo.gif") } }"##, "must be .png, .svg"),
            (r##"{ images = { logo = Image.new("../outside.svg") } }"##, "must be a path inside the patch folder"),
            (r##"{ fonts = { title = Font.new("logo.gif") } }"##, "must be .fnt, .otf, .svg, .ttf"),
        ] {
            std::fs::write(d.0.join("init.luau"), src(theme)).unwrap();
            let err = Patch::load(&d.0).err().map(|e| e.to_string()).unwrap_or_default();
            assert!(err.contains(msg), "{theme}: expected '{msg}', got: {err}");
        }
    }

    #[test]
    fn requires_ui_files_from_the_patch_folder_only() {
        let d = TempDir::with_patch(
            "local ui = require('@self/ui')
             local out = Blocks.Gain.new('out', { db = 0 })
             local m = Macro.new('m', 'M'):drives(out.params.db)
             return Patch.new('T', { chain = Chain.new({ out }), macros = { m },
                                     surface = ui.Surface.new({ gain = m }) })",
        );
        std::fs::create_dir_all(d.0.join("components")).unwrap();
        std::fs::write(
            d.0.join("ui.luau"),
            "local Knob = require('./components/knob')
             local M = {}
             M.Surface = Component.define({ name = 'Surface', render = function(p) return Knob.new({ bind = p.gain }) end })
             return M",
        )
        .unwrap();
        std::fs::write(
            d.0.join("components/knob.luau"),
            "return Component.define({ name = 'Knob', kind = 'knob', render = function(p) return '' end })",
        )
        .unwrap();
        let p = Patch::load(&d.0).unwrap();
        assert_eq!(p.macros[0].id, "m");

        // a string where a Macro belongs is caught when the instance is created
        std::fs::write(
            d.0.join("ui.luau"),
            "local M = {}
             M.Surface = Component.define({ name = 'Surface', render = function(p) return '' end })
             local K = Component.define({ name = 'K', render = function(p) return '' end })
             K.new({ bind = 'macro.m' })
             return M",
        )
        .unwrap();
        let err = Patch::load(&d.0).err().unwrap().to_string();
        assert!(err.contains("ui.luau:4:") && err.contains("'bind' must be a Macro or Param"), "{err}");

        // nothing outside the patch folder
        let outside = d.0.parent().unwrap().join(format!("outside-{}.luau", std::process::id()));
        std::fs::write(&outside, "return 1").unwrap();
        let name = outside.file_stem().unwrap().to_str().unwrap();
        // init.luau is the folder's module: `./x` is a sibling of the folder, `../x` above it.
        for req in [format!("./{name}"), format!("../{}/{name}", d.0.parent().unwrap().file_name().unwrap().to_str().unwrap())] {
            std::fs::write(d.0.join("init.luau"), format!("require('{req}') return Patch.new('T')")).unwrap();
            let err = Patch::load(&d.0).err().unwrap().to_string();
            assert!(err.contains("only files inside the patch folder") && err.contains("@self/"), "{req}: {err}");
        }
        let _ = std::fs::remove_file(&outside);
    }

    #[test]
    fn the_patch_folder_is_a_luau_module() {
        // init.lua works too (Luau's rules for a folder module) ...
        let d = TempDir::with_patch("return Patch.new('luau')");
        std::fs::remove_file(d.0.join("init.luau")).unwrap();
        std::fs::write(d.0.join("init.lua"), "local x = require('@self/x') return Patch.new(x)").unwrap();
        std::fs::write(d.0.join("x.lua"), "return 'from init.lua'").unwrap();
        assert_eq!(Patch::load(&d.0).unwrap().name, "from init.lua");
        assert_eq!(discover(d.0.parent().unwrap()).iter().filter(|p| **p == d.0).count(), 1);
        // ... but not both.
        std::fs::write(d.0.join("init.luau"), "return Patch.new('luau')").unwrap();
        let err = Patch::load(&d.0).err().unwrap().to_string();
        assert!(err.contains("ambiguous"), "{err}");
        // No entry at all.
        std::fs::remove_file(d.0.join("init.luau")).unwrap();
        std::fs::remove_file(d.0.join("init.lua")).unwrap();
        assert!(Patch::load(&d.0).err().unwrap().to_string().contains("no init.luau"));
    }
}
