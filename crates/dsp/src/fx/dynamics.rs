//! Dynamics: lookahead limiter, gate / expander, de-esser.

use std::sync::atomic::Ordering;

use patch::chain::BlockKind;

use super::{Args, Fx, Line, next_meter};
use crate::{Biquad, Ctx, EqShape, Frame, P, coef, db_to_gain, gain_to_db};

pub(super) fn build(kind: &BlockKind, a: &Args, meter: &mut usize) -> Box<dyn Fx> {
    match kind {
        BlockKind::Limiter { ceiling, release, lookahead } => {
            let la = (lookahead.unwrap_or(1.5).clamp(0.0, 20.0) * 0.001 * a.sr).round() as usize;
            Box::new(Limiter {
                ceiling: a.p(ceiling, -0.3),
                release: a.p(release, 80.0),
                la,
                lines: [Line::new(la + 1), Line::new(la + 1)],
                min: MinWindow::new(la + 1),
                avg: vec![1.0; la.max(1)],
                avg_pos: 0,
                avg_sum: la.max(1) as f64,
                g2: 1.0,
                meter: next_meter(meter),
            })
        }
        BlockKind::Gate { threshold, ratio, range, attack, hold, release } => Box::new(Gate {
            threshold: a.req(threshold),
            ratio: a.p(ratio, 20.0),
            range: a.p(range, 60.0),
            attack: a.p(attack, 1.0),
            hold: a.p(hold, 20.0),
            release: a.p(release, 150.0),
            env: 0.0,
            att: 0.0,
            held: 0,
            meter: next_meter(meter),
        }),
        BlockKind::DeEsser { freq, threshold, range } => Box::new(DeEsser {
            freq: a.p(freq, 6000.0),
            threshold: a.req(threshold),
            range: a.p(range, 12.0),
            detector: Biquad::default(),
            shelf: Biquad::default(),
            env: 0.0,
            meter: next_meter(meter),
        }),
        _ => unreachable!(),
    }
}

/// Sliding minimum over the last `len` values (monotonic deque in a ring).
struct MinWindow {
    vals: Vec<f32>,
    idx: Vec<u64>,
    head: usize,
    count: usize,
    len: usize,
    t: u64,
}

impl MinWindow {
    fn new(len: usize) -> Self {
        let cap = len + 1;
        MinWindow { vals: vec![0.0; cap], idx: vec![0; cap], head: 0, count: 0, len, t: 0 }
    }

    #[inline]
    fn push(&mut self, v: f32) -> f32 {
        let cap = self.vals.len();
        // Drop larger values from the back, expired ones from the front.
        while self.count > 0 && self.vals[(self.head + self.count - 1) % cap] >= v {
            self.count -= 1;
        }
        let back = (self.head + self.count) % cap;
        self.vals[back] = v;
        self.idx[back] = self.t;
        self.count += 1;
        while self.idx[self.head] + self.len as u64 <= self.t {
            self.head = (self.head + 1) % cap;
            self.count -= 1;
        }
        self.t += 1;
        self.vals[self.head]
    }
}

/// Gain = the sliding minimum of the needed gain over the lookahead, released
/// slowly, then averaged over the lookahead: it reaches every peak's gain exactly
/// when that (delayed) peak arrives, so nothing overshoots the ceiling.
struct Limiter {
    ceiling: P,
    release: P,
    la: usize,
    lines: [Line; 2],
    min: MinWindow,
    avg: Vec<f32>,
    avg_pos: usize,
    avg_sum: f64,
    g2: f32,
    meter: usize,
}

impl Fx for Limiter {
    fn latency(&self) -> usize {
        self.la
    }

    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let ceil = db_to_gain(self.ceiling.update(ctx).min(0.0));
        let rel = coef(self.release.update(ctx).max(1.0), ctx.sr);
        let mut g3 = 1.0;
        for f in frames {
            let peak = f[0].abs().max(f[1].abs());
            let need = if peak > ceil { ceil / peak } else { 1.0 };
            let wmin = self.min.push(need);
            self.g2 = if wmin < self.g2 { wmin } else { wmin + (self.g2 - wmin) * rel };
            if self.la == 0 {
                g3 = self.g2;
            } else {
                self.avg_sum += (self.g2 - self.avg[self.avg_pos]) as f64;
                self.avg[self.avg_pos] = self.g2;
                self.avg_pos = (self.avg_pos + 1) % self.avg.len();
                g3 = (self.avg_sum / self.avg.len() as f64) as f32;
            }
            for (x, line) in f.iter_mut().zip(&mut self.lines) {
                line.push(*x);
                // Float rounding in the average: never let a sample through above the ceiling.
                *x = (line.at(self.la) * g3).clamp(-ceil, ceil);
            }
        }
        // Re-sum once per call so the running sum can't drift (cheap: ≤ 20 ms of values).
        if self.la > 0 {
            self.avg_sum = self.avg.iter().map(|&v| v as f64).sum();
        }
        ctx.meters[self.meter].store((-gain_to_db(g3) / 24.0).clamp(0.0, 1.0), Ordering::Relaxed);
    }
}

struct Gate {
    threshold: P,
    ratio: P,
    range: P,
    attack: P,
    hold: P,
    release: P,
    env: f32,
    /// Current attenuation, dB (≥ 0).
    att: f32,
    held: usize,
    meter: usize,
}

impl Fx for Gate {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let thr = self.threshold.update(ctx);
        let ratio = self.ratio.update(ctx).max(1.0);
        let range = self.range.update(ctx).abs();
        let att_c = coef(self.attack.update(ctx), ctx.sr);
        let hold = (self.hold.update(ctx).max(0.0) * 0.001 * ctx.sr) as usize;
        let rel_c = coef(self.release.update(ctx), ctx.sr);
        let det = coef(10.0, ctx.sr);
        for f in frames {
            self.env = (f[0].abs().max(f[1].abs())).max(self.env * det);
            let below = thr - gain_to_db(self.env);
            let target = if below > 0.0 { (below * (ratio - 1.0)).min(range) } else { 0.0 };
            if below <= 0.0 {
                self.held = hold;
            }
            if target < self.att {
                self.att = target + (self.att - target) * att_c;
            } else if self.held > 0 {
                self.held -= 1;
            } else {
                self.att = target + (self.att - target) * rel_c;
            }
            let g = db_to_gain(-self.att);
            f[0] *= g;
            f[1] *= g;
        }
        ctx.meters[self.meter].store((self.att / 24.0).clamp(0.0, 1.0), Ordering::Relaxed);
    }
}

/// Detects the band above `freq` (high-pass on the mid signal) and turns it
/// down with a high shelf at `freq` whose gain follows the detected level
/// (coefficients once per sub-block, like `DynamicEq`).
struct DeEsser {
    freq: P,
    threshold: P,
    range: P,
    detector: Biquad,
    shelf: Biquad,
    env: f32,
    meter: usize,
}

impl Fx for DeEsser {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let freq = self.freq.update(ctx).clamp(1000.0, ctx.sr * 0.45);
        let thr = self.threshold.update(ctx);
        let range = self.range.update(ctx).abs();
        self.detector.set(EqShape::HighPass, freq, std::f32::consts::FRAC_1_SQRT_2, 0.0, ctx.sr);
        let (att, rel) = (coef(1.0, ctx.sr), coef(60.0, ctx.sr));
        for f in frames.iter() {
            let d = self.detector.tick(0.5 * (f[0] + f[1]), 0).abs();
            let c = if d > self.env { att } else { rel };
            self.env = c * self.env + (1.0 - c) * d;
        }
        let red = ((gain_to_db(self.env) - thr).max(0.0) * 0.75).min(range);
        self.shelf.set(EqShape::HighShelf, freq, std::f32::consts::FRAC_1_SQRT_2, -red, ctx.sr);
        for f in frames {
            f[0] = self.shelf.tick(f[0], 0);
            f[1] = self.shelf.tick(f[1], 1);
        }
        ctx.meters[self.meter].store((red / 24.0).clamp(0.0, 1.0), Ordering::Relaxed);
    }
}
