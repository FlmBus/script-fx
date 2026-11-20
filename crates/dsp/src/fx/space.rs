//! Space: delay (with a chain in the feedback path), reverb and shimmer (an
//! 8-line feedback delay network).

use patch::chain::{BlockKind, ReverbSpec};

use super::pitch::PitchCore;
use super::{Allpass, Args, Fx, Line, mix, pct, time_ms};
use crate::{Ctx, Frame, Node, P, chain_latency, coef, process_nodes};

/// Longest delay time, ms.
const DELAY_MAX_MS: f32 = 4000.0;

pub(super) fn build(kind: &BlockKind, a: &Args, meter: &mut usize) -> Box<dyn Fx> {
    match kind {
        BlockKind::Delay { time, sync, feedback, mix, pingpong, feedback_chain } => {
            let chain = crate::build_nodes(&feedback_chain.0, a.params, a.sr, a.max_frames, meter);
            let max = a.samples(DELAY_MAX_MS) + a.max_frames;
            Box::new(Delay {
                time: a.p(time),
                sync: *sync,
                feedback: a.p(feedback),
                mix: a.p(mix),
                pingpong: *pingpong,
                chain_latency: chain_latency(&chain),
                chain,
                lines: [Line::new(max), Line::new(max)],
                cur: f32::NAN,
                wet: vec![[0.0; 2]; a.max_frames],
                fb: vec![[0.0; 2]; a.max_frames],
            })
        }
        BlockKind::Reverb(r) => Box::new(Reverb::new(a, r, false)),
        BlockKind::Shimmer(r) => Box::new(Reverb::new(a, r, true)),
        _ => unreachable!(),
    }
}

struct Delay {
    time: P,
    sync: Option<f32>,
    feedback: P,
    mix: P,
    pingpong: bool,
    chain: Vec<Node>,
    chain_latency: usize,
    lines: [Line; 2],
    /// Current delay, samples (glides to the target, like tape).
    cur: f32,
    wet: Vec<Frame>,
    fb: Vec<Frame>,
}

impl Fx for Delay {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let n = frames.len();
        // Repeats are read for the whole block before it is written, so the delay
        // must be at least one block. A feedback chain with latency L returns each
        // repeat L samples late: it is written L samples into the past (not read
        // yet as long as the delay ≥ L + one block), so the repeats stay on time.
        let lat = self.chain_latency;
        let ms = time_ms(self.time.update(ctx), self.sync, ctx.tempo).clamp(1.0, DELAY_MAX_MS);
        let min = (lat + n) as f32;
        let target = (ms * 0.001 * ctx.sr).clamp(min, self.lines[0].max_delay() - n as f32);
        if self.cur.is_nan() {
            self.cur = target;
        }
        let glide = 1.0 - coef(50.0, ctx.sr);
        let fb = pct(self.feedback.update(ctx));
        let wet = pct(self.mix.update(ctx));
        for i in 0..n {
            self.cur += glide * (target - self.cur);
            // `cur` samples before frame i = (cur - 1 - i) before the newest written one.
            let ago = (self.cur - 1.0 - i as f32).max(0.0);
            self.wet[i] = [self.lines[0].tap(ago), self.lines[1].tap(ago)];
            self.fb[i] = [self.wet[i][0] * fb, self.wet[i][1] * fb];
        }
        process_nodes(&mut self.chain, &mut self.fb[..n], ctx);
        for (i, f) in frames.iter_mut().enumerate() {
            let r = self.fb[i];
            let (l_dry, r_dry) = if self.pingpong { (0.5 * (f[0] + f[1]), 0.0) } else { (f[0], f[1]) };
            // Ping-pong: input enters on the left; each repeat crosses to the other side.
            let (l_in, r_in) = if self.pingpong { (l_dry + r[1], r_dry + r[0]) } else { (l_dry + r[0], r_dry + r[1]) };
            let (l_fb, r_fb) = (l_in - l_dry, r_in - r_dry);
            self.lines[0].push(l_dry);
            self.lines[1].push(r_dry);
            // Safety clamp: a feedback chain with gain must not run away to infinity.
            for (line, fb) in self.lines.iter_mut().zip([l_fb, r_fb]) {
                let cur = line.at(lat);
                line.add(lat, (cur + fb).clamp(-4.0, 4.0) - cur);
            }
            f[0] = mix(f[0], self.wet[i][0], wet);
            f[1] = mix(f[1], self.wet[i][1], wet);
        }
    }
}

const LINES: usize = 8;
/// Line lengths at size 100 %, ms (mutually prime-ish).
const LINE_MS: [f32; LINES] = [29.7, 37.1, 41.1, 43.7, 53.0, 59.9, 67.7, 73.1];
/// Input diffusers, ms.
const DIFFUSER_MS: [f32; 4] = [4.77, 3.59, 12.73, 9.31];
const PREDELAY_MAX_MS: f32 = 500.0;

struct Shimmer {
    pitch: P,
    amount: P,
    core: PitchCore,
}

/// Feedback delay network: 8 lines, Householder feedback matrix, damping in the
/// loop, input diffusion and predelay. With `shimmer`, the output is
/// pitch-shifted back into the input.
struct Reverb {
    size: P,
    decay: P,
    damping: P,
    predelay: P,
    width: P,
    mix: P,
    freeze: P,
    shimmer: Option<Shimmer>,
    pre: Line,
    diffusers: Vec<Allpass>,
    lines: Vec<Line>,
    lp: [f32; LINES],
    last: f32,
}

impl Reverb {
    fn new(a: &Args, r: &ReverbSpec, shimmer: bool) -> Self {
        Reverb {
            size: a.p(&r.size),
            decay: a.p(&r.decay),
            damping: a.p(&r.damping),
            predelay: a.p(&r.predelay),
            width: a.p(&r.width),
            mix: a.p(&r.mix),
            // Reverb only (Shimmer has none: never frozen).
            freeze: r.freeze.as_ref().map_or(P::constant(0.0), |x| a.p(x)),
            shimmer: shimmer.then(|| Shimmer {
                pitch: r.pitch.as_ref().map_or(P::constant(12.0), |x| a.p(x)),
                amount: r.amount.as_ref().map_or(P::constant(50.0), |x| a.p(x)),
                core: PitchCore::new(a.samples(100.0)),
            }),
            pre: Line::new(a.samples(PREDELAY_MAX_MS)),
            diffusers: DIFFUSER_MS.iter().map(|&ms| Allpass::new(a.samples(ms), 0.7)).collect(),
            lines: LINE_MS.iter().map(|&ms| Line::new(a.samples(ms) + 2)).collect(),
            lp: [0.0; LINES],
            last: 0.0,
        }
    }
}

impl Fx for Reverb {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let size = 0.25 + 0.75 * pct(self.size.update(ctx));
        let rt60 = self.decay.update(ctx).clamp(0.05, 60.0);
        let frozen = self.freeze.update(ctx) > 0.5;
        let damp = if frozen { 0.0 } else { pct(self.damping.update(ctx)) * 0.85 };
        let predelay = (self.predelay.update(ctx).clamp(0.0, PREDELAY_MAX_MS) * 0.001 * ctx.sr).min(self.pre.max_delay());
        let width = (self.width.update(ctx) / 100.0).clamp(0.0, 2.0);
        let wet = pct(self.mix.update(ctx));
        let mut len = [0.0f32; LINES];
        let mut gain = [0.0f32; LINES];
        for i in 0..LINES {
            len[i] = (LINE_MS[i] * size * 0.001 * ctx.sr).clamp(1.0, self.lines[i].max_delay());
            // -60 dB after rt60 seconds: per pass through a line of len[i] samples.
            gain[i] = if frozen { 1.0 } else { 10f32.powf(-3.0 * len[i] / ctx.sr / rt60) };
        }
        let (ratio, amount) = match &mut self.shimmer {
            Some(s) => (2f32.powf(s.pitch.update(ctx).clamp(-24.0, 24.0) / 12.0), pct(s.amount.update(ctx)) * 4.0),
            None => (1.0, 0.0),
        };
        let in_gain = if frozen { 0.0 } else { 1.0 / (LINES as f32).sqrt() };
        let mean_gain = gain.iter().sum::<f32>() / LINES as f32;
        for f in frames {
            self.pre.push(0.5 * (f[0] + f[1]));
            let mut v = self.pre.tap(predelay);
            for d in &mut self.diffusers {
                v = d.tick(v);
            }
            if let Some(s) = &mut self.shimmer {
                // The tail builds up by ~1 / (1 − line gain); scale the pitched
                // feedback by that so the outer loop stays below unity, and
                // soft-limit it on top.
                let fb = amount * (1.0 - mean_gain) * s.core.process(self.last, ratio, 0.05 * ctx.sr);
                v += fb.tanh();
            }
            let mut u = [0.0f32; LINES];
            let mut sum = 0.0;
            for i in 0..LINES {
                let s = self.lines[i].tap(len[i] - 1.0);
                self.lp[i] += (1.0 - damp) * (s - self.lp[i]);
                u[i] = self.lp[i];
                sum += u[i];
            }
            // Householder: u - 2/N · sum(u) (energy-preserving).
            let m = sum * 2.0 / LINES as f32;
            let (mut l, mut r) = (0.0, 0.0);
            for i in 0..LINES {
                let sign = if i % 4 < 2 { 1.0 } else { -1.0 };
                self.lines[i].push(v * in_gain * sign + gain[i] * (u[i] - m));
                if i % 2 == 0 {
                    l += u[i] * sign;
                } else {
                    r += u[i] * sign;
                }
            }
            let (l, r) = (l * 0.5, r * 0.5);
            self.last = 0.5 * (l + r);
            let mid = 0.5 * (l + r);
            let side = 0.5 * (l - r) * width;
            f[0] = mix(f[0], mid + side, wet);
            f[1] = mix(f[1], mid - side, wet);
        }
    }
}
