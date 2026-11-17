//! Patch model + sandboxed Lua loader.
//!
//! A patch is a folder containing `patch.lua` (returns a table) and whatever
//! assets it references (e.g. `panel.svg`). Lua only *describes* the patch:
//!
//! - `params`: the patch's internal controls (unlimited). The UI and DSP use these.
//! - `macros`: up to [`MACRO_SLOTS`] host-automatable controls. Each macro drives one
//!   or more params, each with its own range and curve (Ableton-rack style).
//!   A param driven by a macro is "locked" to it; all others are free and keep
//!   their own value (persisted with the host project, but not automatable).
//! - `chain`: the effect chain as data (blocks + band splits, see [`chain`]).
//!   Compiled to native DSP by the `dsp` crate; Lua never runs on the audio thread.
//! - `theme` and the `control_panel` (SVG + declarative bindings).
//!
//! The one exception is the optional `control_panel.animate(state)` function, which runs once
//! per UI frame on the UI thread (never on the audio thread).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

pub mod chain;
pub mod locale;
pub use chain::BlockSpec;

use mlua::{Function, HookTriggers, Lua, LuaOptions, LuaSerdeExt, StdLib, Table, VmState};
use serde::Deserialize;

/// Number of host-automatable macro slots the plugin exposes.
pub const MACRO_SLOTS: usize = 16;

/// Instruction budget per Lua call (load or one animate frame).
const INSTRUCTION_BUDGET: u32 = 200_000;
const HOOK_GRANULARITY: u32 = 1_000;
const MEMORY_LIMIT: usize = 16 * 1024 * 1024;

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
    #[serde(default = "two")]
    pub decimals: usize,
}

fn one() -> f32 {
    1.0
}
fn two() -> usize {
    2
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
    pub fn display_plain(&self, plain: f32) -> String {
        format!("{:.*}{}", self.decimals, plain, self.unit)
    }
    pub fn display(&self, normalized: f32) -> String {
        self.display_plain(self.plain(normalized))
    }
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

/// Where a binding gets its value from.
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
    /// comp = gain reduction 0..24 dB; rider/transient = gain, 0.5 = unity.
    Block(usize),
    /// Seconds since the editor opened.
    Time,
    /// Value returned by the patch's `control_panel.animate` function.
    Var(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Pivot {
    /// Center of the bounding box of the element with this id.
    Element(String),
    Point([f32; 2]),
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    #[default]
    Both,
    X,
    Y,
}

/// Declarative UI bindings. `from`/`to` are what a source value of 0 and 1 map to.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Binding {
    Rotate {
        id: String,
        source: String,
        from: f32,
        to: f32,
        pivot: Option<Pivot>,
    },
    Translate {
        id: String,
        source: String,
        from: [f32; 2],
        to: [f32; 2],
    },
    Scale {
        id: String,
        source: String,
        #[serde(default)]
        from: f32,
        #[serde(default = "one")]
        to: f32,
        #[serde(default)]
        axis: Axis,
        pivot: Option<Pivot>,
    },
    Opacity {
        id: String,
        source: String,
        #[serde(default)]
        from: f32,
        #[serde(default = "one")]
        to: f32,
    },
    /// Colors are `#rrggbb` or `$themekey`.
    Fill {
        id: String,
        source: String,
        from: String,
        to: String,
    },
    /// Param sources (and single-target macros) render with the param's
    /// unit/decimals; others use `format` (printf subset: `%.Nf`, `%d`, `%%`).
    Text {
        id: String,
        source: String,
        format: Option<String>,
    },
    /// Vertical drag on this element edits `target` (`macro.<id>` or `param.<id>`).
    Drag {
        id: String,
        target: String,
        /// SVG units for a full 0..1 sweep.
        #[serde(default = "default_drag_range")]
        range: f32,
    },
}

fn default_drag_range() -> f32 {
    200.0
}

impl Binding {
    pub fn element_id(&self) -> &str {
        match self {
            Binding::Rotate { id, .. }
            | Binding::Translate { id, .. }
            | Binding::Scale { id, .. }
            | Binding::Opacity { id, .. }
            | Binding::Fill { id, .. }
            | Binding::Text { id, .. }
            | Binding::Drag { id, .. } => id,
        }
    }
    pub fn source(&self) -> Option<&str> {
        match self {
            Binding::Rotate { source, .. }
            | Binding::Translate { source, .. }
            | Binding::Scale { source, .. }
            | Binding::Opacity { source, .. }
            | Binding::Fill { source, .. }
            | Binding::Text { source, .. } => Some(source),
            Binding::Drag { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Theme(pub BTreeMap<String, String>);

impl Theme {
    /// Resolves `$key` references; anything else is returned unchanged.
    pub fn resolve<'a>(&'a self, color: &'a str) -> &'a str {
        match color.strip_prefix('$') {
            Some(key) => self.0.get(key).map(String::as_str).unwrap_or("#ff00ff"),
            None => color,
        }
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }
}

#[derive(Debug, Clone)]
pub struct ControlPanelSpec {
    pub svg: PathBuf,
    pub bindings: Vec<Binding>,
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
    pub vars: HashMap<String, f32>,
}

impl Values {
    pub fn get(&self, source: &Source) -> f32 {
        match source {
            Source::Macro(i) => self.macros[*i],
            Source::Param(i) => self.params.get(*i).copied().unwrap_or(0.0),
            Source::MeterIn => self.meter_in,
            Source::MeterOut => self.meter_out,
            Source::Block(i) => self.blocks.get(*i).copied().unwrap_or(0.0),
            Source::Time => self.time,
            Source::Var(name) => self.vars.get(name).copied().unwrap_or(0.0),
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
    /// Ids of metered chain blocks (comp/rider/transient), in DSP meter-slot order.
    pub meters: Vec<String>,
    pub theme: Theme,
    pub control_panel: Option<ControlPanelSpec>,
    animator: Option<Animator>,
}

struct Animator {
    lua: Lua,
    func: Function,
    used: Arc<AtomicU32>,
}

const PRELUDE: &str = r#"
local function binding(kind)
  return function(id, opts)
    local t = opts or {}
    t.kind = kind
    t.id = id
    return t
  end
end
rotate    = binding("rotate")
translate = binding("translate")
scale     = binding("scale")
opacity   = binding("opacity")
fill      = binding("fill")
text      = binding("text")
drag      = binding("drag")

-- chain blocks
gain      = binding("gain")
eq        = binding("eq")
drive     = binding("drive")
comp      = binding("comp")
rider     = binding("rider")
transient = binding("transient")
split     = binding("split")
"#;

impl Patch {
    /// Loads `<dir>/patch.lua`.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, PatchError> {
        let _c = locale::CNumeric::enter();
        let dir = dir.as_ref().to_path_buf();
        let src = std::fs::read_to_string(dir.join("patch.lua"))?;
        let (lua, used) = sandboxed_lua()?;
        lua.load(PRELUDE).set_name("prelude").exec()?;

        used.store(0, Ordering::Relaxed);
        let root: Table = lua.load(&src).set_name("patch.lua").eval()?;

        let name: String = root.get("name")?;
        let author: Option<String> = root.get("author")?;
        let params: Vec<ParamSpec> = opt_from_lua(&lua, root.get("params")?)?;
        let macros: Vec<MacroSpec> = opt_from_lua(&lua, root.get("macros")?)?;
        let theme = Theme(opt_from_lua(&lua, root.get("theme")?)?);

        let mut animator = None;
        let ui = match root.get::<Option<Table>>("control_panel")? {
            None => None,
            Some(ui) => {
                let svg: String = ui.get("svg")?;
                let svg = resolve_inside(&dir, &svg)?;
                let bindings: Vec<Binding> = opt_from_lua(&lua, ui.get("bindings")?)?;
                animator = ui.get::<Option<Function>>("animate")?;
                Some(ControlPanelSpec { svg, bindings })
            }
        };

        let controls = resolve_controls(&params, &macros)?;
        let chain: Vec<BlockSpec> = opt_from_lua(&lua, root.get("chain")?)?;
        let meters = chain::validate(&chain, &params)?;
        let patch = Patch {
            name,
            author,
            dir,
            params,
            macros,
            controls,
            chain,
            meters,
            theme,
            control_panel: ui,
            animator: animator.map(|func| Animator { lua, func, used }),
        };
        patch.validate_bindings()?;
        Ok(patch)
    }

    fn validate_bindings(&self) -> Result<(), PatchError> {
        let Some(ui) = &self.control_panel else { return Ok(()) };
        for b in &ui.bindings {
            if let Some(src) = b.source() {
                self.resolve_source(src)?;
            }
            if let Binding::Drag { target, .. } = b {
                match self.resolve_source(target)? {
                    Source::Macro(_) | Source::Param(_) => {}
                    _ => return Err(invalid(format!("drag target '{target}' must be macro.* or param.*"))),
                }
            }
        }
        Ok(())
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
            Some(("var", name)) => Source::Var(name.to_string()),
            None if s == "time" => Source::Time,
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

    pub fn has_animator(&self) -> bool {
        self.animator.is_some()
    }

    /// Runs `control_panel.animate(state)` and stores the returned numbers in `values.vars`.
    /// `state` = { time, dt, macro = {<id> = 0..1}, param = {<id> = 0..1}, meter = {in, out} }.
    pub fn animate(&self, values: &mut Values, dt: f32) -> Result<(), PatchError> {
        let Some(anim) = &self.animator else {
            return Ok(());
        };
        let _c = locale::CNumeric::enter();
        let lua = &anim.lua;
        let state = lua.create_table()?;
        state.set("time", values.time)?;
        state.set("dt", dt)?;
        let macros = lua.create_table()?;
        for (i, m) in self.macros.iter().enumerate() {
            macros.set(m.id.as_str(), values.macros[i])?;
        }
        state.set("macro", macros)?;
        let params = lua.create_table()?;
        for (i, p) in self.params.iter().enumerate() {
            params.set(p.id.as_str(), values.params.get(i).copied().unwrap_or(0.0))?;
        }
        state.set("param", params)?;
        let meter = lua.create_table()?;
        meter.set("in", values.meter_in)?;
        meter.set("out", values.meter_out)?;
        for (i, id) in self.meters.iter().enumerate() {
            meter.set(id.as_str(), values.blocks.get(i).copied().unwrap_or(0.0))?;
        }
        state.set("meter", meter)?;

        anim.used.store(0, Ordering::Relaxed);
        let out: Option<Table> = anim.func.call(state)?;
        values.vars.clear();
        if let Some(out) = out {
            for pair in out.pairs::<String, f32>() {
                let (k, v) = pair?;
                values.vars.insert(k, v);
            }
        }
        Ok(())
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

    let mut controls = vec![Control::Free; params.len()];
    for (slot, m) in macros.iter().enumerate() {
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

/// A sandboxed Lua state (table/string/math/utf8 only, no file loading, memory
/// cap, instruction budget). The returned counter must be reset to 0 before each
/// top-level call; the budget applies per call (checked every `HOOK_GRANULARITY`
/// instructions).
pub fn sandboxed_lua() -> Result<(Lua, Arc<AtomicU32>), PatchError> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
        LuaOptions::default(),
    )?;
    lua.set_memory_limit(MEMORY_LIMIT)?;
    let globals = lua.globals();
    for name in ["dofile", "loadfile", "load", "require", "collectgarbage"] {
        globals.set(name, mlua::Value::Nil)?;
    }
    let used = Arc::new(AtomicU32::new(0));
    let counter = used.clone();
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(HOOK_GRANULARITY),
        move |_, _| {
            let n = counter.fetch_add(HOOK_GRANULARITY, Ordering::Relaxed);
            if n > INSTRUCTION_BUDGET {
                Err(mlua::Error::runtime("instruction budget exceeded"))
            } else {
                Ok(VmState::Continue)
            }
        },
    )?;
    Ok((lua, used))
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

/// Lists patch folders (those containing `patch.lua`) in `root`, sorted.
pub fn discover(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("patch.lua").is_file())
        .collect();
    out.sort();
    out
}

/// Minimal printf: `%.Nf`, `%d`, `%%`.
pub fn format_value(fmt: &str, v: f32) -> String {
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('%') => {
                chars.next();
                out.push('%');
            }
            Some('d') => {
                chars.next();
                out.push_str(&format!("{}", v.round() as i64));
            }
            Some('.') => {
                chars.next();
                let mut digits = String::new();
                while let Some(d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                    digits.push(*d);
                    chars.next();
                }
                if chars.peek() == Some(&'f') {
                    chars.next();
                }
                let p = digits.parse().unwrap_or(2);
                out.push_str(&format!("{v:.p$}"));
            }
            Some('f') => {
                chars.next();
                out.push_str(&format!("{v:.2}"));
            }
            _ => out.push('%'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn with_patch(src: &str) -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "patch-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("patch.lua"), src).unwrap();
            std::fs::write(p.join("panel.svg"), "<svg/>").unwrap();
            TempDir(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const BASE: &str = r##"
        local angle = 0
        return {
          name = "T",
          params = {
            { id = "drive", name = "Drive", min = 0, max = 24, unit = " dB", decimals = 1, default = 6 },
            { id = "tone",  name = "Tone",  min = 500, max = 20000, unit = " Hz", decimals = 0 },
            { id = "free",  name = "Free",  min = 0, max = 10, default = 5 },
          },
          macros = {
            { id = "character", name = "Character", default = 0.5, targets = {
              { param = "drive", from = 0, to = 12 },
              { param = "tone", from = 16000, to = 1000, curve = "exp" },
            } },
            { id = "drv", name = "Drv", targets = { { param = "free", from = 0, to = 10 } } },
          },
          theme = { accent = "#ff0000" },
          chain = {
            comp("c1", { style = "fet", threshold = "param.drive", ratio = 4 }),
            split("s", { freq = 250,
              low  = { rider("r", { target = -18, range = 6 }) },
              high = { drive("d", { amount = "param.drive" }) },
            }),
            eq("e", { type = "peak", freq = 100, gain = "param.free" }),
          },
          control_panel = {
            svg = "panel.svg",
            bindings = {
              rotate("needle", { source = "param.drive", from = -135, to = 135, pivot = "knob" }),
              rotate("reel", { source = "var.reel", from = 0, to = 360, pivot = { 10, 20 } }),
              drag("knob", { target = "macro.character" }),
              drag("free_knob", { target = "param.free" }),
            },
            animate = function(s)
              angle = angle + s.dt
              return { reel = angle, c = s.macro.character, d = s.param.drive }
            end,
          },
        }"##;

    #[test]
    fn macros_drive_params_with_ranges_and_curves() {
        let d = TempDir::with_patch(BASE);
        let p = Patch::load(&d.0).unwrap();
        assert_eq!(p.meters, vec!["c1", "r"]);
        assert_eq!(p.resolve_source("meter.r").unwrap(), Source::Block(1));
        assert_eq!(p.chain.len(), 3);
        assert_eq!(p.controls[2], Control::Macro { slot: 1, from: 0.0, to: 10.0, curve: Curve::Lin });
        let mut macros = [0.0; MACRO_SLOTS];
        macros[0] = 0.5;
        let free = p.default_param_values();
        assert_eq!(p.param_plain(0, &macros, &free), 6.0); // lin 0..12
        assert!((p.param_plain(1, &macros, &free) - 4000.0).abs() < 1.0); // exp 16k..1k
        assert_eq!(p.param_plain(2, &macros, &free), 0.0); // driven by macro 1 (= 0)
        assert_eq!(p.macro_default(0), 0.5); // explicit
        assert_eq!(p.macro_default(1), 0.5); // derived from param default 5 of 0..10

        let mut v = Values { macros, ..Default::default() };
        p.compute_params(&v.macros.clone(), &free, &mut v.params);
        assert_eq!(p.params[0].display(v.params[0]), "6.0 dB");
        assert_eq!(p.macro_display(0, &v), "50%");
        p.animate(&mut v, 0.25).unwrap();
        p.animate(&mut v, 0.25).unwrap();
        assert_eq!(v.vars["reel"], 0.5);
        assert_eq!(v.vars["c"], 0.5);
        assert_eq!(v.vars["d"], 0.25);
    }

    #[test]
    fn rejects_bad_mappings_sources_and_escapes() {
        for bad in [
            BASE.replace(r#"param = "drive""#, r#"param = "nope""#),
            BASE.replace(r#"source = "param.drive""#, r#"source = "param.nope""#),
            BASE.replace(r#"target = "macro.character""#, r#"target = "meter.out""#),
            BASE.replace(r#"from = 16000, to = 1000"#, r#"from = 0, to = 1000"#),
            BASE.replace(r#"svg = "panel.svg""#, r#"svg = "../../etc/passwd""#),
            BASE.replace(r#"amount = "param.drive""#, r#"amount = "param.nope""#),
            BASE.replace(r#"rider("r""#, r#"rider("c1""#),
            BASE.replace(r#"type = "peak""#, r#"type = "bandwobble""#),
            BASE.replace(r#"ratio = 4 }"#, r#"ratio = 4, treshold = 3 }"#),
            BASE.replace(r#"unit = " dB", decimals = 1"#, r#"unit = " dB", decimls = 1"#),
            BASE.replace(
                r#"{ param = "tone""#,
                r#"{ param = "drive" }, { param = "tone""#,
            ),
        ] {
            let d = TempDir::with_patch(&bad);
            assert!(Patch::load(&d.0).is_err(), "should fail:\n{bad}");
        }
    }

    #[test]
    fn sandbox_blocks_io_and_infinite_loops() {
        let d = TempDir::with_patch(r#"io.open("/etc/passwd") return { name = "T" }"#);
        assert!(Patch::load(&d.0).is_err());
        let d = TempDir::with_patch(r#"while true do end"#);
        let err = Patch::load(&d.0).err().unwrap().to_string();
        assert!(err.contains("budget"), "{err}");
        let d = TempDir::with_patch(
            r#"return { name = "T", control_panel = { svg = "panel.svg", animate = function() while true do end end } }"#,
        );
        let p = Patch::load(&d.0).unwrap();
        assert!(p.animate(&mut Values::default(), 0.0).is_err());
    }

    #[test]
    fn printf_subset() {
        assert_eq!(format_value("%.1f dB", 3.14159), "3.1 dB");
        assert_eq!(format_value("%d%%", 42.4), "42%");
    }
}
