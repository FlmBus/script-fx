//! Modulation: chorus, flanger, phaser, tremolo, auto-pan. LFO rates are Hz or
//! a note length (`sync`) at the host tempo; the right channel's LFO runs a
//! quarter cycle ahead for stereo movement.

use patch::chain::{BlockKind, LfoShape};

use super::{Args, Fx, Lfo, Line, Smooth, mix, pct, rate_hz, wave};
use crate::{Ctx, Frame, P};

pub(super) fn build(kind: &BlockKind, a: &Args) -> Box<dyn Fx> {
    match kind {
        BlockKind::Chorus { rate, sync, depth, mix } => Box::new(Chorus {
            rate: a.p(rate, 0.8),
            sync: *sync,
            depth: a.p(depth, 50.0),
            mix: a.p(mix, 50.0),
            lfo: Lfo::default(),
            lines: [Line::new(a.samples(30.0)), Line::new(a.samples(30.0))],
        }),
        BlockKind::Flanger { rate, sync, depth, feedback, mix } => Box::new(Flanger {
            rate: a.p(rate, 0.3),
            sync: *sync,
            depth: a.p(depth, 70.0),
            feedback: a.p(feedback, 50.0),
            mix: a.p(mix, 50.0),
            lfo: Lfo::default(),
            lines: [Line::new(a.samples(10.0)), Line::new(a.samples(10.0))],
            last: [0.0; 2],
        }),
        BlockKind::Phaser { rate, sync, depth, feedback, freq, stages, mix } => Box::new(Phaser {
            rate: a.p(rate, 0.5),
            sync: *sync,
            depth: a.p(depth, 70.0),
            feedback: a.p(feedback, 30.0),
            freq: a.p(freq, 800.0),
            mix: a.p(mix, 50.0),
            stages: stages.unwrap_or(6).clamp(2, 12) as usize,
            lfo: Lfo::default(),
            z: [[0.0; 12]; 2],
            last: [0.0; 2],
        }),
        BlockKind::Tremolo { rate, sync, phase_offset, depth, shape } => Box::new(Tremolo {
            rate: a.p(rate, 5.0),
            sync: *sync,
            phase_offset: a.p(phase_offset, 0.0),
            depth: a.p(depth, 50.0),
            shape: *shape,
            lfo: Lfo::default(),
            gain: Smooth::new(1.0, 1.0, a.sr),
        }),
        BlockKind::AutoPan { rate, sync, phase_offset, depth, shape } => Box::new(AutoPan {
            rate: a.p(rate, 1.0),
            sync: *sync,
            phase_offset: a.p(phase_offset, 0.0),
            depth: a.p(depth, 100.0),
            shape: *shape,
            lfo: Lfo::default(),
            pan: Smooth::new(0.0, 1.0, a.sr),
        }),
        _ => unreachable!(),
    }
}

/// Two voices per channel at opposite LFO phases, around a 12 ms delay (±4 ms).
struct Chorus {
    rate: P,
    sync: Option<f32>,
    depth: P,
    mix: P,
    lfo: Lfo,
    lines: [Line; 2],
}

impl Fx for Chorus {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let hz = rate_hz(self.rate.update(ctx), self.sync, ctx.tempo);
        let swing = pct(self.depth.update(ctx)) * 0.004 * ctx.sr;
        let base = 0.012 * ctx.sr;
        let wet = pct(self.mix.update(ctx));
        for f in frames {
            // Voices half a cycle apart (±), the right channel a quarter ahead
            // (cos): one sin_cos gives all four.
            let (sin, cos) = (std::f32::consts::TAU * self.lfo.step(hz, ctx.sr)).sin_cos();
            for (ch, m) in [sin, cos].into_iter().enumerate() {
                self.lines[ch].push(f[ch]);
                let v1 = self.lines[ch].tap(base + swing * m);
                let v2 = self.lines[ch].tap(base - swing * m);
                f[ch] = mix(f[ch], 0.5 * (v1 + v2), wet);
            }
        }
    }
}

/// 0.3..(0.3 + 5 × depth) ms swept delay with feedback.
struct Flanger {
    rate: P,
    sync: Option<f32>,
    depth: P,
    feedback: P,
    mix: P,
    lfo: Lfo,
    lines: [Line; 2],
    last: [f32; 2],
}

impl Fx for Flanger {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let hz = rate_hz(self.rate.update(ctx), self.sync, ctx.tempo);
        let min = 0.0003 * ctx.sr;
        let span = pct(self.depth.update(ctx)) * 0.005 * ctx.sr;
        let fb = (self.feedback.update(ctx) / 100.0).clamp(-0.99, 0.99);
        let wet = pct(self.mix.update(ctx));
        for f in frames {
            let p = self.lfo.step(hz, ctx.sr);
            for (ch, ((line, last), x)) in self.lines.iter_mut().zip(&mut self.last).zip(f.iter_mut()).enumerate() {
                let lfo = 0.5 + 0.5 * wave(LfoShape::Triangle, p + 0.25 * ch as f32);
                line.push(*x + fb * *last);
                let y = line.tap(min + span * lfo);
                *last = y;
                *x = mix(*x, y, wet);
            }
        }
    }
}

/// First-order allpass stages swept ±2 octaves (× depth) around `freq`.
struct Phaser {
    rate: P,
    sync: Option<f32>,
    depth: P,
    feedback: P,
    freq: P,
    mix: P,
    stages: usize,
    lfo: Lfo,
    z: [[f32; 12]; 2],
    last: [f32; 2],
}

impl Fx for Phaser {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let hz = rate_hz(self.rate.update(ctx), self.sync, ctx.tempo);
        let depth = pct(self.depth.update(ctx)) * 2.0;
        let fb = (self.feedback.update(ctx) / 100.0).clamp(-0.99, 0.99);
        let center = self.freq.update(ctx).clamp(20.0, ctx.sr * 0.2);
        let wet = pct(self.mix.update(ctx));
        // Coefficients every 8 samples: tan() per sample would dominate the cost.
        for chunk in frames.chunks_mut(8) {
            let p = self.lfo.phase;
            for _ in 0..chunk.len() {
                self.lfo.step(hz, ctx.sr);
            }
            for ch in 0..2 {
                let lfo = wave(LfoShape::Sine, p + 0.25 * ch as f32);
                let f = (center * 2f32.powf(depth * lfo)).clamp(10.0, ctx.sr * 0.45);
                let t = (std::f32::consts::PI * f / ctx.sr).tan();
                let a = (t - 1.0) / (t + 1.0);
                for fr in chunk.iter_mut() {
                    let mut s = fr[ch] + fb * self.last[ch];
                    for z in self.z[ch][..self.stages].iter_mut() {
                        // y = a·x + z; z = x − a·y
                        let y = a * s + *z;
                        *z = s - a * y;
                        s = y;
                    }
                    self.last[ch] = s;
                    fr[ch] = mix(fr[ch], s, wet);
                }
            }
        }
    }
}

/// LFO phase for `Tremolo` / `AutoPan`: synced and the host playing → taken
/// from the beat position (locked to the grid); otherwise free-running. Plus
/// `offset` (fraction of a cycle, from `phase_offset` in degrees). `i` = frame index within this call.
#[inline]
fn grid_phase(lfo: &mut Lfo, hz: f32, sync: Option<f32>, offset: f32, i: usize, ctx: &Ctx) -> f32 {
    let free = lfo.step(hz, ctx.sr);
    let p = match (sync, ctx.beat) {
        (Some(beats), Some(beat)) => {
            let at = beat + i as f64 * ctx.tempo as f64 / 60.0 / ctx.sr as f64;
            let p = (at / beats as f64).rem_euclid(1.0) as f32;
            // Keep the free-running phase in step, so stopping the host doesn't jump.
            lfo.phase = p;
            p
        }
        _ => free,
    };
    p + offset
}

struct Tremolo {
    rate: P,
    sync: Option<f32>,
    /// Degrees.
    phase_offset: P,
    depth: P,
    shape: LfoShape,
    lfo: Lfo,
    /// Smoothed so the square shape doesn't click.
    gain: Smooth,
}

impl Fx for Tremolo {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let hz = rate_hz(self.rate.update(ctx), self.sync, ctx.tempo);
        let offset = self.phase_offset.update(ctx) / 360.0;
        let depth = pct(self.depth.update(ctx));
        for (i, f) in frames.iter_mut().enumerate() {
            let w = wave(self.shape, grid_phase(&mut self.lfo, hz, self.sync, offset, i, ctx));
            let g = self.gain.next(1.0 - depth * (0.5 - 0.5 * w));
            f[0] *= g;
            f[1] *= g;
        }
    }
}

struct AutoPan {
    rate: P,
    sync: Option<f32>,
    /// Degrees.
    phase_offset: P,
    depth: P,
    shape: LfoShape,
    lfo: Lfo,
    pan: Smooth,
}

impl Fx for AutoPan {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let hz = rate_hz(self.rate.update(ctx), self.sync, ctx.tempo);
        let offset = self.phase_offset.update(ctx) / 360.0;
        let depth = pct(self.depth.update(ctx));
        for (i, f) in frames.iter_mut().enumerate() {
            let w = wave(self.shape, grid_phase(&mut self.lfo, hz, self.sync, offset, i, ctx));
            let p = self.pan.next(depth * w);
            // Balance, like Blocks.Pan.
            f[0] *= 1.0 - p.max(0.0);
            f[1] *= 1.0 + p.min(0.0);
        }
    }
}
