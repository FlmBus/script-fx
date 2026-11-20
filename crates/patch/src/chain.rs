//! Declarative effect chain: what `Chain.new { Blocks.<Type>.new(...) }` builds.
//! Only data here; the `dsp` crate compiles it into an audio graph.
//!
//! The Lua prelude already validates option names, types and required fields
//! with line-accurate errors; the strict deserialization here is the backstop.

use serde::{Deserialize, Deserializer};

use crate::{ParamSpec, PatchError, invalid};

/// A block setting: the index of its param in [`crate::Patch::params`]. Every numeric setting
/// is a param of its own block (`"<block id>.<setting>"`, see the prelude), so a setting always
/// has one; the loader assigns the indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamRef(pub usize);

impl<'de> Deserialize<'de> for ParamRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::whole_number(d).map(ParamRef)
    }
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
    pub freq: ParamRef,
    pub gain: ParamRef,
    pub q: ParamRef,
}

/// Low/high pass (12 dB/oct). q default 0.707.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterSpec {
    pub freq: ParamRef,
    pub q: ParamRef,
}

/// Shared by all compressor types (defaults differ per type, see `dsp`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompSpec {
    pub threshold: ParamRef,
    pub ratio: ParamRef,
    pub attack: ParamRef,
    pub release: ParamRef,
    pub knee: ParamRef,
    pub makeup: ParamRef,
    /// Adaptive makeup: gives back ~80% of the average gain reduction (≈1.5 s).
    #[serde(default)]
    pub auto_makeup: bool,
    /// Parallel compression wet amount, % (default 100).
    pub mix: ParamRef,
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
    pub size: ParamRef,
    /// RT60, seconds (default 2).
    pub decay: ParamRef,
    /// High-frequency damping, % (default 50).
    pub damping: ParamRef,
    /// ms (default 0).
    pub predelay: ParamRef,
    /// Stereo width, % (default 100).
    pub width: ParamRef,
    /// Wet amount, % (default 30).
    pub mix: ParamRef,
    /// Shimmer: pitch of the feedback, semitones (default 12).
    pub pitch: Option<ParamRef>,
    /// Shimmer: amount of pitched feedback, % (default 50).
    pub amount: Option<ParamRef>,
    /// Reverb: > 0.5 = frozen (infinite decay, no new input).
    pub freeze: Option<ParamRef>,
}

/// One block of the chain: what every block has (id, oversampling) plus its kind.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockSpec {
    pub id: String,
    /// The block's knobs, in the order its type declares them (block page, chain view).
    #[serde(default, deserialize_with = "crate::list")]
    pub params: Vec<ParamRef>,
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
        db: ParamRef,
    },
    PeakEq(EqSpec),
    LowShelf(EqSpec),
    HighShelf(EqSpec),
    LowPass(FilterSpec),
    HighPass(FilterSpec),
    Drive {
        /// Pre-gain into the saturator, dB (0..36).
        amount: ParamRef,
        /// Wet amount, % (default 100).
        mix: ParamRef,
    },
    /// Plain feed-forward compressor.
    Compressor(CompSpec),
    /// Peak detector, fast attack, hard-ish knee (1176-ish).
    FetCompressor(CompSpec),
    /// RMS detector, soft knee, program-dependent release (LA-2A-ish).
    OptoCompressor(CompSpec),
    /// Slow automatic gain ("fader riding") towards `target` dBFS, within ±`range` dB.
    Rider {
        target: ParamRef,
        range: ParamRef,
        /// Time constant, ms (default 300).
        speed: ParamRef,
    },
    /// Transient shaper: dB boost/cut of attacks and sustain.
    Transient {
        attack: ParamRef,
        sustain: ParamRef,
    },
    /// Linkwitz-Riley (4th order) split into `crossovers.len() + 1` bands
    /// (phase-aligned, sums flat); each band is processed, then all are summed.
    BandSplit {
        #[serde(deserialize_with = "crate::list")]
        crossovers: Vec<ParamRef>,
        #[serde(deserialize_with = "crate::list")]
        bands: Vec<Chain>,
    },
    /// Stereo balance, -100 (left) .. 100 (right); the other side is attenuated.
    Pan {
        pan: ParamRef,
    },
    /// Mid/side width, %: 0 = mono, 100 = unchanged, 200 = twice the side signal.
    Width {
        width: ParamRef,
    },
    /// Removes the side signal below `freq` (LR4 high-pass on side): mono bass.
    MonoBass {
        freq: ParamRef,
    },
    /// Delays one channel by `delay` ms (0..40): Haas widening.
    Haas {
        delay: ParamRef,
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
        drive: ParamRef,
        /// Post-gain, dB (default 0).
        output: ParamRef,
        mix: ParamRef,
    },
    /// Sine wavefolder: `amount` dB pre-gain (0..36), `bias` % (asymmetry).
    Wavefolder {
        amount: ParamRef,
        bias: ParamRef,
        mix: ParamRef,
    },
    /// Clips at `ceiling` dB; `softness` % (0 = hard, 100 = tanh-like).
    Clipper {
        ceiling: ParamRef,
        softness: ParamRef,
        drive: ParamRef,
    },
    Rectifier {
        #[serde(default)]
        mode: RectifyMode,
        mix: ParamRef,
    },
    /// Bit depth (1..24) and sample-and-hold rate (Hz). Aliases on purpose.
    Bitcrusher {
        bits: ParamRef,
        rate: Option<ParamRef>,
        mix: ParamRef,
    },
    /// Multiplies with a sine at `freq` Hz.
    RingMod {
        freq: ParamRef,
        mix: ParamRef,
    },
    /// Tape: asymmetric saturation, head bump, HF loss, wow and flutter.
    Tape {
        drive: ParamRef,
        wow: ParamRef,
        flutter: ParamRef,
        mix: ParamRef,
    },
    // ---------------------------------------------------------------- filter
    /// State-variable filter morphing LP (0 %) → BP (50 %) → HP (100 %).
    MorphFilter {
        freq: ParamRef,
        morph: ParamRef,
        resonance: ParamRef,
    },
    /// 4-pole (24 dB/oct) ladder low-pass with nonlinear stages.
    LadderFilter {
        freq: ParamRef,
        resonance: ParamRef,
        drive: ParamRef,
    },
    /// Feedback comb tuned to `freq` Hz (feedback -99..99 %).
    CombFilter {
        freq: ParamRef,
        feedback: ParamRef,
        mix: ParamRef,
    },
    /// Vowel filter: `vowel` 0..4 = a, e, i, o, u (continuous), `shift` semitones.
    Formant {
        vowel: ParamRef,
        shift: ParamRef,
        resonance: ParamRef,
        mix: ParamRef,
    },
    /// Resonant band-passes at `notes` (Hz), ringing for `decay` ms.
    Resonator {
        #[serde(deserialize_with = "crate::list")]
        notes: Vec<f32>,
        decay: ParamRef,
        mix: ParamRef,
    },
    /// A peak EQ band whose gain follows the band's level above `threshold`.
    DynamicEq {
        freq: ParamRef,
        q: ParamRef,
        threshold: ParamRef,
        ratio: ParamRef,
        range: ParamRef,
        attack: ParamRef,
        release: ParamRef,
        #[serde(default)]
        mode: DynamicMode,
    },
    // ---------------------------------------------------------------- modulation
    /// `sync` (all LFO/time blocks) = a note length in beats, overrides `rate`/`time`.
    Chorus {
        rate: ParamRef,
        sync: Option<f32>,
        depth: ParamRef,
        mix: ParamRef,
    },
    Flanger {
        rate: ParamRef,
        sync: Option<f32>,
        depth: ParamRef,
        feedback: ParamRef,
        mix: ParamRef,
    },
    Phaser {
        rate: ParamRef,
        sync: Option<f32>,
        depth: ParamRef,
        feedback: ParamRef,
        freq: ParamRef,
        #[serde(default, deserialize_with = "crate::opt_whole_number")]
        stages: Option<u32>,
        mix: ParamRef,
    },
    /// `phase_offset`: LFO phase shift in degrees. Synced + host playing: the
    /// phase is locked to the host's beat position.
    Tremolo {
        rate: ParamRef,
        sync: Option<f32>,
        phase_offset: ParamRef,
        depth: ParamRef,
        #[serde(default)]
        shape: LfoShape,
    },
    AutoPan {
        rate: ParamRef,
        sync: Option<f32>,
        phase_offset: ParamRef,
        depth: ParamRef,
        #[serde(default)]
        shape: LfoShape,
    },
    // ---------------------------------------------------------------- space
    /// Echo; `feedback_chain` processes every repeat (e.g. a filter or a crusher).
    Delay {
        time: ParamRef,
        sync: Option<f32>,
        feedback: ParamRef,
        mix: ParamRef,
        #[serde(default)]
        pingpong: bool,
        #[serde(default)]
        feedback_chain: Chain,
    },
    Reverb(ReverbSpec),
    Shimmer(ReverbSpec),
    // ---------------------------------------------------------------- pitch
    PitchShift {
        semitones: ParamRef,
        window: ParamRef,
        mix: ParamRef,
    },
    /// Grain cloud from the last seconds of input.
    Granular {
        grain: ParamRef,
        density: ParamRef,
        spread: ParamRef,
        pitch: ParamRef,
        jitter: ParamRef,
        reverse: ParamRef,
        width: ParamRef,
        mix: ParamRef,
    },
    // ---------------------------------------------------------------- rhythm
    /// While `active` > 0.5: repeats the slice that started then.
    Stutter {
        active: ParamRef,
        length: ParamRef,
        sync: Option<f32>,
        mix: ParamRef,
    },
    // ---------------------------------------------------------------- dynamics
    /// Lookahead peak limiter (`lookahead` ms is fixed: it is reported latency).
    Limiter {
        ceiling: ParamRef,
        release: ParamRef,
        lookahead: Option<f32>,
    },
    /// Gate / downward expander (`ratio`, default 20 ≈ gate).
    Gate {
        threshold: ParamRef,
        ratio: ParamRef,
        range: ParamRef,
        attack: ParamRef,
        hold: ParamRef,
        release: ParamRef,
    },
    /// Reduces the band above `freq` when it exceeds `threshold`.
    DeEsser {
        freq: ParamRef,
        threshold: ParamRef,
        range: ParamRef,
    },
    /// Every branch gets the full signal; outputs are summed. With `mix` (exactly
    /// two branches) it crossfades instead: 0 % = first branch only, 100 % = second.
    Parallel {
        #[serde(deserialize_with = "crate::list")]
        branches: Vec<Chain>,
        mix: Option<ParamRef>,
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

/// Highest oversampling, also for nested oversampled blocks multiplied together.
pub const MAX_OVERSAMPLING: u32 = 8;

/// Checks ids are unique, the blocks' params exist, splits are well-formed
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
        if let Some(r) = b.params.iter().find(|r| r.0 >= params.len()) {
            return Err(invalid(format!("block '{id}': param #{} doesn't exist", r.0)));
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
