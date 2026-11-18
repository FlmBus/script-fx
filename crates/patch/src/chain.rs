//! Declarative effect chain: what `Chain.new { Blocks.<Type>.new(...) }` builds.
//! Only data here; the `dsp` crate compiles it into an audio graph.
//!
//! The Lua prelude already validates option names, types and required fields
//! with line-accurate errors; the strict deserialization here is the backstop.

use serde::{Deserialize, Deserializer};

use crate::{ParamSpec, PatchError, invalid};

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

/// A list of blocks processed in series (accepts `{}` for an empty chain).
#[derive(Debug, Clone, Default)]
pub struct Chain(pub Vec<BlockSpec>);

impl<'de> Deserialize<'de> for Chain {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::list(d).map(Chain)
    }
}

/// Peak and shelving EQs. Gain in dB, q default 0.707.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EqSpec {
    pub freq: Value,
    pub gain: Value,
    pub q: Option<Value>,
}

/// Low/high pass (12 dB/oct). q default 0.707.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterSpec {
    pub freq: Value,
    pub q: Option<Value>,
}

/// Shared by all compressor types (defaults differ per type, see `dsp`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompSpec {
    pub threshold: Value,
    pub ratio: Option<Value>,
    pub attack: Option<Value>,
    pub release: Option<Value>,
    pub knee: Option<Value>,
    pub makeup: Option<Value>,
    /// Adaptive makeup: gives back ~80% of the average gain reduction (≈1.5 s).
    #[serde(default)]
    pub auto_makeup: bool,
    /// Parallel compression wet amount, % (default 100).
    pub mix: Option<Value>,
}

/// Which channel a one-sided block acts on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    #[default]
    Right,
}

/// Which channels a block acts on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channels {
    #[default]
    Both,
    Left,
    Right,
}

/// `Rectifier` mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RectifyMode {
    Half,
    #[default]
    Full,
}

/// LFO waveform (`Tremolo`, `AutoPan`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LfoShape {
    #[default]
    Sine,
    Triangle,
    Square,
}

/// `DynamicEq` direction: cut the band when it gets loud, or boost it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DynamicMode {
    #[default]
    Cut,
    Boost,
}

/// Settings shared by `Reverb` and `Shimmer`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReverbSpec {
    /// Room size, % (default 50).
    pub size: Option<Value>,
    /// RT60, seconds (default 2).
    pub decay: Option<Value>,
    /// High-frequency damping, % (default 50).
    pub damping: Option<Value>,
    /// ms (default 0).
    pub predelay: Option<Value>,
    /// Stereo width, % (default 100).
    pub width: Option<Value>,
    /// Wet amount, % (default 30).
    pub mix: Option<Value>,
    /// Shimmer: pitch of the feedback, semitones (default 12).
    pub pitch: Option<Value>,
    /// Shimmer: amount of pitched feedback, % (default 50).
    pub amount: Option<Value>,
    /// Reverb: > 0.5 = frozen (infinite decay, no new input).
    pub freeze: Option<Value>,
}

/// One block of the chain: what every block has (id, oversampling) plus its kind.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockSpec {
    pub id: String,
    /// Oversampling factor (1, 2, 4, 8) for this block (and everything inside it).
    /// `None` = automatic: nonlinear blocks get enough to run at ≥ 88 kHz (see `dsp`).
    #[serde(default, deserialize_with = "crate::opt_whole_number")]
    pub oversampling: Option<u32>,
    pub kind: BlockKind,
}

/// The kind of block and its settings. Units: dB, Hz, ms, percent (0..100).
/// `type` is the snake_case variant name (`Blocks.FETCompressor` → `fet_compressor`).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BlockKind {
    Gain {
        db: Value,
    },
    PeakEq(EqSpec),
    LowShelf(EqSpec),
    HighShelf(EqSpec),
    LowPass(FilterSpec),
    HighPass(FilterSpec),
    Drive {
        /// Pre-gain into the saturator, dB (0..36).
        amount: Value,
        /// Wet amount, % (default 100).
        mix: Option<Value>,
    },
    /// Plain feed-forward compressor.
    Compressor(CompSpec),
    /// Peak detector, fast attack, hard-ish knee (1176-ish).
    FetCompressor(CompSpec),
    /// RMS detector, soft knee, program-dependent release (LA-2A-ish).
    OptoCompressor(CompSpec),
    /// Slow automatic gain ("fader riding") towards `target` dBFS, within ±`range` dB.
    Rider {
        target: Value,
        range: Value,
        /// Time constant, ms (default 300).
        speed: Option<Value>,
    },
    /// Transient shaper: dB boost/cut of attacks and sustain.
    Transient {
        attack: Value,
        sustain: Option<Value>,
    },
    /// Linkwitz-Riley (4th order) split into `crossovers.len() + 1` bands
    /// (phase-aligned, sums flat); each band is processed, then all are summed.
    BandSplit {
        #[serde(deserialize_with = "crate::list")]
        crossovers: Vec<Value>,
        #[serde(deserialize_with = "crate::list")]
        bands: Vec<Chain>,
    },
    /// Stereo balance, -100 (left) .. 100 (right); the other side is attenuated.
    Pan {
        pan: Value,
    },
    /// Mid/side width, %: 0 = mono, 100 = unchanged, 200 = twice the side signal.
    Width {
        width: Value,
    },
    /// Removes the side signal below `freq` (LR4 high-pass on side): mono bass.
    MonoBass {
        freq: Value,
    },
    /// Delays one channel by `delay` ms (0..40): Haas widening.
    Haas {
        delay: Value,
        #[serde(default)]
        channel: Side,
    },
    /// Polarity inversion.
    Invert {
        #[serde(default)]
        channels: Channels,
    },
    /// Removes DC offset (one-pole high-pass at 5 Hz).
    DcBlock,
    /// Passive level meter at this point of the chain (peak, like the in/out meters).
    Meter,
    /// Encodes to mid/side, runs `mid` and `side` (each in mono), decodes.
    MidSide {
        #[serde(default)]
        mid: Chain,
        #[serde(default)]
        side: Chain,
    },
    /// Runs `left` and `right` on their own channel (each in mono).
    LeftRight {
        #[serde(default)]
        left: Chain,
        #[serde(default)]
        right: Chain,
    },
    // ---------------------------------------------------------------- distortion
    /// Transfer curve sampled evenly over -1..1 (input clipped to that range).
    Waveshaper {
        #[serde(deserialize_with = "crate::list")]
        curve: Vec<f32>,
        /// Pre-gain, dB (default 0).
        drive: Option<Value>,
        /// Post-gain, dB (default 0).
        output: Option<Value>,
        mix: Option<Value>,
    },
    /// Sine wavefolder: `amount` dB pre-gain (0..36), `bias` % (asymmetry).
    Wavefolder {
        amount: Value,
        bias: Option<Value>,
        mix: Option<Value>,
    },
    /// Clips at `ceiling` dB; `softness` % (0 = hard, 100 = tanh-like).
    Clipper {
        ceiling: Option<Value>,
        softness: Option<Value>,
        drive: Option<Value>,
    },
    Rectifier {
        #[serde(default)]
        mode: RectifyMode,
        mix: Option<Value>,
    },
    /// Bit depth (1..24) and sample-and-hold rate (Hz). Aliases on purpose.
    Bitcrusher {
        bits: Option<Value>,
        rate: Option<Value>,
        mix: Option<Value>,
    },
    /// Multiplies with a sine at `freq` Hz.
    RingMod {
        freq: Value,
        mix: Option<Value>,
    },
    /// Tape: asymmetric saturation, head bump, HF loss, wow and flutter.
    Tape {
        drive: Option<Value>,
        wow: Option<Value>,
        flutter: Option<Value>,
        mix: Option<Value>,
    },
    // ---------------------------------------------------------------- filter
    /// State-variable filter morphing LP (0 %) → BP (50 %) → HP (100 %).
    MorphFilter {
        freq: Value,
        morph: Option<Value>,
        resonance: Option<Value>,
    },
    /// 4-pole (24 dB/oct) ladder low-pass with nonlinear stages.
    LadderFilter {
        freq: Value,
        resonance: Option<Value>,
        drive: Option<Value>,
    },
    /// Feedback comb tuned to `freq` Hz (feedback -99..99 %).
    CombFilter {
        freq: Value,
        feedback: Option<Value>,
        mix: Option<Value>,
    },
    /// Vowel filter: `vowel` 0..4 = a, e, i, o, u (continuous), `shift` semitones.
    Formant {
        vowel: Value,
        shift: Option<Value>,
        resonance: Option<Value>,
        mix: Option<Value>,
    },
    /// Resonant band-passes at `notes` (Hz), ringing for `decay` ms.
    Resonator {
        #[serde(deserialize_with = "crate::list")]
        notes: Vec<f32>,
        decay: Option<Value>,
        mix: Option<Value>,
    },
    /// A peak EQ band whose gain follows the band's level above `threshold`.
    DynamicEq {
        freq: Value,
        q: Option<Value>,
        threshold: Value,
        ratio: Option<Value>,
        range: Option<Value>,
        attack: Option<Value>,
        release: Option<Value>,
        #[serde(default)]
        mode: DynamicMode,
    },
    // ---------------------------------------------------------------- modulation
    /// `sync` (all LFO/time blocks) = a note length in beats, overrides `rate`/`time`.
    Chorus {
        rate: Option<Value>,
        sync: Option<f32>,
        depth: Option<Value>,
        mix: Option<Value>,
    },
    Flanger {
        rate: Option<Value>,
        sync: Option<f32>,
        depth: Option<Value>,
        feedback: Option<Value>,
        mix: Option<Value>,
    },
    Phaser {
        rate: Option<Value>,
        sync: Option<f32>,
        depth: Option<Value>,
        feedback: Option<Value>,
        freq: Option<Value>,
        #[serde(default, deserialize_with = "crate::opt_whole_number")]
        stages: Option<u32>,
        mix: Option<Value>,
    },
    /// `phase_offset`: LFO phase shift in degrees. Synced + host playing: the
    /// phase is locked to the host's beat position.
    Tremolo {
        rate: Option<Value>,
        sync: Option<f32>,
        phase_offset: Option<Value>,
        depth: Option<Value>,
        #[serde(default)]
        shape: LfoShape,
    },
    AutoPan {
        rate: Option<Value>,
        sync: Option<f32>,
        phase_offset: Option<Value>,
        depth: Option<Value>,
        #[serde(default)]
        shape: LfoShape,
    },
    // ---------------------------------------------------------------- space
    /// Echo; `feedback_chain` processes every repeat (e.g. a filter or a crusher).
    Delay {
        time: Option<Value>,
        sync: Option<f32>,
        feedback: Option<Value>,
        mix: Option<Value>,
        #[serde(default)]
        pingpong: bool,
        #[serde(default)]
        feedback_chain: Chain,
    },
    Reverb(ReverbSpec),
    Shimmer(ReverbSpec),
    // ---------------------------------------------------------------- pitch
    PitchShift {
        semitones: Value,
        window: Option<Value>,
        mix: Option<Value>,
    },
    /// Grain cloud from the last seconds of input.
    Granular {
        grain: Option<Value>,
        density: Option<Value>,
        spread: Option<Value>,
        pitch: Option<Value>,
        jitter: Option<Value>,
        reverse: Option<Value>,
        width: Option<Value>,
        mix: Option<Value>,
    },
    // ---------------------------------------------------------------- rhythm
    /// While `active` > 0.5: repeats the slice that started then.
    Stutter {
        active: Value,
        length: Option<Value>,
        sync: Option<f32>,
        mix: Option<Value>,
    },
    // ---------------------------------------------------------------- dynamics
    /// Lookahead peak limiter (`lookahead` ms is fixed: it is reported latency).
    Limiter {
        ceiling: Option<Value>,
        release: Option<Value>,
        lookahead: Option<f32>,
    },
    /// Gate / downward expander (`ratio`, default 20 ≈ gate).
    Gate {
        threshold: Value,
        ratio: Option<Value>,
        range: Option<Value>,
        attack: Option<Value>,
        hold: Option<Value>,
        release: Option<Value>,
    },
    /// Reduces the band above `freq` when it exceeds `threshold`.
    DeEsser {
        freq: Option<Value>,
        threshold: Value,
        range: Option<Value>,
    },
    /// Every branch gets the full signal; outputs are summed. With `mix` (exactly
    /// two branches) it crossfades instead: 0 % = first branch only, 100 % = second.
    Parallel {
        #[serde(deserialize_with = "crate::list")]
        branches: Vec<Chain>,
        mix: Option<Value>,
    },
}

impl BlockKind {
    pub fn is_compressor(&self) -> bool {
        matches!(self, BlockKind::Compressor(_) | BlockKind::FetCompressor(_) | BlockKind::OptoCompressor(_))
    }

    /// The meter value (0..1) of a metered block at rest: no gain reduction / no
    /// signal = 0, gain meters sit at unity (0.5). `None` = no meter.
    pub fn idle_meter(&self) -> Option<f32> {
        match self {
            b if b.is_compressor() => Some(0.0),
            BlockKind::Meter
            | BlockKind::DynamicEq { .. }
            | BlockKind::Limiter { .. }
            | BlockKind::Gate { .. }
            | BlockKind::DeEsser { .. } => Some(0.0),
            BlockKind::Rider { .. } | BlockKind::Transient { .. } => Some(0.5),
            _ => None,
        }
    }

    /// Blocks that create new harmonics and alias without oversampling; they are
    /// oversampled automatically unless the patch says otherwise.
    pub fn is_nonlinear(&self) -> bool {
        matches!(
            self,
            BlockKind::Drive { .. }
                | BlockKind::Waveshaper { .. }
                | BlockKind::Wavefolder { .. }
                | BlockKind::Clipper { .. }
                | BlockKind::Rectifier { .. }
                | BlockKind::RingMod { .. }
                | BlockKind::Tape { .. }
                | BlockKind::LadderFilter { .. }
        )
    }

    /// Sub-chains in processing order (bands / branches / mid, side / left, right).
    pub fn sub_chains(&self) -> Vec<&Chain> {
        match self {
            BlockKind::BandSplit { bands, .. } => bands.iter().collect(),
            BlockKind::Parallel { branches, .. } => branches.iter().collect(),
            BlockKind::MidSide { mid, side } => vec![mid, side],
            BlockKind::LeftRight { left, right } => vec![left, right],
            BlockKind::Delay { feedback_chain, .. } => vec![feedback_chain],
            _ => Vec::new(),
        }
    }

    fn values(&self) -> Vec<&Value> {
        let mut out = Vec::new();
        fn opt(v: &Option<Value>) -> Option<&Value> {
            v.as_ref()
        }
        match self {
            BlockKind::Gain { db } => out.push(db),
            BlockKind::PeakEq(e) | BlockKind::LowShelf(e) | BlockKind::HighShelf(e) => {
                out.extend([Some(&e.freq), Some(&e.gain), opt(&e.q)].into_iter().flatten())
            }
            BlockKind::LowPass(f) | BlockKind::HighPass(f) => out.extend([Some(&f.freq), opt(&f.q)].into_iter().flatten()),
            BlockKind::Drive { amount, mix } => out.extend([Some(amount), opt(mix)].into_iter().flatten()),
            BlockKind::Compressor(c) | BlockKind::FetCompressor(c) | BlockKind::OptoCompressor(c) => out.extend(
                [Some(&c.threshold), opt(&c.ratio), opt(&c.attack), opt(&c.release), opt(&c.knee), opt(&c.makeup), opt(&c.mix)]
                    .into_iter()
                    .flatten(),
            ),
            BlockKind::Rider { target, range, speed } => out.extend([Some(target), Some(range), opt(speed)].into_iter().flatten()),
            BlockKind::Transient { attack, sustain } => out.extend([Some(attack), opt(sustain)].into_iter().flatten()),
            BlockKind::Pan { pan: v } | BlockKind::Width { width: v } | BlockKind::MonoBass { freq: v } | BlockKind::Haas { delay: v, .. } => {
                out.push(v)
            }
            BlockKind::Waveshaper { drive, output, mix, .. } => out.extend([opt(drive), opt(output), opt(mix)].into_iter().flatten()),
            BlockKind::Wavefolder { amount, bias, mix } => out.extend([Some(amount), opt(bias), opt(mix)].into_iter().flatten()),
            BlockKind::Clipper { ceiling, softness, drive } => out.extend([opt(ceiling), opt(softness), opt(drive)].into_iter().flatten()),
            BlockKind::Rectifier { mix, .. } => out.extend(opt(mix)),
            BlockKind::Bitcrusher { bits, rate, mix } => out.extend([opt(bits), opt(rate), opt(mix)].into_iter().flatten()),
            BlockKind::RingMod { freq, mix } => out.extend([Some(freq), opt(mix)].into_iter().flatten()),
            BlockKind::Tape { drive, wow, flutter, mix } => out.extend([opt(drive), opt(wow), opt(flutter), opt(mix)].into_iter().flatten()),
            BlockKind::MorphFilter { freq, morph, resonance } => out.extend([Some(freq), opt(morph), opt(resonance)].into_iter().flatten()),
            BlockKind::LadderFilter { freq, resonance, drive } => out.extend([Some(freq), opt(resonance), opt(drive)].into_iter().flatten()),
            BlockKind::CombFilter { freq, feedback, mix } => out.extend([Some(freq), opt(feedback), opt(mix)].into_iter().flatten()),
            BlockKind::Formant { vowel, shift, resonance, mix } => {
                out.extend([Some(vowel), opt(shift), opt(resonance), opt(mix)].into_iter().flatten())
            }
            BlockKind::Resonator { decay, mix, .. } => out.extend([opt(decay), opt(mix)].into_iter().flatten()),
            BlockKind::DynamicEq { freq, q, threshold, ratio, range, attack, release, .. } => out.extend(
                [Some(freq), opt(q), Some(threshold), opt(ratio), opt(range), opt(attack), opt(release)].into_iter().flatten(),
            ),
            BlockKind::Chorus { rate, depth, mix, .. } => out.extend([opt(rate), opt(depth), opt(mix)].into_iter().flatten()),
            BlockKind::Flanger { rate, depth, feedback, mix, .. } => {
                out.extend([opt(rate), opt(depth), opt(feedback), opt(mix)].into_iter().flatten())
            }
            BlockKind::Phaser { rate, depth, feedback, freq, mix, .. } => {
                out.extend([opt(rate), opt(depth), opt(feedback), opt(freq), opt(mix)].into_iter().flatten())
            }
            BlockKind::Tremolo { rate, phase_offset, depth, .. } | BlockKind::AutoPan { rate, phase_offset, depth, .. } => {
                out.extend([opt(rate), opt(phase_offset), opt(depth)].into_iter().flatten())
            }
            BlockKind::Delay { time, feedback, mix, .. } => out.extend([opt(time), opt(feedback), opt(mix)].into_iter().flatten()),
            BlockKind::Reverb(r) | BlockKind::Shimmer(r) => out.extend(
                [opt(&r.size), opt(&r.decay), opt(&r.damping), opt(&r.predelay), opt(&r.width), opt(&r.mix), opt(&r.pitch), opt(&r.amount), opt(&r.freeze)]
                    .into_iter()
                    .flatten(),
            ),
            BlockKind::PitchShift { semitones, window, mix } => out.extend([Some(semitones), opt(window), opt(mix)].into_iter().flatten()),
            BlockKind::Granular { grain, density, spread, pitch, jitter, reverse, width, mix } => out.extend(
                [opt(grain), opt(density), opt(spread), opt(pitch), opt(jitter), opt(reverse), opt(width), opt(mix)].into_iter().flatten(),
            ),
            BlockKind::Stutter { active, length, mix, .. } => out.extend([Some(active), opt(length), opt(mix)].into_iter().flatten()),
            BlockKind::Limiter { ceiling, release, .. } => out.extend([opt(ceiling), opt(release)].into_iter().flatten()),
            BlockKind::Gate { threshold, ratio, range, attack, hold, release } => out.extend(
                [Some(threshold), opt(ratio), opt(range), opt(attack), opt(hold), opt(release)].into_iter().flatten(),
            ),
            BlockKind::DeEsser { freq, threshold, range } => out.extend([opt(freq), Some(threshold), opt(range)].into_iter().flatten()),
            BlockKind::Invert { .. }
            | BlockKind::DcBlock
            | BlockKind::Meter
            | BlockKind::MidSide { .. }
            | BlockKind::LeftRight { .. } => {}
            BlockKind::BandSplit { crossovers, .. } => out.extend(crossovers),
            BlockKind::Parallel { mix, .. } => out.extend(opt(mix)),
        }
        out
    }
}

impl BlockSpec {
    pub fn has_meter(&self) -> bool {
        self.kind.idle_meter().is_some()
    }
}

/// Visits blocks depth-first: a block, then its sub-chains in order.
/// The DSP graph assigns meter slots in this same order.
pub fn walk<'a>(chain: &'a [BlockSpec], f: &mut impl FnMut(&'a BlockSpec)) {
    for b in chain {
        f(b);
        for c in b.kind.sub_chains() {
            walk(&c.0, f);
        }
    }
}

pub fn resolve_value(v: &Value, params: &[ParamSpec]) -> Result<ValueRef, PatchError> {
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

/// Highest oversampling, also for nested oversampled blocks multiplied together.
pub const MAX_OVERSAMPLING: u32 = 8;

/// Checks ids are unique, all `param.*` references resolve, splits are well-formed
/// and nested oversampling stays within [`MAX_OVERSAMPLING`].
/// Returns the ids of metered blocks in [`walk`] order.
pub fn validate(chain: &[BlockSpec], params: &[ParamSpec]) -> Result<Vec<String>, PatchError> {
    let mut ids = std::collections::HashSet::new();
    let mut meters = Vec::new();
    validate_chain(chain, params, 1, &mut ids, &mut meters)?;
    Ok(meters)
}

fn validate_chain(
    chain: &[BlockSpec],
    params: &[ParamSpec],
    oversampling: u32,
    ids: &mut std::collections::HashSet<String>,
    meters: &mut Vec<String>,
) -> Result<(), PatchError> {
    for b in chain {
        let id = b.id.as_str();
        if !ids.insert(id.to_string()) {
            return Err(invalid(format!("duplicate block id '{id}'")));
        }
        if matches!(id, "in" | "out") && b.has_meter() {
            return Err(invalid("metered blocks can't be called 'in' or 'out' (reserved meters)"));
        }
        match &b.kind {
            BlockKind::BandSplit { crossovers, bands } if crossovers.is_empty() || bands.len() != crossovers.len() + 1 => {
                return Err(invalid(format!("band split '{id}': needs n crossovers and n+1 bands")));
            }
            BlockKind::Parallel { branches, mix } if branches.len() < 2 || (mix.is_some() && branches.len() != 2) => {
                return Err(invalid(format!("parallel '{id}': needs ≥ 2 branches, exactly 2 with mix")));
            }
            _ => {}
        }
        for v in b.kind.values() {
            resolve_value(v, params)?;
        }
        let os = match b.oversampling {
            None => oversampling,
            Some(f @ (1 | 2 | 4 | 8)) => oversampling * f,
            Some(f) => return Err(invalid(format!("block '{id}': oversampling must be 1, 2, 4 or 8 (got {f})"))),
        };
        if os > MAX_OVERSAMPLING {
            return Err(invalid(format!(
                "block '{id}': nested oversampling multiplies to {os}x (max {MAX_OVERSAMPLING}x); oversample either the outer or the inner block"
            )));
        }
        if b.has_meter() {
            meters.push(id.to_string());
        }
        for c in b.kind.sub_chains() {
            validate_chain(&c.0, params, os, ids, meters)?;
        }
    }
    Ok(())
}
