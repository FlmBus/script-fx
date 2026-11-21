//! The bigger blocks, one module per category. Each implements [`Fx`]; shared
//! building blocks (delay line, LFO, random numbers, smoothing) live here.
//!
//! Like everything on the audio thread: no allocations in `process` (all
//! buffers are sized in the constructors), no locks.

pub(crate) mod distortion;
mod dynamics;

pub use dynamics::comp_static;
mod filter;
mod modulation;
mod pitch;
mod rhythm;
mod space;

use patch::ParamSpec;
use patch::chain::{BlockKind, ParamRef};

use crate::{Ctx, Frame, P};

/// A block that processes stereo frames in place.
pub(crate) trait Fx: Send {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx);
    /// Latency in samples at the rate the block runs at (reported to the host).
    fn latency(&self) -> usize {
        0
    }
}

/// What a constructor needs: params (to resolve settings), the rate it runs at
/// and the most frames per call.
pub(crate) struct Args<'a> {
    pub params: &'a [ParamSpec],
    pub sr: f32,
    pub max_frames: usize,
}

impl Args<'_> {
    /// A setting: follows its param (every setting has one, defaults come from the prelude).
    pub fn p(&self, r: &ParamRef) -> P {
        P::param(*r)
    }
    pub fn samples(&self, ms: f32) -> usize {
        (ms * 0.001 * self.sr).ceil() as usize
    }
}

pub(crate) fn next_meter(meter: &mut usize) -> usize {
    let m = *meter;
    *meter += 1;
    m
}

/// Builds the blocks of this module; `None` for the ones `lib.rs` builds itself.
pub(crate) fn build(kind: &BlockKind, a: &Args, meter: &mut usize) -> Option<Box<dyn Fx>> {
    use BlockKind as K;
    Some(match kind {
        K::Waveshaper { .. }
        | K::Wavefolder { .. }
        | K::Clipper { .. }
        | K::Rectifier { .. }
        | K::Bitcrusher { .. }
        | K::RingMod { .. }
        | K::Tape { .. } => distortion::build(kind, a),
        K::MorphFilter { .. }
        | K::LadderFilter { .. }
        | K::CombFilter { .. }
        | K::Formant { .. }
        | K::Resonator { .. }
        | K::DynamicEq { .. } => filter::build(kind, a, meter),
        K::Chorus { .. } | K::Flanger { .. } | K::Phaser { .. } | K::Tremolo { .. } | K::AutoPan { .. } => {
            modulation::build(kind, a)
        }
        K::Delay { .. } | K::Reverb(_) | K::Shimmer(_) => space::build(kind, a, meter),
        K::PitchShift { .. } | K::Granular { .. } => pitch::build(kind, a),
        K::Stutter { .. } => rhythm::build(kind, a),
        K::Limiter { .. }
        | K::Gate { .. }
        | K::DeEsser { .. }
        | K::Transient { .. }
        | K::Compressor(_)
        | K::FetCompressor(_)
        | K::OptoCompressor(_)
        | K::VariMuCompressor(_) => dynamics::build(kind, a, meter),
        _ => return None,
    })
}

// ---------------------------------------------------------------- helpers

/// `x` crossfaded towards `y` by `wet` (0..1).
#[inline]
pub(crate) fn mix(x: f32, y: f32, wet: f32) -> f32 {
    x + wet * (y - x)
}

/// Percent setting → 0..1.
#[inline]
pub(crate) fn pct(v: f32) -> f32 {
    (v / 100.0).clamp(0.0, 1.0)
}

/// An LFO rate in Hz: `sync` (beats) at the host tempo if given, else `rate`.
#[inline]
pub(crate) fn rate_hz(rate: f32, sync: Option<f32>, tempo: f32) -> f32 {
    match sync {
        Some(beats) => tempo / 60.0 / beats,
        None => rate.max(0.0),
    }
}

/// A time in ms: `sync` (beats) at the host tempo if given, else `ms`.
#[inline]
pub(crate) fn time_ms(ms: f32, sync: Option<f32>, tempo: f32) -> f32 {
    match sync {
        Some(beats) => beats * 60_000.0 / tempo,
        None => ms,
    }
}

/// Mono circular delay line with fractional (linear) taps.
pub(crate) struct Line {
    buf: Vec<f32>,
    pos: usize,
}

impl Line {
    /// Holds at least `max_delay` samples of history.
    pub fn new(max_delay: usize) -> Self {
        Line { buf: vec![0.0; max_delay + 2], pos: 0 }
    }

    /// The longest usable delay.
    pub fn max_delay(&self) -> f32 {
        (self.buf.len() - 2) as f32
    }

    #[inline]
    pub fn push(&mut self, x: f32) {
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        self.buf[self.pos] = x;
    }

    /// The sample `d` samples ago (0 = the newest), linearly interpolated.
    #[inline]
    pub fn tap(&self, d: f32) -> f32 {
        let d = d.clamp(0.0, self.max_delay());
        let i = d as usize;
        let frac = d - i as f32;
        let i0 = self.index(i);
        let i1 = if i0 == 0 { self.buf.len() - 1 } else { i0 - 1 };
        let a = self.buf[i0];
        a + frac * (self.buf[i1] - a)
    }

    /// Buffer index of the sample `ago` samples back (`ago` < buffer length);
    /// a branch instead of `%` (an integer division) on this hot path.
    #[inline]
    fn index(&self, ago: usize) -> usize {
        if ago <= self.pos { self.pos - ago } else { self.pos + self.buf.len() - ago }
    }

    /// Adds `x` to the sample `ago` samples before the newest.
    #[inline]
    pub fn add(&mut self, ago: usize, x: f32) {
        let i = self.index(ago.min(self.buf.len() - 1));
        self.buf[i] += x;
    }

    #[inline]
    pub fn at(&self, ago: usize) -> f32 {
        self.buf[self.index(ago.min(self.buf.len() - 1))]
    }
}

/// Phase accumulator 0..1 with the usual waveforms (-1..1).
#[derive(Clone, Copy, Default)]
pub(crate) struct Lfo {
    pub phase: f32,
}

impl Lfo {
    /// Advances by `hz` at `sr` and returns the phase before the step.
    #[inline]
    pub fn step(&mut self, hz: f32, sr: f32) -> f32 {
        let p = self.phase;
        self.phase += hz / sr;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }
        p
    }
}

#[inline]
pub(crate) fn wave(shape: patch::chain::LfoShape, phase: f32) -> f32 {
    use patch::chain::LfoShape;
    let p = phase - phase.floor();
    match shape {
        LfoShape::Sine => (std::f32::consts::TAU * p).sin(),
        // 0 at 0, +1 at 0.25, 0 at 0.5, -1 at 0.75 (in phase with the sine).
        LfoShape::Triangle => 1.0 - 4.0 * (p - 0.25 - (p - 0.25).round()).abs(),
        LfoShape::Square => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
    }
}

/// xorshift32: cheap, deterministic randomness for the audio thread.
pub(crate) struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(seed.max(1))
    }
    /// Uniform in 0..1.
    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
    /// Uniform in -1..1.
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        2.0 * self.next() - 1.0
    }
}

/// One-pole smoother (per sample).
#[derive(Clone, Copy)]
pub(crate) struct Smooth {
    pub value: f32,
    c: f32,
}

impl Smooth {
    pub fn new(value: f32, ms: f32, sr: f32) -> Self {
        Smooth { value, c: 1.0 - crate::coef(ms, sr) }
    }
    #[inline]
    pub fn next(&mut self, target: f32) -> f32 {
        self.value += self.c * (target - self.value);
        self.value
    }
}

/// One-pole DC blocker (~5 Hz).
#[derive(Clone, Copy, Default)]
pub(crate) struct DcBlocker {
    x1: f32,
    y1: f32,
}

impl DcBlocker {
    #[inline]
    pub fn tick(&mut self, x: f32, r: f32) -> f32 {
        self.y1 = x - self.x1 + r * self.y1;
        self.x1 = x;
        self.y1
    }
    pub fn coef(sr: f32) -> f32 {
        (-std::f32::consts::TAU * 5.0 / sr).exp()
    }
}

/// Schroeder allpass (diffusion).
pub(crate) struct Allpass {
    line: Line,
    delay: usize,
    g: f32,
}

impl Allpass {
    pub fn new(delay: usize, g: f32) -> Self {
        Allpass { line: Line::new(delay + 1), delay: delay.max(1), g }
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let d = self.line.at(self.delay - 1);
        let y = -self.g * x + d;
        self.line.push(x + self.g * y);
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_taps_the_past() {
        let mut l = Line::new(16);
        for i in 0..10 {
            l.push(i as f32);
        }
        assert_eq!(l.tap(0.0), 9.0);
        assert_eq!(l.tap(3.0), 6.0);
        assert_eq!(l.tap(2.5), 6.5);
    }

    #[test]
    fn waves_are_in_range() {
        use patch::chain::LfoShape;
        for shape in [LfoShape::Sine, LfoShape::Triangle, LfoShape::Square] {
            for i in 0..100 {
                let v = wave(shape, i as f32 / 100.0);
                assert!((-1.0..=1.0).contains(&v), "{v}");
            }
        }
        assert!((wave(LfoShape::Triangle, 0.25) - 1.0).abs() < 1e-6);
        assert!((wave(LfoShape::Triangle, 0.75) + 1.0).abs() < 1e-6);
        assert!(wave(LfoShape::Triangle, 0.0).abs() < 1e-6);
    }
}
