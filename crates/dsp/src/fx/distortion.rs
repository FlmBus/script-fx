//! Distortion: waveshaper, wavefolder, clipper, rectifier, bitcrusher, ring
//! modulator, tape. All except the bitcrusher are flagged nonlinear in the patch
//! model, so they're oversampled automatically (see `lib.rs`).

use patch::chain::{BlockKind, ParamRef, RectifyMode};

use super::{Args, DcBlocker, Fx, Line, Lfo, Smooth, mix, pct};
use crate::{Biquad, Ctx, EqShape, Frame, P, db_to_gain};

pub(super) fn build(kind: &BlockKind, a: &Args) -> Box<dyn Fx> {
    match kind {
        BlockKind::Waveshaper { curve, drive, output, mix } => Box::new(Waveshaper {
            lut: curve.clone(),
            drive: a.p(drive),
            output: a.p(output),
            mix: a.p(mix),
        }),
        BlockKind::Wavefolder { amount, bias, mix } => Box::new(Wavefolder {
            amount: a.p(amount),
            bias: a.p(bias),
            mix: a.p(mix),
            dc: [DcBlocker::default(); 2],
        }),
        BlockKind::Clipper { ceiling, softness, drive } => {
            Box::new(Clipper { ceiling: a.p(ceiling), softness: a.p(softness), drive: a.p(drive) })
        }
        BlockKind::Rectifier { mode, mix } => {
            Box::new(Rectifier { mode: *mode, mix: a.p(mix), dc: [DcBlocker::default(); 2] })
        }
        BlockKind::Bitcrusher { bits, rate, mix } => Box::new(Bitcrusher {
            bits: a.p(bits),
            rate: rate.as_ref().map(|r| a.p(r)),
            mix: a.p(mix),
            phase: 1.0,
            held: [0.0; 2],
        }),
        BlockKind::RingMod { freq, mix } => Box::new(RingMod { freq: a.p(freq), mix: a.p(mix), lfo: Lfo::default() }),
        BlockKind::Tape { drive, wow, flutter, mix } => Box::new(Tape::new(a, drive, wow, flutter, mix)),
        _ => unreachable!(),
    }
}

struct Waveshaper {
    lut: Vec<f32>,
    drive: P,
    output: P,
    mix: P,
}

impl Fx for Waveshaper {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let g = db_to_gain(self.drive.update(ctx));
        let out = db_to_gain(self.output.update(ctx));
        let wet = pct(self.mix.update(ctx));
        for f in frames {
            for x in f.iter_mut() {
                *x = mix(*x, lut_at(&self.lut, *x * g) * out, wet);
            }
        }
    }
}

/// The waveshaper's curve at `v` (clamped to ±1), linearly interpolated.
#[inline]
pub(crate) fn lut_at(lut: &[f32], v: f32) -> f32 {
    let pos = (v.clamp(-1.0, 1.0) + 1.0) * 0.5 * (lut.len() - 1) as f32;
    let i = (pos as usize).min(lut.len() - 2);
    lut[i] + (pos - i as f32) * (lut[i + 1] - lut[i])
}

/// The wavefolder's curve (before its DC blocker): sin() folds the signal back each time it
/// passes ±1; `bias` -1..1.
#[inline]
pub(crate) fn fold(x: f32, g: f32, bias: f32) -> f32 {
    let half_pi = std::f32::consts::FRAC_PI_2;
    (half_pi * (x * g + bias)).sin() - (half_pi * bias).sin()
}

/// The clipper's curve: `c` ceiling (gain), `g` drive (gain), `soft` 0..1.
#[inline]
pub(crate) fn clip(x: f32, g: f32, c: f32, soft: f32) -> f32 {
    let v = x * g / c;
    let hard = v.clamp(-1.0, 1.0);
    c * (hard + soft * (v.tanh() - hard))
}

struct Wavefolder {
    amount: P,
    bias: P,
    mix: P,
    dc: [DcBlocker; 2],
}

impl Fx for Wavefolder {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let g = db_to_gain(self.amount.update(ctx));
        let bias = (self.bias.update(ctx) / 100.0).clamp(-1.0, 1.0);
        let wet = pct(self.mix.update(ctx));
        let r = DcBlocker::coef(ctx.sr);
        for f in frames {
            for (ch, x) in f.iter_mut().enumerate() {
                let y = self.dc[ch].tick(fold(*x, g, bias), r);
                *x = mix(*x, y, wet);
            }
        }
    }
}

struct Clipper {
    ceiling: P,
    softness: P,
    drive: P,
}

impl Fx for Clipper {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let c = db_to_gain(self.ceiling.update(ctx)).max(1e-6);
        let soft = pct(self.softness.update(ctx));
        let g = db_to_gain(self.drive.update(ctx));
        for f in frames {
            for x in f.iter_mut() {
                *x = clip(*x, g, c, soft);
            }
        }
    }
}

struct Rectifier {
    mode: RectifyMode,
    mix: P,
    dc: [DcBlocker; 2],
}

impl Fx for Rectifier {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let wet = pct(self.mix.update(ctx));
        let r = DcBlocker::coef(ctx.sr);
        for f in frames {
            for (ch, x) in f.iter_mut().enumerate() {
                let y = match self.mode {
                    RectifyMode::Full => x.abs(),
                    RectifyMode::Half => x.max(0.0) * 2.0,
                };
                let y = self.dc[ch].tick(y, r);
                *x = mix(*x, y, wet);
            }
        }
    }
}

struct Bitcrusher {
    bits: P,
    rate: Option<P>,
    mix: P,
    /// Sample-and-hold phase (≥ 1 = take a new sample).
    phase: f32,
    held: Frame,
}

impl Fx for Bitcrusher {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let levels = 2f32.powf(self.bits.update(ctx).clamp(1.0, 24.0) - 1.0);
        let step = match &mut self.rate {
            Some(r) => (r.update(ctx) / ctx.sr).clamp(1e-4, 1.0),
            None => 1.0,
        };
        let wet = pct(self.mix.update(ctx));
        for f in frames {
            self.phase += step;
            if self.phase >= 1.0 {
                self.phase -= self.phase.floor();
                self.held = *f;
            }
            for (x, h) in f.iter_mut().zip(self.held) {
                let q = (h * levels).round() / levels;
                *x = mix(*x, q, wet);
            }
        }
    }
}

struct RingMod {
    freq: P,
    mix: P,
    lfo: Lfo,
}

impl Fx for RingMod {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let hz = self.freq.update(ctx).clamp(0.0, ctx.sr * 0.45);
        let wet = pct(self.mix.update(ctx));
        for f in frames {
            let c = (std::f32::consts::TAU * self.lfo.step(hz, ctx.sr)).sin();
            for x in f.iter_mut() {
                *x = mix(*x, *x * c, wet);
            }
        }
    }
}

/// Fixed delay around which wow and flutter swing, ms. Dry and wet both pass it,
/// so it's reported as latency.
const TAPE_BASE_MS: f32 = 4.0;

struct Tape {
    drive: P,
    wow: P,
    flutter: P,
    mix: P,
    lines: [Line; 2],
    base: usize,
    wow_lfo: Lfo,
    flutter_lfo: Lfo,
    bump: Biquad,
    /// One-pole HF loss state per channel.
    lp: [f32; 2],
    depth: [Smooth; 2],
    sr: f32,
}

impl Tape {
    fn new(a: &Args, drive: &ParamRef, wow: &ParamRef, flutter: &ParamRef, mix: &ParamRef) -> Self {
        let base = a.samples(TAPE_BASE_MS);
        let mut bump = Biquad::default();
        bump.set(EqShape::Peak, 90.0, 1.0, 2.0, a.sr);
        Tape {
            drive: a.p(drive),
            wow: a.p(wow),
            flutter: a.p(flutter),
            mix: a.p(mix),
            lines: [Line::new(2 * base + 2), Line::new(2 * base + 2)],
            base,
            wow_lfo: Lfo::default(),
            flutter_lfo: Lfo { phase: 0.3 },
            bump,
            lp: [0.0; 2],
            depth: [Smooth::new(0.0, 50.0, a.sr), Smooth::new(0.0, 50.0, a.sr)],
            sr: a.sr,
        }
    }
}

impl Fx for Tape {
    fn latency(&self) -> usize {
        self.base
    }

    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let g = db_to_gain(self.drive.update(ctx));
        let comp = 1.0 / g.sqrt();
        let base = self.base as f32;
        // Swing (samples) of the two LFOs; wow ±3 ms at 0.6 Hz, flutter ±0.15 ms at 7 Hz.
        let wow = pct(self.wow.update(ctx)) * 0.003 * ctx.sr;
        let flutter = pct(self.flutter.update(ctx)) * 0.000_15 * ctx.sr;
        let wet = pct(self.mix.update(ctx));
        let hf = 1.0 - (-std::f32::consts::TAU * 14_000.0f32.min(0.45 * self.sr) / self.sr).exp();
        // Asymmetric curve (even harmonics): tanh(x + a) - tanh(a).
        const ASYM: f32 = 0.15;
        let offset = ASYM.tanh();
        for f in frames {
            let w = (std::f32::consts::TAU * self.wow_lfo.step(0.6, ctx.sr)).sin();
            let fl = (std::f32::consts::TAU * self.flutter_lfo.step(7.0, ctx.sr)).sin();
            let wow_d = self.depth[0].next(wow);
            let fl_d = self.depth[1].next(flutter);
            let d = (base + w * wow_d.min(base) + fl * fl_d).clamp(0.0, 2.0 * base);
            for (ch, ((line, lp), out)) in self.lines.iter_mut().zip(&mut self.lp).zip(f.iter_mut()).enumerate() {
                line.push(*out);
                let dry = line.at(self.base);
                let x = line.tap(d);
                let y = ((x * g + ASYM).tanh() - offset) * comp;
                let y = self.bump.tick(y, ch);
                *lp += hf * (y - *lp);
                *out = mix(dry, *lp, wet);
            }
        }
    }
}
