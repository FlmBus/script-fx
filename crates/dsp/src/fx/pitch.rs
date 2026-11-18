//! Pitch: delay-line pitch shifter and granular cloud.

use patch::chain::BlockKind;

use super::{Args, Fx, Line, Rng, mix, pct};
use crate::{Ctx, Frame, P};

/// Longest pitch-shift window, ms.
const WINDOW_MAX_MS: f32 = 200.0;
/// How far back the granular buffer reaches, ms.
const GRAIN_BUFFER_MS: f32 = 2500.0;
const MAX_GRAINS: usize = 48;

pub(super) fn build(kind: &BlockKind, a: &Args) -> Box<dyn Fx> {
    match kind {
        BlockKind::PitchShift { semitones, window, mix } => Box::new(PitchShift {
            semitones: a.req(semitones),
            window: a.p(window, 60.0),
            mix: a.p(mix, 100.0),
            cores: [PitchCore::new(a.samples(WINDOW_MAX_MS)), PitchCore::new(a.samples(WINDOW_MAX_MS))],
        }),
        BlockKind::Granular { grain, density, spread, pitch, jitter, reverse, width, mix } => Box::new(Granular {
            grain: a.p(grain, 80.0),
            density: a.p(density, 20.0),
            spread: a.p(spread, 300.0),
            pitch: a.p(pitch, 0.0),
            jitter: a.p(jitter, 0.0),
            reverse: a.p(reverse, 0.0),
            width: a.p(width, 100.0),
            mix: a.p(mix, 50.0),
            lines: [Line::new(a.samples(GRAIN_BUFFER_MS)), Line::new(a.samples(GRAIN_BUFFER_MS))],
            grains: [Grain::default(); MAX_GRAINS],
            rng: Rng::new(0x9E37_79B9),
            due: 0.0,
        }),
        _ => unreachable!(),
    }
}

/// Two read heads sweep through a delay window at the pitch ratio, half a window
/// apart, each faded in and out (triangle) so the jumps back are inaudible.
pub(super) struct PitchCore {
    line: Line,
    phase: f32,
}

impl PitchCore {
    pub fn new(max_window: usize) -> Self {
        PitchCore { line: Line::new(max_window + 2), phase: 0.0 }
    }

    /// `ratio` = playback speed (2 = an octave up), `window` in samples.
    #[inline]
    pub fn process(&mut self, x: f32, ratio: f32, window: f32) -> f32 {
        self.line.push(x);
        let w = window.clamp(16.0, self.line.max_delay());
        self.phase += (1.0 - ratio) / w;
        self.phase -= self.phase.floor();
        let pa = self.phase;
        let pb = (pa + 0.5).fract();
        let ga = 1.0 - (2.0 * pa - 1.0).abs();
        let gb = 1.0 - (2.0 * pb - 1.0).abs();
        self.line.tap(pa * w) * ga + self.line.tap(pb * w) * gb
    }
}

struct PitchShift {
    semitones: P,
    window: P,
    mix: P,
    cores: [PitchCore; 2],
}

impl Fx for PitchShift {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let ratio = 2f32.powf(self.semitones.update(ctx).clamp(-24.0, 24.0) / 12.0);
        let window = self.window.update(ctx).clamp(5.0, WINDOW_MAX_MS) * 0.001 * ctx.sr;
        let wet = pct(self.mix.update(ctx));
        for f in frames {
            for (x, core) in f.iter_mut().zip(&mut self.cores) {
                *x = mix(*x, core.process(*x, ratio, window), wet);
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Grain {
    active: bool,
    /// Read position, samples before the newest input.
    ago: f32,
    /// Change of `ago` per sample (1 − speed; reverse: 1 + speed).
    step: f32,
    len: f32,
    age: f32,
    gains: [f32; 2],
}

struct Granular {
    grain: P,
    density: P,
    spread: P,
    pitch: P,
    jitter: P,
    reverse: P,
    width: P,
    mix: P,
    lines: [Line; 2],
    grains: [Grain; MAX_GRAINS],
    rng: Rng,
    /// Accumulates density / sr; a grain starts each time it passes 1.
    due: f32,
}

impl Fx for Granular {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let len = self.grain.update(ctx).clamp(5.0, 1000.0) * 0.001 * ctx.sr;
        let density = self.density.update(ctx).clamp(0.0, 200.0);
        let spread = self.spread.update(ctx).clamp(0.0, 2000.0) * 0.001 * ctx.sr;
        let pitch = self.pitch.update(ctx).clamp(-24.0, 24.0);
        let jitter = self.jitter.update(ctx).max(0.0) / 100.0;
        let reverse = pct(self.reverse.update(ctx));
        let width = pct(self.width.update(ctx));
        let wet = pct(self.mix.update(ctx));
        let max = self.lines[0].max_delay();
        // Overlapping grains add up; keep the cloud near the input level.
        let norm = 1.0 / (density * len / ctx.sr).max(1.0).sqrt();
        for f in frames {
            self.lines[0].push(f[0]);
            self.lines[1].push(f[1]);
            self.due += density / ctx.sr;
            if self.due >= 1.0 {
                self.due -= 1.0;
                if let Some(g) = self.grains.iter_mut().find(|g| !g.active) {
                    let speed = 2f32.powf((pitch + jitter * self.rng.bipolar()) / 12.0);
                    let backwards = self.rng.next() < reverse;
                    let step = if backwards { 1.0 + speed } else { 1.0 - speed };
                    // Start far enough back that a faster-than-realtime grain never
                    // reads the future, and close enough to stay in the buffer.
                    let min_ago = (len * (speed - 1.0)).max(0.0) + 1.0;
                    let room = (max - len * step.max(0.0) - min_ago).max(0.0);
                    let p = width * self.rng.bipolar();
                    *g = Grain {
                        active: true,
                        ago: min_ago + (self.rng.next() * spread).min(room),
                        step,
                        len,
                        age: 0.0,
                        gains: [1.0 - p.max(0.0), 1.0 + p.min(0.0)],
                    };
                }
            }
            let mut out = [0.0f32; 2];
            for g in self.grains.iter_mut().filter(|g| g.active) {
                let t = g.age / g.len;
                let w = 4.0 * t * (1.0 - t);
                out[0] += self.lines[0].tap(g.ago) * w * g.gains[0];
                out[1] += self.lines[1].tap(g.ago) * w * g.gains[1];
                g.age += 1.0;
                g.ago += g.step;
                if g.age >= g.len || g.ago < 0.0 || g.ago > max {
                    g.active = false;
                }
            }
            f[0] = mix(f[0], out[0] * norm, wet);
            f[1] = mix(f[1], out[1] * norm, wet);
        }
    }
}
