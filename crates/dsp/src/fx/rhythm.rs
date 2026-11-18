//! Rhythm: stutter / beat repeat.

use patch::chain::BlockKind;

use super::{Args, Fx, Line, Smooth, mix, pct, time_ms};
use crate::{Ctx, Frame, P};

/// Longest slice, ms.
const SLICE_MAX_MS: f32 = 4000.0;
/// Fade at slice edges and when switching on/off, ms.
const FADE_MS: f32 = 2.0;

pub(super) fn build(kind: &BlockKind, a: &Args) -> Box<dyn Fx> {
    match kind {
        BlockKind::Stutter { active, length, sync, mix } => Box::new(Stutter {
            active: a.req(active),
            length: a.p(length, 125.0),
            sync: *sync,
            mix: a.p(mix, 100.0),
            lines: [Line::new(a.samples(SLICE_MAX_MS)), Line::new(a.samples(SLICE_MAX_MS))],
            on: false,
            elapsed: 0,
            slice: 1,
            gate: Smooth::new(0.0, FADE_MS, a.sr),
        }),
        _ => unreachable!(),
    }
}

/// While active, the last `length` before the trigger repeats. Recording stops
/// while it repeats (so the slice stays put without copying it anywhere).
struct Stutter {
    active: P,
    length: P,
    sync: Option<f32>,
    mix: P,
    lines: [Line; 2],
    on: bool,
    elapsed: usize,
    slice: usize,
    gate: Smooth,
}

impl Fx for Stutter {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let want = self.active.update(ctx) > 0.5;
        let fade = (FADE_MS * 0.001 * ctx.sr).max(1.0);
        let ms = time_ms(self.length.update(ctx), self.sync, ctx.tempo).clamp(5.0, SLICE_MAX_MS);
        let len = ((ms * 0.001 * ctx.sr) as usize).clamp(2 * fade as usize + 1, self.lines[0].max_delay() as usize);
        let wet = pct(self.mix.update(ctx));
        for f in frames {
            if want && !self.on {
                self.on = true;
                self.elapsed = 0;
                self.slice = len;
            }
            if !self.on {
                self.lines[0].push(f[0]);
                self.lines[1].push(f[1]);
                continue;
            }
            let g = self.gate.next(if want { 1.0 } else { 0.0 });
            if !want && g < 1e-3 {
                self.on = false;
                self.gate.value = 0.0;
            }
            let k = self.elapsed % self.slice;
            let ago = self.slice - 1 - k;
            let edge = ((k + 1) as f32 / fade).min((self.slice - k) as f32 / fade).min(1.0);
            for (x, line) in f.iter_mut().zip(&self.lines) {
                let s = line.at(ago) * edge;
                *x = mix(*x, mix(*x, s, g), wet);
            }
            self.elapsed += 1;
        }
    }
}
