//! Patch model + sandboxed Lua loader.
//!
//! A patch is a folder containing `init.luau` (Luau) plus the UI files it
//! `require`s (components, see the `ui-components` crate). Lua only *describes*
//! the patch, using the API from `prelude.luau` (runtime: [`runtime`]):
//!
//! - `Param.new(id, name, {...})`: internal controls (unlimited). Blocks and macros
//!   reference them as objects; the patch collects every param it reaches.
//! - `Macro.new(id, name, {...}):drives(param, {...})`: up to [`MACRO_SLOTS`]
//!   host-automatable controls. Each drives one or more params with its own range
//!   and curve (Ableton-rack style). A param driven by a macro is "locked" to it;
//!   all others are free and keep their own value (persisted with the host project).
//! - `Chain.new { Blocks.<Type>.new(id, {...}), ... }`: the effect chain (see [`chain`]),
//!   compiled to native DSP by the `dsp` crate. Lua never runs on the audio thread.
//! - `Patch.new(name, { chain, macros, theme, author, panel })` is what `init.luau` returns;
//!   `panel` is the root component instance of the ControlPanel.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub mod chain;
pub mod locale;
pub mod runtime;
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
    /// Default in plain units (defaults to `min`). Ignored when a macro drives the param.
    pub default: Option<f32>,
    #[serde(default)]
    pub unit: String,
    #[serde(default = "two", deserialize_with = "whole_number")]
    pub decimals: usize,
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
fn whole_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
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
        ((plain - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }
    pub fn plain(&self, normalized: f32) -> f32 {
        self.min + normalized.clamp(0.0, 1.0) * (self.max - self.min)
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
        Some(v.clamp(self.min.min(self.max), self.min.max(self.max)))
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
    /// Plain value at macro = 0 (defaults to the param's min).
    pub from: Option<f32>,
    /// Plain value at macro = 1 (defaults to the param's max).
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

/// How a param gets its value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    /// The param keeps its own value.
    Free,
    /// Macro slot `slot` drives it: `curve.apply(from, to, macro_value)`.
    Macro { slot: usize, from: f32, to: f32, curve: Curve },
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
    pub meter_in: f32,
    pub meter_out: f32,
    /// Block meters, index = [`Patch::meters`].
    pub blocks: Vec<f32>,
    pub time: f32,
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
}

pub struct Patch {
    pub name: String,
    pub author: Option<String>,
    pub dir: PathBuf,
    pub params: Vec<ParamSpec>,
    pub macros: Vec<MacroSpec>,
    /// One entry per param.
    pub controls: Vec<Control>,
    pub chain: Vec<BlockSpec>,
    /// Ids of metered chain blocks (compressors/rider/transient), in DSP meter-slot order.
    pub meters: Vec<String>,
    pub theme: Theme,
    /// `Patch.new` got a `panel` (the component UI, built by `ui_components::Ui`).
    pub has_panel: bool,
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
            controls: Vec::new(),
            chain: Vec::new(),
            meters: Vec::new(),
            theme: Theme::default(),
            has_panel: false,
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

        let has_panel = !patch.get::<mlua::Value>("panel")?.is_nil();
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
        let controls = resolve_controls(&params, &macros)?;
        let chain: Vec<BlockSpec> = opt_from_lua(lua, root.get("chain")?)?;
        let meters = chain::validate(&chain, &params)?;
        Ok(Patch { name, author, dir, params, macros, controls, chain, meters, theme, has_panel })
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

    /// Plain value of every param, given macro values and the free params' own values.
    pub fn param_plain(&self, index: usize, macros: &[f32; MACRO_SLOTS], free: &[f32]) -> f32 {
        match self.controls[index] {
            Control::Free => free.get(index).copied().unwrap_or_else(|| self.params[index].default_plain()),
            Control::Macro { slot, from, to, curve } => curve.apply(from, to, macros[slot]),
        }
    }

    /// Fills `out` with normalized values of all params.
    pub fn compute_params(&self, macros: &[f32; MACRO_SLOTS], free: &[f32], out: &mut Vec<f32>) {
        out.clear();
        out.extend(
            (0..self.params.len()).map(|i| self.params[i].normalize(self.param_plain(i, macros, free))),
        );
    }

    /// Normalized default of a macro slot (see [`MacroSpec::default`]).
    pub fn macro_default(&self, slot: usize) -> f32 {
        let Some(m) = self.macros.get(slot) else { return 0.0 };
        if let Some(d) = m.default {
            return d.clamp(0.0, 1.0);
        }
        let Some(i) = m.targets.first().and_then(|t| self.param_index(&t.param)) else { return 0.0 };
        match self.controls[i] {
            Control::Macro { from, to, curve, .. } => curve.invert(from, to, self.params[i].default_plain()),
            Control::Free => 0.0,
        }
    }

    /// Default values of all params (plain units).
    pub fn default_param_values(&self) -> Vec<f32> {
        self.params.iter().map(ParamSpec::default_plain).collect()
    }

    /// Text for a macro: the driven param's value if it drives exactly one, else percent.
    pub fn macro_display(&self, slot: usize, values: &Values) -> String {
        self.macro_text(slot, values.macros[slot])
    }

    /// Text for a macro at normalized value `v` (see [`macro_display`](Self::macro_display)).
    pub fn macro_text(&self, slot: usize, v: f32) -> String {
        match self.macros.get(slot).map(|m| m.targets.as_slice()) {
            Some([t]) => {
                let i = self.param_index(&t.param).expect("validated");
                match self.controls[i] {
                    Control::Macro { from, to, curve, .. } => {
                        self.params[i].display_plain(curve.apply(from, to, v))
                    }
                    Control::Free => unreachable!("targets are macro-controlled"),
                }
            }
            _ => format!("{:.0}%", v * 100.0),
        }
    }

    /// Typed text → normalized macro value, the inverse of [`macro_text`](Self::macro_text):
    /// in the driven param's unit if the macro drives exactly one, else percent.
    pub fn macro_from_text(&self, slot: usize, text: &str) -> Option<f32> {
        match self.macros.get(slot)?.targets.as_slice() {
            [t] => {
                let i = self.param_index(&t.param)?;
                match self.controls[i] {
                    Control::Macro { from, to, curve, .. } => {
                        let plain = parse_number(text)?.clamp(from.min(to), from.max(to));
                        Some(curve.invert(from, to, plain).clamp(0.0, 1.0))
                    }
                    Control::Free => None,
                }
            }
            _ => Some((parse_number(text)? / 100.0).clamp(0.0, 1.0)),
        }
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

fn resolve_controls(params: &[ParamSpec], macros: &[MacroSpec]) -> Result<Vec<Control>, PatchError> {
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
    let mut controls = vec![Control::Free; params.len()];
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
            if controls[i] != Control::Free {
                return Err(invalid(format!("param '{}' is driven by more than one macro", t.param)));
            }
            let (from, to) = (t.from.unwrap_or(params[i].min), t.to.unwrap_or(params[i].max));
            if t.curve == Curve::Exp && (from == 0.0 || to == 0.0 || from.signum() != to.signum()) {
                return Err(invalid(format!(
                    "macro '{}' → '{}': exp curve needs non-zero from/to with the same sign",
                    m.id, t.param
                )));
            }
            controls[i] = Control::Macro { slot, from, to, curve: t.curve };
        }
    }
    Ok(controls)
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
        local drive = Param.new("drive", "Drive", { min = 0, max = 24, unit = "dB", decimals = 1, default = 6 })
        local tone  = Param.new("tone",  "Tone",  { min = 500, max = 20000, unit = "Hz", decimals = 0 })
        local free  = Param.new("free",  "Free",  { min = 0, max = 10, default = 5 })
        local xover = Param.new("xover", "Crossover", { min = 100, max = 500, default = 250, unit = "Hz" })

        local character = Macro.new("character", "Character", { default = 0.5 })
          :drives(drive, { from = 0, to = 12 })
          :drives(tone,  { from = 16000, to = 1000, curve = "exp" })
        local drv = Macro.new("drv", "Drv"):drives(free, { from = 0, to = 10 })

        local chain = Chain.new {
          Blocks.FETCompressor.new("c1", { threshold = drive, ratio = 4 }),
          Blocks.BandSplit.new("s", {
            crossovers = { xover },
            bands = {
              Chain.new { Blocks.Rider.new("r", { target = -18, range = 6 }) },
              Chain.new { Blocks.Drive.new("d", { amount = drive }) },
            },
          }),
          Blocks.Parallel.new("par", { mix = 50, branches = { Chain.new(), Chain.new { Blocks.Transient.new("t", { attack = 3 }) } } }),
        }
        chain:add(Blocks.PeakEq.new("e", { freq = 100, gain = free }))
        chain:add(Blocks.LowPass.new("lp", { freq = tone }))

        return Patch.new("T", { chain = chain, macros = { character, drv }, theme = { colors = { accent = "#ff0000" } } })
    "##;

    #[test]
    fn builds_params_macros_and_chain() {
        let p = load(BASE).unwrap();
        // Params in the order the chain reaches them, then macro-only ones.
        let ids: Vec<_> = p.params.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["drive", "xover", "free", "tone"]);
        assert_eq!(p.meters, vec!["c1", "r", "t"]);
        assert_eq!(p.resolve_source("meter.r").unwrap(), Source::Block(1));
        assert_eq!(p.chain.len(), 5);
        assert_eq!(p.theme.color("accent"), Some("#ff0000"));
        assert_eq!(p.controls[2], Control::Macro { slot: 1, from: 0.0, to: 10.0, curve: Curve::Lin });
        assert_eq!(p.controls[1], Control::Free);
        let mut macros = [0.0; MACRO_SLOTS];
        macros[0] = 0.5;
        let free = p.default_param_values();
        assert_eq!(p.param_plain(0, &macros, &free), 6.0); // lin 0..12
        assert!((p.param_plain(3, &macros, &free) - 4000.0).abs() < 1.0); // exp 16k..1k
        assert_eq!(p.param_plain(2, &macros, &free), 0.0); // driven by macro 1 (= 0)
        assert_eq!(p.param_plain(1, &macros, &free), 250.0); // free: own default
        assert_eq!(p.macro_default(0), 0.5); // explicit
        assert_eq!(p.macro_default(1), 0.5); // derived from param default 5 of 0..10

        let mut v = Values { macros, ..Default::default() };
        p.compute_params(&v.macros.clone(), &free, &mut v.params);
        assert_eq!(p.params[0].display(v.params[0]), "6.0 dB");
        assert_eq!(p.macro_display(0, &v), "50%");
    }

    #[test]
    fn typed_values_parse_back() {
        for (text, want) in [("6", Some(6.0)), ("6.5 dB", Some(6.5)), ("-3,5dB", Some(-3.5)), ("−12 dB", Some(-12.0)), ("+2", Some(2.0)), ("dB", None), ("", None)] {
            assert_eq!(parse_number(text), want, "{text:?}");
        }
        let p = load(BASE).unwrap();
        // xover: a free param, 100..500 Hz, clamped into range
        assert_eq!(p.params[1].parse_plain("300 Hz"), Some(300.0));
        assert_eq!(p.params[1].parse_plain("9999"), Some(500.0));
        // drv drives one param (0..10): typed in that param's unit
        assert_eq!(p.macro_from_text(1, "5"), Some(0.5));
        assert_eq!(p.macro_from_text(1, "20"), Some(1.0));
        // character drives two: percent, like its display
        assert_eq!(p.macro_from_text(0, "25%"), Some(0.25));
        assert_eq!(p.macro_from_text(0, "abc"), None);
    }

    /// Each case: (replace this, with that, expected error substring).
    #[test]
    fn rejects_mistakes_with_useful_messages() {
        for (from, to, msg) in [
            ("ratio = 4 }", "ratio = 4, treshold = 3 }", "unknown option 'treshold'"),
            (r#"{ threshold = drive, ratio = 4 }"#, "{ ratio = 4 }", "'threshold' is required"),
            ("threshold = drive", r#"threshold = "param.drive""#, "must be a number or a Param"),
            ("Blocks.FETCompressor.new", "Blocks.FetCompressor.new", "'Blocks.FetCompressor' does not exist"),
            (r#"Blocks.LowPass.new("lp""#, r#"Blocks.LowPass.new("c1""#, "duplicate block id 'c1'"),
            ("crossovers = { xover }", "crossovers = { xover, 800 }", "2 crossover(s) need 3 bands"),
            ("mix = 50, branches = { Chain.new(),", "mix = 50, branches = { Chain.new(), Chain.new(),", "'mix' needs exactly 2 branches"),
            (r#"{ from = 16000, to = 1000, curve = "exp" }"#, r#"{ from = 0, to = 1000, curve = "exp" }"#, "non-zero from/to"),
            (r#"curve = "exp""#, r#"curve = "log""#, r#"must be "lin" or "exp""#),
            (r#"unit = "dB", decimals = 1"#, r#"unit = "dB", decimls = 1"#, "unknown option 'decimls'"),
            (r#"Param.new("free",  "Free",  { min = 0, max = 10, default = 5 })"#, r#"Param.new("free",  "Free",  { min = 0, max = 10, default = 50 })"#, "outside"),
            (r#"Param.new("free""#, r#"Param.new("drive""#, "two different params use the id 'drive'"),
            ("macros = { character, drv }", "macros = { character, drv, character }", "listed twice"),
            (":drives(free, { from = 0, to = 10 })", "", "drives nothing"),
            ("local drv = Macro.new(\"drv\", \"Drv\"):drives(free", "local drv = Macro.new(\"drv\", \"Drv\"):drives(drive", "more than one macro"),
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
            ("Blocks.Haas.new('h', { delay = 50 })", "delay must be 0..40 ms"),
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
        let err = load("local x = 1\nlocal p = Param.new(\"a\", \"A\", { min = 0 })\nreturn Patch.new(\"T\")")
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("init.luau:2:"), "{err}");
        assert!(err.contains("'max' is required"), "{err}");
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
        let err = load("Param.new = nil return Patch.new('T')").err().unwrap().to_string();
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
                "local g = Param.new('g', 'G', {{ min = -24, max = 0 }})
                 return Patch.new('T', {{ chain = Chain.new({{ Blocks.Gain.new('out', {{ db = g }}) }}), theme = {theme} }})"
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
             local g = Param.new('g', 'G', { min = -24, max = 0 })
             local m = Macro.new('m', 'M'):drives(g)
             return Patch.new('T', { chain = Chain.new({ Blocks.Gain.new('out', { db = g }) }), macros = { m },
                                     panel = ui.Panel.new({ gain = m }) })",
        );
        std::fs::create_dir_all(d.0.join("components")).unwrap();
        std::fs::write(
            d.0.join("ui.luau"),
            "local Knob = require('./components/knob')
             local M = {}
             M.Panel = Component.define({ name = 'Panel', render = function(p) return Knob.new({ bind = p.gain }) end })
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
             M.Panel = Component.define({ name = 'Panel', render = function(p) return '' end })
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
