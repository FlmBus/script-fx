//! Declarative effect chain: what the preset's `chain = { ... }` describes.
//! Only data here; the `dsp` crate compiles it into an audio graph.

use serde::Deserialize;

use crate::{ParamSpec, PresetError, invalid};

/// A block setting: a constant or `"param.<id>"` (the param's plain value is used,
/// so its unit must match the setting's unit).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Const(f32),
    Ref(String),
}

/// Resolved [`Value`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueRef {
    Const(f32),
    Param(usize),
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EqShape {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CompStyle {
    /// Peak detector, fast attack, hard-ish knee (1176-ish).
    Fet,
    /// RMS detector, soft knee, program-dependent release (LA-2A-ish).
    Opto,
    /// Plain feed-forward compressor.
    #[default]
    Clean,
}

/// One block of the chain. Units: dB, Hz, ms, percent (0..100).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BlockSpec {
    Gain {
        id: String,
        db: Value,
    },
    Eq {
        id: String,
        #[serde(rename = "type")]
        shape: EqShape,
        freq: Value,
        q: Option<Value>,
        /// Only for peak/shelves.
        gain: Option<Value>,
    },
    Drive {
        id: String,
        /// Pre-gain into the saturator, dB (0..36).
        amount: Value,
        /// Wet amount, % (default 100).
        mix: Option<Value>,
    },
    Comp {
        id: String,
        #[serde(default)]
        style: CompStyle,
        threshold: Value,
        ratio: Option<Value>,
        attack: Option<Value>,
        release: Option<Value>,
        knee: Option<Value>,
        makeup: Option<Value>,
        /// Adaptive makeup: gives back ~80% of the average gain reduction (≈1.5 s).
        #[serde(default)]
        auto_makeup: bool,
        /// Parallel compression wet amount, % (default 100).
        mix: Option<Value>,
    },
    /// Slow automatic gain ("fader riding") towards `target` dBFS, within ±`range` dB.
    Rider {
        id: String,
        target: Value,
        range: Value,
        /// Time constant, ms (default 300).
        speed: Option<Value>,
    },
    /// Transient shaper: dB boost/cut of attacks and sustain.
    Transient {
        id: String,
        attack: Value,
        sustain: Option<Value>,
    },
    /// Linkwitz-Riley (4th order) band split; bands are processed and summed.
    Split {
        id: String,
        freq: Value,
        #[serde(default, deserialize_with = "crate::list")]
        low: Vec<BlockSpec>,
        #[serde(default, deserialize_with = "crate::list")]
        high: Vec<BlockSpec>,
    },
}

impl BlockSpec {
    pub fn id(&self) -> &str {
        match self {
            BlockSpec::Gain { id, .. }
            | BlockSpec::Eq { id, .. }
            | BlockSpec::Drive { id, .. }
            | BlockSpec::Comp { id, .. }
            | BlockSpec::Rider { id, .. }
            | BlockSpec::Transient { id, .. }
            | BlockSpec::Split { id, .. } => id,
        }
    }

    /// Blocks that report a meter value (`meter.<id>` in the UI).
    pub fn has_meter(&self) -> bool {
        matches!(self, BlockSpec::Comp { .. } | BlockSpec::Rider { .. } | BlockSpec::Transient { .. })
    }

    fn values(&self) -> Vec<&Value> {
        fn opt(v: &Option<Value>) -> Option<&Value> {
            v.as_ref()
        }
        match self {
            BlockSpec::Gain { db, .. } => vec![db],
            BlockSpec::Eq { freq, q, gain, .. } => [Some(freq), opt(q), opt(gain)].into_iter().flatten().collect(),
            BlockSpec::Drive { amount, mix, .. } => [Some(amount), opt(mix)].into_iter().flatten().collect(),
            BlockSpec::Comp { threshold, ratio, attack, release, knee, makeup, mix, .. } => {
                [Some(threshold), opt(ratio), opt(attack), opt(release), opt(knee), opt(makeup), opt(mix)]
                    .into_iter()
                    .flatten()
                    .collect()
            }
            BlockSpec::Rider { target, range, speed, .. } => {
                [Some(target), Some(range), opt(speed)].into_iter().flatten().collect()
            }
            BlockSpec::Transient { attack, sustain, .. } => [Some(attack), opt(sustain)].into_iter().flatten().collect(),
            BlockSpec::Split { freq, .. } => vec![freq],
        }
    }
}

/// Visits blocks depth-first (split: the split itself, then `low`, then `high`).
/// The DSP graph assigns meter slots in this same order.
pub fn walk<'a>(chain: &'a [BlockSpec], f: &mut impl FnMut(&'a BlockSpec)) {
    for b in chain {
        f(b);
        if let BlockSpec::Split { low, high, .. } = b {
            walk(low, f);
            walk(high, f);
        }
    }
}

pub fn resolve_value(v: &Value, params: &[ParamSpec]) -> Result<ValueRef, PresetError> {
    match v {
        Value::Const(c) => Ok(ValueRef::Const(*c)),
        Value::Ref(s) => {
            let id = s
                .strip_prefix("param.")
                .ok_or_else(|| invalid(format!("block setting '{s}' must be a number or \"param.<id>\"")))?;
            params
                .iter()
                .position(|p| p.id == id)
                .map(ValueRef::Param)
                .ok_or_else(|| invalid(format!("block setting refers to unknown param '{id}'")))
        }
    }
}

/// Checks ids are unique and all `param.*` references resolve. Returns the
/// ids of metered blocks in [`walk`] order.
pub fn validate(chain: &[BlockSpec], params: &[ParamSpec]) -> Result<Vec<String>, PresetError> {
    let mut ids = std::collections::HashSet::new();
    let mut meters = Vec::new();
    let mut err = None;
    walk(chain, &mut |b| {
        if err.is_some() {
            return;
        }
        if !ids.insert(b.id().to_string()) {
            err = Some(invalid(format!("duplicate block id '{}'", b.id())));
            return;
        }
        if matches!(b.id(), "in" | "out") && b.has_meter() {
            err = Some(invalid("metered blocks can't be called 'in' or 'out' (reserved meters)"));
            return;
        }
        for v in b.values() {
            if let Err(e) = resolve_value(v, params) {
                err = Some(e);
                return;
            }
        }
        if b.has_meter() {
            meters.push(b.id().to_string());
        }
    });
    match err {
        Some(e) => Err(e),
        None => Ok(meters),
    }
}
