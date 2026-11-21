//! Dynamics: the versatile compressor (all compressor blocks are presets of it), transient
//! shaper, lookahead limiter, gate / expander, de-esser.
//!
//! Lookahead (`lookahead` ms, fixed at load: it is latency): the detector sees the signal now,
//! the gain applies to the signal delayed by the lookahead, so it is already down when a peak
//! arrives.

use std::sync::atomic::Ordering;

use patch::chain::BlockKind;

use super::{Args, Fx, Line, next_meter};
use crate::{Biquad, Ctx, EqShape, Frame, P, coef, db_to_gain, gain_to_db};

pub(super) fn build(kind: &BlockKind, a: &Args, meter: &mut usize) -> Box<dyn Fx> {
    match kind {
        BlockKind::Limiter { ceiling, release, lookahead } => {
            let la = (lookahead.unwrap_or(1.5).clamp(0.0, 20.0) * 0.001 * a.sr).round() as usize;
            Box::new(Limiter {
                ceiling: a.p(ceiling),
                release: a.p(release),
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
        BlockKind::Gate { threshold, ratio, range, attack, hold, release, lookahead } => Box::new(Gate {
            threshold: a.p(threshold),
            ratio: a.p(ratio),
            range: a.p(range),
            attack: a.p(attack),
            hold: a.p(hold),
            release: a.p(release),
            ahead: Lookahead::new(*lookahead, a),
            env: 0.0,
            att: 0.0,
            held: 0,
            meter: next_meter(meter),
        }),
        BlockKind::DeEsser { freq, threshold, range, lookahead } => Box::new(DeEsser {
            freq: a.p(freq),
            threshold: a.p(threshold),
            range: a.p(range),
            ahead: Lookahead::new(*lookahead, a),
            detector: Biquad::default(),
            shelf: Biquad::default(),
            env: 0.0,
            meter: next_meter(meter),
        }),
        BlockKind::Compressor(c) | BlockKind::FetCompressor(c) | BlockKind::OptoCompressor(c) | BlockKind::VariMuCompressor(c) => {
            Box::new(Compressor {
                threshold: a.p(&c.threshold),
                ratio: a.p(&c.ratio),
                attack: a.p(&c.attack),
                release: a.p(&c.release),
                knee: a.p(&c.knee),
                makeup: a.p(&c.makeup),
                mix: a.p(&c.mix),
                detector: a.p(&c.detector),
                window: a.p(&c.window),
                feedback: a.p(&c.feedback),
                ratio_curve: a.p(&c.ratio_curve),
                program: a.p(&c.program),
                link: a.p(&c.link),
                saturation: a.p(&c.saturation),
                auto_makeup: c.auto_makeup,
                ahead: Lookahead::new(c.lookahead, a),
                env: [0.0; 2],
                rms: [0.0; 2],
                last_out: [0.0; 2],
                sustain: 0.0,
                avg_gr: 0.0,
                meter: next_meter(meter),
            })
        }
        BlockKind::Transient { attack, sustain, lookahead } => Box::new(Transient {
            attack: a.p(attack),
            sustain: a.p(sustain),
            ahead: Lookahead::new(*lookahead, a),
            fast: 0.0,
            slow: 0.0,
            gain_db: 0.0,
            meter: next_meter(meter),
        }),
        _ => unreachable!(),
    }
}

/// The longest lookahead, ms.
pub(crate) const MAX_LOOKAHEAD_MS: f32 = 20.0;

/// Delays the audio by the lookahead; detection runs on the undelayed signal.
struct Lookahead {
    lines: [Line; 2],
    n: usize,
}

impl Lookahead {
    fn new(ms: Option<f32>, a: &Args) -> Self {
        let n = (ms.unwrap_or(0.0).clamp(0.0, MAX_LOOKAHEAD_MS) * 0.001 * a.sr).round() as usize;
        Lookahead { lines: [Line::new(n + 1), Line::new(n + 1)], n }
    }

    /// The frame from `n` samples ago (the input when there's no lookahead).
    #[inline]
    fn delay(&mut self, f: Frame) -> Frame {
        if self.n == 0 {
            return f;
        }
        self.lines[0].push(f[0]);
        self.lines[1].push(f[1]);
        [self.lines[0].at(self.n), self.lines[1].at(self.n)]
    }
}

/// How far above the threshold a rising ratio (`ratio_curve`) reaches the dialled ratio, dB.
const RATIO_RISE_DB: f32 = 30.0;

/// The compressor's static curve: gain reduction (dB, ≥ 0) at `over` dB above the threshold.
/// `curve` (0..1) blends towards a ratio that rises with the overshoot, as in a vari-mu: the
/// local ratio goes from 1:1 at the threshold to `ratio` 30 dB above it and stays there (the
/// reduction is its integral, so the output never falls as the input rises). Shared with the
/// display.
pub fn comp_static(over: f32, ratio: f32, knee: f32, curve: f32) -> f32 {
    let slope = 1.0 - 1.0 / ratio.max(1.0);
    let fixed = crate::comp_gr(over, slope, knee);
    if curve <= 0.0 {
        return fixed;
    }
    let o = over.max(0.0);
    let rising = if o < RATIO_RISE_DB { slope * o * o / (2.0 * RATIO_RISE_DB) } else { slope * (o - RATIO_RISE_DB / 2.0) };
    fixed + curve * (rising - fixed)
}

/// One compressor, every behaviour a parameter: detector (peak ↔ RMS), topology (feed-forward
/// ↔ feedback), ratio curve (vari-mu), program-dependent release, stereo link, saturation,
/// lookahead. The compressor blocks (clean, FET, opto, vari-mu) are presets of it.
struct Compressor {
    threshold: P,
    ratio: P,
    attack: P,
    release: P,
    knee: P,
    makeup: P,
    mix: P,
    /// 0 = peak, 100 = RMS over `window`.
    detector: P,
    window: P,
    /// 0 = feed-forward (detect the input), 100 = feedback (detect the output).
    feedback: P,
    ratio_curve: P,
    /// How much a long, heavy compression slows the release (opto, vari-mu), %.
    program: P,
    /// 0 = each channel on its own, 100 = both by the louder one.
    link: P,
    saturation: P,
    auto_makeup: bool,
    ahead: Lookahead,
    /// Gain reduction per channel, dB (≥ 0).
    env: [f32; 2],
    rms: [f32; 2],
    /// The last output per channel (the feedback detector's input).
    last_out: [f32; 2],
    /// How long/hard it has been compressing (0..1), for the program-dependent release.
    sustain: f32,
    /// Slow average of the gain reduction (dB), for adaptive auto makeup.
    avg_gr: f32,
    meter: usize,
}

impl Fx for Compressor {
    fn latency(&self) -> usize {
        self.ahead.n
    }

    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let sr = ctx.sr;
        let thr = self.threshold.update(ctx);
        let ratio = self.ratio.update(ctx).max(1.0);
        let knee = self.knee.update(ctx).max(0.0);
        let makeup_db = self.makeup.update(ctx);
        let wet = (self.mix.update(ctx) / 100.0).clamp(0.0, 1.0);
        let rms_amt = (self.detector.update(ctx) / 100.0).clamp(0.0, 1.0);
        let rms_c = coef(self.window.update(ctx).max(0.1), sr);
        let fb = (self.feedback.update(ctx) / 100.0).clamp(0.0, 1.0);
        let curve = (self.ratio_curve.update(ctx) / 100.0).clamp(0.0, 1.0);
        let program = (self.program.update(ctx) / 100.0).clamp(0.0, 1.0);
        let link = (self.link.update(ctx) / 100.0).clamp(0.0, 1.0);
        let sat = (self.saturation.update(ctx) / 100.0).clamp(0.0, 1.0);
        let att = coef(self.attack.update(ctx), sr);
        // Program-dependent: the longer/harder it compressed, the slower it lets go.
        let rel = coef(self.release.update(ctx) * (1.0 + 4.0 * program * self.sustain), sr);
        let sus_c = coef(1500.0, sr);
        // Adaptive auto makeup: give back most of the *average* gain reduction.
        let avg_c = coef(1500.0, sr);
        let auto = if self.auto_makeup { 0.8 } else { 0.0 };
        // Feedback detects the already reduced output: in steady state that gives
        // gr = s/(1 + fb·s) · over, so scale the slope to keep the dialled ratio. The loop
        // through the attack smoothing stays stable while (1 − att)·s·fb < 1 + att.
        let loop_max = (1.0 + att) / ((1.0 - att) * fb).max(1e-6);
        let sat_drive = 1.0 + 3.0 * sat;
        let sat_bias = 0.15 * sat;

        for f in frames {
            let input = *f;
            // Detector input per channel: the input or the output (topology), linked or not.
            let det = [0, 1].map(|ch| (1.0 - fb) * input[ch].abs() + fb * self.last_out[ch].abs());
            let loud = det[0].max(det[1]);
            let mut level = [0.0; 2];
            for ch in 0..2 {
                let x = det[ch] + link * (loud - det[ch]);
                self.rms[ch] = rms_c * self.rms[ch] + (1.0 - rms_c) * x * x;
                let peak_db = gain_to_db(x);
                let rms_db = 10.0 * self.rms[ch].max(1e-18).log10() + 3.01; // a sine's RMS reads as its peak
                level[ch] = peak_db + rms_amt * (rms_db - peak_db);
            }
            let delayed = self.ahead.delay(input);
            for ch in 0..2 {
                // The slope comes from the static curve at the input's overshoot (the detected
                // level plus what feedback already took off); applied to the detected level,
                // scaled for feedback (see above), so the steady state is the dialled curve.
                // Below the threshold (the knee's lower half) the curve applies directly.
                let over = level[ch] + fb * self.env[ch] - thr;
                let gr = if over > 0.0 {
                    let s = comp_static(over, ratio, knee, curve) / over;
                    let s_loop = (s / (1.0 - fb * s).max(1e-3)).min(loop_max);
                    (s_loop * (level[ch] - thr)).max(0.0)
                } else {
                    comp_static(over, ratio, knee, curve)
                };
                let c = if gr > self.env[ch] { att } else { rel };
                self.env[ch] = c * self.env[ch] + (1.0 - c) * gr;
            }
            let env = self.env[0].max(self.env[1]);
            self.sustain = sus_c * self.sustain + (1.0 - sus_c) * (env / 12.0).min(1.0);
            self.avg_gr = avg_c * self.avg_gr + (1.0 - avg_c) * env;
            for ch in 0..2 {
                let g = db_to_gain(-self.env[ch] + makeup_db + auto * self.avg_gr);
                let mut y = delayed[ch] * g;
                // The gain stage's colour: asymmetric soft saturation (even harmonics), more
                // drive with more `saturation`.
                if sat > 0.0 {
                    let shaped = ((sat_drive * y + sat_bias).tanh() - sat_bias.tanh()) / sat_drive;
                    y += sat * (shaped - y);
                }
                self.last_out[ch] = y;
                f[ch] = delayed[ch] + wet * (y - delayed[ch]);
            }
        }
        let env = self.env[0].max(self.env[1]);
        ctx.meters[self.meter].store((env / 24.0).clamp(0.0, 1.0), Ordering::Relaxed);
    }
}

/// Transient shaper: a fast and a slow envelope; where the fast one is above the slow one (an
/// attack) the attack gain applies, below it (the sustain) the sustain gain.
struct Transient {
    attack: P,
    sustain: P,
    ahead: Lookahead,
    fast: f32,
    slow: f32,
    gain_db: f32,
    meter: usize,
}

impl Fx for Transient {
    fn latency(&self) -> usize {
        self.ahead.n
    }

    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let sr = ctx.sr;
        let attack = self.attack.update(ctx);
        let sustain = self.sustain.update(ctx);
        let (fa, fr) = (coef(0.5, sr), coef(30.0, sr));
        let (sa, sr_) = (coef(25.0, sr), coef(200.0, sr));
        let gs = coef(1.0, sr);
        for f in frames {
            let x = f[0].abs().max(f[1].abs());
            let c = if x > self.fast { fa } else { fr };
            self.fast = c * self.fast + (1.0 - c) * x;
            let c = if x > self.slow { sa } else { sr_ };
            self.slow = c * self.slow + (1.0 - c) * x;
            let diff = gain_to_db(self.fast) - gain_to_db(self.slow);
            let target = attack * (diff / 6.0).clamp(0.0, 1.0) + sustain * (-diff / 6.0).clamp(0.0, 1.0);
            self.gain_db = gs * self.gain_db + (1.0 - gs) * target;
            let g = db_to_gain(self.gain_db);
            let d = self.ahead.delay(*f);
            *f = [d[0] * g, d[1] * g];
        }
        ctx.meters[self.meter].store((0.5 + self.gain_db / 24.0).clamp(0.0, 1.0), Ordering::Relaxed);
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
    ahead: Lookahead,
    env: f32,
    /// Current attenuation, dB (≥ 0).
    att: f32,
    held: usize,
    meter: usize,
}

impl Fx for Gate {
    fn latency(&self) -> usize {
        self.ahead.n
    }

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
            let d = self.ahead.delay(*f);
            *f = [d[0] * g, d[1] * g];
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
    ahead: Lookahead,
    detector: Biquad,
    shelf: Biquad,
    env: f32,
    meter: usize,
}

impl Fx for DeEsser {
    fn latency(&self) -> usize {
        self.ahead.n
    }

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
            let d = self.ahead.delay(*f);
            *f = [self.shelf.tick(d[0], 0), self.shelf.tick(d[1], 1)];
        }
        ctx.meters[self.meter].store((red / 24.0).clamp(0.0, 1.0), Ordering::Relaxed);
    }
}
