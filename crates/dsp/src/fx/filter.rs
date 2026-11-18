//! Filters: morphing state-variable filter, ladder, comb, formant (vowel),
//! resonator bank, dynamic EQ.

use std::sync::atomic::Ordering;

use patch::chain::{BlockKind, DynamicMode};

use super::{Args, Fx, Line, mix, next_meter, pct};
use crate::{Biquad, Ctx, EqShape, Frame, P, coef, db_to_gain, gain_to_db};

pub(super) fn build(kind: &BlockKind, a: &Args, meter: &mut usize) -> Box<dyn Fx> {
    match kind {
        BlockKind::MorphFilter { freq, morph, resonance } => Box::new(MorphFilter {
            freq: a.req(freq),
            morph: a.p(morph, 0.0),
            resonance: a.p(resonance, 0.0),
            ic: [[0.0; 2]; 2],
        }),
        BlockKind::LadderFilter { freq, resonance, drive } => Box::new(Ladder {
            freq: a.req(freq),
            resonance: a.p(resonance, 0.0),
            drive: a.p(drive, 0.0),
            z: [[0.0; 4]; 2],
            y4: [0.0; 2],
        }),
        BlockKind::CombFilter { freq, feedback, mix } => Box::new(Comb {
            freq: a.req(freq),
            feedback: a.p(feedback, 70.0),
            mix: a.p(mix, 50.0),
            // Down to 20 Hz.
            lines: [Line::new(a.samples(50.0)), Line::new(a.samples(50.0))],
        }),
        BlockKind::Formant { vowel, shift, resonance, mix } => Box::new(Formant {
            vowel: a.req(vowel),
            shift: a.p(shift, 0.0),
            resonance: a.p(resonance, 50.0),
            mix: a.p(mix, 100.0),
            bands: [Biquad::default(); 3],
            amps: [0.0; 3],
            last: [f32::NAN; 3],
        }),
        BlockKind::Resonator { notes, decay, mix } => Box::new(Resonator {
            notes: notes.iter().map(|&hz| Reson { hz, ..Reson::default() }).collect(),
            decay: a.p(decay, 500.0),
            mix: a.p(mix, 50.0),
            last: f32::NAN,
        }),
        BlockKind::DynamicEq { freq, q, threshold, ratio, range, attack, release, mode } => Box::new(DynamicEq {
            freq: a.req(freq),
            q: a.p(q, 1.0),
            threshold: a.req(threshold),
            ratio: a.p(ratio, 3.0),
            range: a.p(range, 12.0),
            attack: a.p(attack, 5.0),
            release: a.p(release, 100.0),
            mode: *mode,
            detector: Biquad::default(),
            eq: Biquad::default(),
            env: 0.0,
            gain_db: 0.0,
            meter: next_meter(meter),
        }),
        _ => unreachable!(),
    }
}

/// Topology-preserving SVF (Zavalishin): low/band/high from one structure.
struct MorphFilter {
    freq: P,
    morph: P,
    resonance: P,
    /// Integrator states per channel.
    ic: [[f32; 2]; 2],
}

impl Fx for MorphFilter {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let fc = self.freq.update(ctx).clamp(10.0, ctx.sr * 0.45);
        let m = pct(self.morph.update(ctx));
        let r = pct(self.resonance.update(ctx));
        let q = 0.5 + r * r * 24.5;
        let k = 1.0 / q;
        let g = (std::f32::consts::PI * fc / ctx.sr).tan();
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        // 0 → LP, 0.5 → BP (normalized to 0 dB peak), 1 → HP.
        let (wl, wb, wh) = if m < 0.5 { (1.0 - 2.0 * m, 2.0 * m, 0.0) } else { (0.0, 2.0 - 2.0 * m, 2.0 * m - 1.0) };
        for f in frames {
            for (ch, x) in f.iter_mut().enumerate() {
                let [ic1, ic2] = &mut self.ic[ch];
                let v3 = *x - *ic2;
                let v1 = a1 * *ic1 + a2 * v3;
                let v2 = *ic2 + a2 * *ic1 + a3 * v3;
                *ic1 = 2.0 * v1 - *ic1;
                *ic2 = 2.0 * v2 - *ic2;
                let (lp, bp, hp) = (v2, v1, *x - k * v1 - v2);
                *x = wl * lp + wb * k * bp + wh * hp;
            }
        }
    }
}

/// Four TPT one-poles with a saturating input stage and feedback (Moog-style).
struct Ladder {
    freq: P,
    resonance: P,
    drive: P,
    z: [[f32; 4]; 2],
    y4: [f32; 2],
}

impl Fx for Ladder {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let fc = self.freq.update(ctx).clamp(10.0, ctx.sr * 0.45);
        let k = 4.0 * pct(self.resonance.update(ctx));
        let gd = db_to_gain(self.drive.update(ctx));
        let g = (std::f32::consts::PI * fc / ctx.sr).tan();
        let gg = g / (1.0 + g);
        // Resonance thins the passband (DC gain 1/(1+k)); give some of it back.
        let makeup = (1.0 + 0.5 * k) / gd.sqrt();
        for f in frames {
            for (ch, x) in f.iter_mut().enumerate() {
                let mut s = (gd * *x - k * self.y4[ch]).tanh();
                for z in self.z[ch].iter_mut() {
                    let v = (s - *z) * gg;
                    s = v + *z;
                    *z = s + v;
                }
                self.y4[ch] = s;
                *x = s * makeup;
            }
        }
    }
}

struct Comb {
    freq: P,
    feedback: P,
    mix: P,
    lines: [Line; 2],
}

impl Fx for Comb {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let max = self.lines[0].max_delay();
        let d = (ctx.sr / self.freq.update(ctx).max(1.0)).clamp(1.0, max);
        let fb = (self.feedback.update(ctx) / 100.0).clamp(-0.99, 0.99);
        let wet = pct(self.mix.update(ctx));
        // Peak gain of a feedback comb is 1 / (1 - |fb|).
        let norm = 1.0 - fb.abs();
        for f in frames {
            for (ch, x) in f.iter_mut().enumerate() {
                // tap(d - 1) before the push = d samples before the new one.
                let y = *x + fb * self.lines[ch].tap(d - 1.0);
                self.lines[ch].push(y);
                *x = mix(*x, y * norm, wet);
            }
        }
    }
}

/// F1..F3 (Hz) and relative amplitudes of a, e, i, o, u.
const VOWELS: [([f32; 3], [f32; 3]); 5] = [
    ([730.0, 1090.0, 2440.0], [1.0, 0.5, 0.25]),
    ([530.0, 1840.0, 2480.0], [1.0, 0.45, 0.3]),
    ([270.0, 2290.0, 3010.0], [1.0, 0.3, 0.25]),
    ([570.0, 840.0, 2410.0], [1.0, 0.6, 0.2]),
    ([300.0, 870.0, 2240.0], [1.0, 0.35, 0.15]),
];

struct Formant {
    vowel: P,
    shift: P,
    resonance: P,
    mix: P,
    bands: [Biquad; 3],
    amps: [f32; 3],
    last: [f32; 3],
}

impl Fx for Formant {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let v = self.vowel.update(ctx).clamp(0.0, 4.0);
        let shift = 2f32.powf(self.shift.update(ctx).clamp(-24.0, 24.0) / 12.0);
        let q = 2.0 + pct(self.resonance.update(ctx)) * 18.0;
        let wet = pct(self.mix.update(ctx));
        let cur = [v, shift, q];
        if cur != self.last {
            self.last = cur;
            let i = (v.floor() as usize).min(3);
            let t = v - i as f32;
            let (fa, aa) = VOWELS[i];
            let (fb, ab) = VOWELS[i + 1];
            for b in 0..3 {
                let hz = (fa[b] + t * (fb[b] - fa[b])) * shift;
                self.amps[b] = aa[b] + t * (ab[b] - aa[b]);
                self.bands[b].set(EqShape::BandPass, hz, q, 0.0, ctx.sr);
            }
        }
        // Narrow band-passes lose level; scale back up roughly with Q.
        let gain = (q / 4.0).sqrt();
        for f in frames {
            for (ch, x) in f.iter_mut().enumerate() {
                let mut y = 0.0;
                for (band, amp) in self.bands.iter_mut().zip(self.amps) {
                    y += amp * band.tick(*x, ch);
                }
                *x = mix(*x, y * gain, wet);
            }
        }
    }
}

/// Two-pole resonator with zeros at DC and Nyquist (≈ 0 dB peak), in f64:
/// long decays put the poles very close to the unit circle.
#[derive(Default)]
struct Reson {
    hz: f32,
    b0: f64,
    a1: f64,
    a2: f64,
    x: [[f64; 2]; 2],
    y: [[f64; 2]; 2],
}

struct Resonator {
    notes: Vec<Reson>,
    decay: P,
    mix: P,
    last: f32,
}

impl Fx for Resonator {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let decay = self.decay.update(ctx).clamp(10.0, 30_000.0);
        let wet = pct(self.mix.update(ctx));
        if decay != self.last {
            self.last = decay;
            // Pole radius for a -60 dB decay in `decay` ms.
            let r = (-6.907_755 / (decay as f64 * 0.001 * ctx.sr as f64)).exp();
            for n in &mut self.notes {
                let w = std::f64::consts::TAU * (n.hz as f64).clamp(10.0, ctx.sr as f64 * 0.45) / ctx.sr as f64;
                n.a1 = -2.0 * r * w.cos();
                n.a2 = r * r;
                n.b0 = (1.0 - r * r) / 2.0;
            }
        }
        let norm = 1.0 / (self.notes.len() as f32).sqrt();
        for f in frames {
            for (ch, x) in f.iter_mut().enumerate() {
                let input = *x as f64;
                let mut sum = 0.0f64;
                for n in &mut self.notes {
                    let y = n.b0 * (input - n.x[ch][1]) - n.a1 * n.y[ch][0] - n.a2 * n.y[ch][1];
                    n.x[ch] = [input, n.x[ch][0]];
                    n.y[ch] = [y, n.y[ch][0]];
                    sum += y;
                }
                *x = mix(*x, sum as f32 * norm, wet);
            }
        }
    }
}

struct DynamicEq {
    freq: P,
    q: P,
    threshold: P,
    ratio: P,
    range: P,
    attack: P,
    release: P,
    mode: DynamicMode,
    detector: Biquad,
    eq: Biquad,
    /// Band level envelope (linear).
    env: f32,
    /// Current EQ gain, dB.
    gain_db: f32,
    meter: usize,
}

impl Fx for DynamicEq {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let freq = self.freq.update(ctx);
        let q = self.q.update(ctx).max(0.1);
        let thr = self.threshold.update(ctx);
        let slope = 1.0 - 1.0 / self.ratio.update(ctx).max(1.0);
        let range = self.range.update(ctx).abs();
        let att = coef(self.attack.update(ctx), ctx.sr);
        let rel = coef(self.release.update(ctx), ctx.sr);
        self.detector.set(EqShape::BandPass, freq, q, 0.0, ctx.sr);
        for f in frames.iter() {
            let b = self.detector.tick(0.5 * (f[0] + f[1]), 0).abs();
            let c = if b > self.env { att } else { rel };
            self.env = c * self.env + (1.0 - c) * b;
        }
        let over = (gain_to_db(self.env) - thr).max(0.0);
        let change = (over * slope).min(range);
        self.gain_db = match self.mode {
            DynamicMode::Cut => -change,
            DynamicMode::Boost => change,
        };
        // Coefficients once per sub-block (≤ 32 samples at the base rate).
        self.eq.set(EqShape::Peak, freq, q, self.gain_db, ctx.sr);
        for f in frames {
            f[0] = self.eq.tick(f[0], 0);
            f[1] = self.eq.tick(f[1], 1);
        }
        ctx.meters[self.meter].store((change / 24.0).clamp(0.0, 1.0), Ordering::Relaxed);
    }
}
