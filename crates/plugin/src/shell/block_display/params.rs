//! The displays drawn from the knobs alone (`docs/research/block-displays.md` §1, step 2; the
//! designs are `docs/research/mockups/block-displays.html`).
//!
//! These are **illustrations** of what the settings do, not the DSP's output: the filter curves are
//! analog prototypes, the reverb's decay per band and the tape's colour are approximations. The
//! modulation blocks draw their LFO at phase 0 (the live phase comes with the taps, step 3), the
//! containers their routing without levels.

use std::f32::consts::{PI, TAU};

use patch::chain::{BlockKind, BlockSpec, Chain, Channels, DynamicMode, LfoShape, ParamRef, RectifyMode, Side};

use super::super::theme;
use super::{Plot, curve, dashed, faint, grid, handle, label, muted, polyline, shaper, zero};
use nice_plug_iced::iced::{
    Color, Pixels, Point, Rectangle, Size,
    alignment,
    widget::canvas::{Frame, Path, Stroke, Text},
};

type V<'a> = &'a dyn Fn(&ParamRef) -> f32;

/// Can this block be drawn from its knobs?
pub(super) fn draws(kind: &BlockKind) -> bool {
    matches!(
        kind,
        BlockKind::MorphFilter { .. }
            | BlockKind::LadderFilter { .. }
            | BlockKind::CombFilter { .. }
            | BlockKind::Formant { .. }
            | BlockKind::Resonator { .. }
            | BlockKind::DynamicEq { .. }
            | BlockKind::Transient { .. }
            | BlockKind::Gate { .. }
            | BlockKind::DeEsser { .. }
            | BlockKind::Rider { .. }
            | BlockKind::Bitcrusher { .. }
            | BlockKind::RingMod { .. }
            | BlockKind::Rectifier { .. }
            | BlockKind::Tape { .. }
            | BlockKind::Reverb(_)
            | BlockKind::Shimmer(_)
            | BlockKind::PitchShift { .. }
            | BlockKind::Pan { .. }
            | BlockKind::Haas { .. }
            | BlockKind::MonoBass { .. }
            | BlockKind::Invert { .. }
            | BlockKind::Chorus { .. }
            | BlockKind::Flanger { .. }
            | BlockKind::Phaser { .. }
            | BlockKind::Tremolo { .. }
            | BlockKind::AutoPan { .. }
            | BlockKind::BandSplit { .. }
            | BlockKind::Parallel { .. }
            | BlockKind::MidSide { .. }
            | BlockKind::LeftRight { .. }
    )
}

/// The main settings, for the caption.
pub(super) fn caption(kind: &BlockKind, v: V, show: &dyn Fn(&ParamRef) -> String) -> String {
    let join = |parts: &[(&str, &ParamRef)]| parts.iter().map(|(l, r)| format!("{l}{}", show(r))).collect::<Vec<_>>().join(" · ");
    let rate = |rate: &ParamRef, sync: &Option<f32>| match sync {
        Some(beats) => format!("{} beat{}", trim(*beats), if *beats == 1.0 { "" } else { "s" }),
        None => show(rate),
    };
    match kind {
        BlockKind::MorphFilter { freq, morph, resonance } => join(&[("", freq), ("morph ", morph), ("res ", resonance)]),
        BlockKind::LadderFilter { freq, resonance, drive } => join(&[("", freq), ("res ", resonance), ("drive ", drive)]),
        BlockKind::CombFilter { freq, feedback, .. } => join(&[("", freq), ("feedback ", feedback)]),
        BlockKind::Formant { vowel, shift, .. } => format!("vowel {} · shift {}", vowel_name(v(vowel)), show(shift)),
        BlockKind::Resonator { notes, decay, .. } => format!("{} · decay {}", notes.iter().map(|n| note_name(*n)).collect::<Vec<_>>().join(" "), show(decay)),
        BlockKind::DynamicEq { freq, threshold, range, mode, .. } => {
            format!("{} · {} up to {} above {}", show(freq), if *mode == DynamicMode::Cut { "cut" } else { "boost" }, show(range), show(threshold))
        }
        BlockKind::Transient { attack, sustain, .. } => join(&[("attack ", attack), ("sustain ", sustain)]),
        BlockKind::Gate { threshold, ratio, range, .. } => join(&[("", threshold), ("ratio ", ratio), ("range ", range)]),
        BlockKind::DeEsser { freq, threshold, range, .. } => join(&[("above ", freq), ("", threshold), ("range ", range)]),
        BlockKind::Rider { target, range, speed } => join(&[("target ", target), ("± ", range), ("", speed)]),
        BlockKind::Bitcrusher { bits, rate, .. } => match rate {
            Some(r) => format!("{} bit · {}", v(bits).round(), show(r)),
            None => format!("{} bit", v(bits).round()),
        },
        BlockKind::RingMod { freq, mix } => join(&[("carrier ", freq), ("mix ", mix)]),
        BlockKind::Rectifier { mode, mix } => format!("{} wave · mix {}", if *mode == RectifyMode::Full { "full" } else { "half" }, show(mix)),
        BlockKind::Tape { drive, wow, flutter, .. } => join(&[("drive ", drive), ("wow ", wow), ("flutter ", flutter)]),
        BlockKind::Reverb(r) => join(&[("", &r.decay), ("damping ", &r.damping), ("predelay ", &r.predelay)]),
        BlockKind::Shimmer(r) => match (&r.pitch, &r.amount) {
            (Some(p), Some(a)) => format!("{} · {} · amount {}", show(&r.decay), show(p), show(a)),
            _ => show(&r.decay),
        },
        BlockKind::PitchShift { semitones, window, .. } => join(&[("", semitones), ("window ", window)]),
        BlockKind::Pan { pan } => {
            let p = v(pan);
            format!("{} ({})", show(pan), if p < -0.5 { "left" } else if p > 0.5 { "right" } else { "centre" })
        }
        BlockKind::Haas { delay, channel } => format!("{} +{}", if *channel == Side::Left { "left" } else { "right" }, show(delay)),
        BlockKind::MonoBass { freq } => format!("mono below {}", show(freq)),
        BlockKind::Invert { channels } => match channels {
            Channels::Both => "both channels inverted".into(),
            Channels::Left => "left channel inverted".into(),
            Channels::Right => "right channel inverted".into(),
        },
        BlockKind::Chorus { rate: r, sync, depth, .. } => format!("{} · depth {}", rate(r, sync), show(depth)),
        BlockKind::Flanger { rate: r, sync, depth, feedback, .. } => format!("{} · depth {} · feedback {}", rate(r, sync), show(depth), show(feedback)),
        BlockKind::Phaser { rate: r, sync, freq, stages, .. } => format!("{} stages · {} · centre {}", stages.unwrap_or(4), rate(r, sync), show(freq)),
        BlockKind::Tremolo { rate: r, sync, depth, shape, .. } | BlockKind::AutoPan { rate: r, sync, depth, shape, .. } => {
            format!("{} · depth {} · {}", rate(r, sync), show(depth), shape_name(*shape))
        }
        BlockKind::BandSplit { crossovers, .. } => format!("LR4 · {}", crossovers.iter().map(show).collect::<Vec<_>>().join(" · ")),
        BlockKind::Parallel { branches, mix } => match mix {
            Some(m) => format!("{} branches · mix {}", branches.len(), show(m)),
            None => format!("{} branches, summed", branches.len()),
        },
        BlockKind::MidSide { .. } => "encode · mid ‖ side · decode".into(),
        BlockKind::LeftRight { .. } => "left ‖ right".into(),
        _ => String::new(),
    }
}

/// Draws the block's display into `plot`.
pub(super) fn draw(f: &mut Frame, plot: Plot, kind: &BlockKind, v: V) {
    match kind {
        BlockKind::MorphFilter { freq, morph, resonance } => morph_filter(f, plot, v(freq), v(morph) / 100.0, v(resonance) / 100.0),
        BlockKind::LadderFilter { freq, resonance, .. } => ladder(f, plot, v(freq), v(resonance) / 100.0),
        BlockKind::CombFilter { freq, feedback, mix } => comb(f, plot, v(freq), v(feedback) / 100.0, v(mix) / 100.0),
        BlockKind::Formant { vowel, shift, resonance, mix } => formant(f, plot, v(vowel), v(shift), v(resonance) / 100.0, v(mix) / 100.0),
        BlockKind::Resonator { notes, decay, mix } => resonator(f, plot, notes, v(decay), v(mix) / 100.0),
        BlockKind::DynamicEq { freq, q, range, mode, .. } => dynamic_eq(f, plot, v(freq), v(q), v(range), *mode),
        BlockKind::Transient { attack, sustain, .. } => transient(f, plot, v(attack), v(sustain)),
        BlockKind::Gate { threshold, ratio, range, .. } => gate(f, plot, v(threshold), v(ratio), v(range)),
        BlockKind::DeEsser { freq, range, .. } => de_esser(f, plot, v(freq), v(range)),
        BlockKind::Rider { target, range, .. } => rider(f, plot, v(target), v(range)),
        BlockKind::Bitcrusher { bits, rate, mix } => bitcrusher(f, plot, v(bits), rate.as_ref().map_or(48_000.0, v), v(mix) / 100.0),
        BlockKind::RingMod { freq, mix } => ring_mod(f, plot, v(freq), v(mix) / 100.0),
        BlockKind::Rectifier { mode, mix } => {
            let (full, mix) = (*mode == RectifyMode::Full, v(mix) / 100.0);
            let shape = move |x: f32| Some(x + mix * (if full { x.abs() } else { x.max(0.0) } - x));
            let _ = shaper(f, plot, &shape, None);
        }
        BlockKind::Tape { drive, wow, flutter, .. } => tape(f, plot, v(drive), v(wow) / 100.0, v(flutter) / 100.0),
        BlockKind::Reverb(r) => reverb(f, plot, v(&r.decay), v(&r.damping) / 100.0, v(&r.predelay)),
        BlockKind::Shimmer(r) => shimmer(f, plot, v(&r.decay), r.pitch.as_ref().map_or(12.0, v), r.amount.as_ref().map_or(50.0, v) / 100.0),
        BlockKind::PitchShift { semitones, .. } => pitch_shift(f, plot, v(semitones)),
        BlockKind::Pan { pan } => pan_view(f, plot, v(pan) / 100.0),
        BlockKind::Haas { delay, channel } => haas(f, plot, v(delay), *channel),
        BlockKind::MonoBass { freq } => mono_bass(f, plot, v(freq)),
        BlockKind::Invert { channels } => invert(f, plot, *channels),
        BlockKind::Chorus { depth, .. } => chorus(f, plot, v(depth) / 100.0),
        BlockKind::Flanger { depth, feedback, .. } => flanger(f, plot, v(depth) / 100.0, v(feedback) / 100.0),
        BlockKind::Phaser { freq, stages, mix, .. } => phaser(f, plot, v(freq), stages.unwrap_or(4), v(mix) / 100.0),
        BlockKind::Tremolo { depth, shape, phase_offset, .. } => tremolo(f, plot, v(depth) / 100.0, *shape, v(phase_offset)),
        BlockKind::AutoPan { depth, shape, phase_offset, .. } => auto_pan(f, plot, v(depth) / 100.0, *shape, v(phase_offset)),
        BlockKind::BandSplit { crossovers, .. } => band_split(f, plot, &crossovers.iter().map(v).collect::<Vec<_>>()),
        BlockKind::Parallel { branches, mix } => {
            let wet = branches.iter().filter(|c| !c.0.is_empty()).count();
            let mut n = 0;
            let lanes = branches
                .iter()
                .map(|c| {
                    let name = if c.0.is_empty() {
                        "dry".to_string()
                    } else {
                        n += 1;
                        if wet == 1 { "wet".to_string() } else { format!("wet {n}") }
                    };
                    (name, c)
                })
                .collect::<Vec<_>>();
            routing(f, plot, &lanes, "∥", "Σ", mix.as_ref().map(|m| v(m) / 100.0));
        }
        BlockKind::MidSide { mid, side } => routing(f, plot, &[("mid".into(), mid), ("side".into(), side)], "M/S", "L/R", None),
        BlockKind::LeftRight { left, right } => routing(f, plot, &[("left".into(), left), ("right".into(), right)], "L | R", "L+R", None),
        _ => {}
    }
}

// ---- a frequency plot ----------------------------------------------------------------------------

/// Magnitude in dB over `f0`…`f1` Hz (log); the axis labels go below `plot`.
struct Freq {
    p: Plot,
    f0: f32,
    f1: f32,
    lo: f32,
    hi: f32,
}

impl Freq {
    /// Room for the labels at the bottom.
    fn new(plot: Plot, f0: f32, f1: f32, lo: f32, hi: f32) -> Self {
        Self { p: Plot { h: plot.h - 12.0, ..plot }, f0, f1, lo, hi }
    }

    fn x(&self, hz: f32) -> f32 {
        self.p.px(((hz / self.f0).max(1e-6).ln() / (self.f1 / self.f0).ln()).clamp(-0.01, 1.01))
    }

    fn y(&self, db: f32) -> f32 {
        self.p.py(((db - self.lo) / (self.hi - self.lo)).clamp(-0.01, 1.01))
    }

    fn at(&self, hz: f32, db: f32) -> Point {
        Point::new(self.x(hz), self.y(db))
    }

    fn grid(&self, f: &mut Frame) {
        for hz in [20.0f32, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0] {
            if hz > self.f0 * 1.01 && hz < self.f1 * 0.99 {
                let x = self.x(hz);
                f.stroke(&Path::line(Point::new(x, self.p.y), Point::new(x, self.p.y + self.p.h)), grid());
                if [100.0, 1000.0, 10000.0].contains(&hz) || self.f1 / self.f0 < 50.0 {
                    label(f, &hz_short(hz), Point::new(x, self.p.y + self.p.h + 4.0), alignment::Horizontal::Center, faint());
                }
            }
        }
        let mut d = (self.lo / 12.0).ceil() * 12.0;
        while d <= self.hi {
            f.stroke(&Path::line(Point::new(self.p.x, self.y(d)), Point::new(self.p.x + self.p.w, self.y(d))), if d == 0.0 { zero() } else { grid() });
            d += 12.0;
        }
    }

    /// The curve of `db(hz)`, `n` points.
    fn path(&self, n: usize, db: impl Fn(f32) -> f32) -> Path {
        Path::new(|p| polyline(p, (0..=n).map(|i| {
            let hz = self.f0 * (self.f1 / self.f0).powf(i as f32 / n as f32);
            self.at(hz, db(hz))
        })))
    }

    /// The area between the curve and 0 dB.
    fn fill(&self, f: &mut Frame, n: usize, db: impl Fn(f32) -> f32) {
        let y0 = self.y(0.0);
        let area = Path::new(|p| {
            p.move_to(Point::new(self.p.x, y0));
            for i in 0..=n {
                let hz = self.f0 * (self.f1 / self.f0).powf(i as f32 / n as f32);
                p.line_to(self.at(hz, db(hz)));
            }
            p.line_to(Point::new(self.p.x + self.p.w, y0));
            p.close();
        });
        f.fill(&area, ink(0.07));
    }
}

// Analog prototypes: magnitudes of second-order sections at w = f / fc.
fn m2(a2: f32, a1: f32, a0: f32, w: f32) -> f32 {
    (a0 - a2 * w * w).hypot(a1 * w)
}
fn lp2(f: f32, fc: f32, q: f32) -> f32 {
    1.0 / m2(1.0, 1.0 / q, 1.0, f / fc)
}
fn hp2(f: f32, fc: f32, q: f32) -> f32 {
    let w = f / fc;
    w * w / m2(1.0, 1.0 / q, 1.0, w)
}
fn bp2(f: f32, fc: f32, q: f32) -> f32 {
    let w = f / fc;
    (w / q) / m2(1.0, 1.0 / q, 1.0, w)
}
fn peak(f: f32, fc: f32, q: f32, gain_db: f32) -> f32 {
    let a = 10f32.powf(gain_db / 40.0);
    let w = f / fc;
    m2(1.0, a / q, 1.0, w) / m2(1.0, 1.0 / (a * q), 1.0, w)
}
fn high_shelf(f: f32, fc: f32, q: f32, gain_db: f32) -> f32 {
    let a = 10f32.powf(gain_db / 40.0);
    let s = a.sqrt();
    let w = f / fc;
    a * m2(a, s / q, 1.0, w) / m2(1.0, s / q, a, w)
}
fn db(x: f32) -> f32 {
    20.0 * x.max(1e-6).log10()
}

// ---- filters ---------------------------------------------------------------------------------------

/// LP (0) → BP (0.5) → HP (1), and where the morph is on that track.
fn morph_filter(f: &mut Frame, plot: Plot, fc: f32, morph: f32, res: f32) {
    let q = 0.707 + res * 8.0;
    let mag = |hz: f32| {
        let (l, b, h) = (lp2(hz, fc, q), bp2(hz, fc, q), hp2(hz, fc, q));
        db(if morph < 0.5 { lerp(l, b, morph * 2.0) } else { lerp(b, h, morph * 2.0 - 1.0) })
    };
    let fr = Freq::new(plot, 20.0, 20000.0, -36.0, 24.0);
    fr.grid(f);
    f.stroke(&fr.path(240, mag), curve());
    handle(f, fr.at(fc, mag(fc)));
    track(f, Point::new(plot.x + plot.w - 112.0, plot.y + 6.0), 100.0, &["LP", "BP", "HP"], morph);
}

/// The 4-pole ladder's low pass with its resonance peak.
fn ladder(f: &mut Frame, plot: Plot, fc: f32, res: f32) {
    let mag = |hz: f32| db(lp2(hz, fc, 0.54) * lp2(hz, fc, 0.54 + res * 6.0));
    let fr = Freq::new(plot, 20.0, 20000.0, -48.0, 24.0);
    fr.grid(f);
    f.stroke(&fr.path(240, mag), curve());
    handle(f, fr.at(fc, mag(fc)));
}

/// The comb on a linear axis, so its teeth are evenly spaced (dry and wet mixed).
fn comb(f: &mut Frame, plot: Plot, f0: f32, g: f32, mix: f32) {
    let p = Plot { h: plot.h - 12.0, ..plot };
    let span = (f0 * 16.0).clamp(500.0, 20000.0);
    let (lo, hi) = (-12.0, 12.0);
    let x = |hz: f32| p.px(hz / span);
    let y = |d: f32| p.py(((d - lo) / (hi - lo)).clamp(-0.01, 1.01));
    let mag = |hz: f32| {
        let th = TAU * hz / f0;
        let (re, im) = (1.0 - g * th.cos(), g * th.sin());
        let n = re * re + im * im;
        // dry + wet · 1 / (1 − g·e^(−jθ))
        let (wr, wi) = (re / n, -im / n);
        db(((1.0 - mix) + mix * wr).hypot(mix * wi))
    };
    let step = nice_step(span / 5.0);
    let mut t = step;
    while t < span {
        f.stroke(&Path::line(Point::new(x(t), p.y), Point::new(x(t), p.y + p.h)), grid());
        label(f, &hz_short(t), Point::new(x(t), p.y + p.h + 4.0), alignment::Horizontal::Center, faint());
        t += step;
    }
    f.stroke(&Path::line(Point::new(p.x, y(0.0)), Point::new(p.x + p.w, y(0.0))), zero());
    let n = 600;
    f.stroke(&Path::new(|b| polyline(b, (0..=n).map(|i| { let hz = span * i as f32 / n as f32; Point::new(x(hz), y(mag(hz))) }))), curve().with_width(1.6));
    handle(f, Point::new(x(f0), y(mag(f0))));
}

/// Formants of the vowels a e i o u (F1–F3, Hz).
const VOWELS: [[f32; 3]; 5] = [[730.0, 1090.0, 2440.0], [530.0, 1840.0, 2480.0], [270.0, 2290.0, 3010.0], [570.0, 840.0, 2410.0], [300.0, 870.0, 2240.0]];

/// The formant peaks of the current vowel, and the vowel on an a e i o u track.
fn formant(f: &mut Frame, plot: Plot, vowel: f32, shift: f32, res: f32, mix: f32) {
    let vw = vowel.clamp(0.0, 4.0);
    let (i, t) = ((vw.floor() as usize).min(3), vw - vw.floor().min(3.0));
    let k = 2f32.powf(shift / 12.0);
    let formants: Vec<f32> = (0..3).map(|j| lerp(VOWELS[i][j], VOWELS[i + 1][j], t) * k).collect();
    let bw = 220.0 * (1.0 - 0.8 * res) + 30.0;
    let gains = [1.0, 0.55, 0.3];
    let mag = |hz: f32| db((1.0 - mix) + mix * 2.0 * formants.iter().zip(gains).map(|(fc, g)| g * bp2(hz, *fc, fc / bw)).sum::<f32>());
    let fr = Freq::new(plot, 80.0, 8000.0, -36.0, 18.0);
    fr.grid(f);
    fr.fill(f, 240, mag);
    f.stroke(&fr.path(240, mag), curve());
    for (j, fc) in formants.iter().enumerate() {
        label(f, &format!("F{}", j + 1), Point::new(fr.x(*fc), fr.p.y + fr.p.h - 12.0), alignment::Horizontal::Center, faint());
    }
    track(f, Point::new(plot.x + plot.w - 112.0, plot.y + 6.0), 100.0, &["a", "e", "i", "o", "u"], vw / 4.0);
}

/// The tuned notes as sharp peaks, labelled.
fn resonator(f: &mut Frame, plot: Plot, notes: &[f32], decay_ms: f32, mix: f32) {
    if notes.is_empty() {
        return;
    }
    let (min, max) = notes.iter().fold((f32::MAX, 0f32), |(a, b), n| (a.min(*n), b.max(*n)));
    let (mut f0, mut f1) = ((min / 1.6).max(20.0), (max * 1.6).min(20000.0));
    if f1 / f0 < 6.0 {
        let c = (f0 * f1).sqrt();
        (f0, f1) = (c / 6f32.sqrt(), c * 6f32.sqrt());
    }
    // Rings for `decay` (to −60 dB): bandwidth 6.9 / (π · decay).
    let bw = 6.9 / (PI * (decay_ms / 1000.0).max(0.01));
    let mag = |hz: f32| db((1.0 - mix) * 0.05 + mix * notes.iter().map(|n| bp2(hz, *n, (n / bw).min(800.0))).sum::<f32>());
    let fr = Freq::new(plot, f0, f1, -42.0, 12.0);
    fr.grid(f);
    f.stroke(&fr.path(800, mag), curve().with_width(1.6));
    for n in notes {
        label(f, &note_name(*n), Point::new(fr.x(*n), (fr.y(mag(*n)) - 13.0).max(plot.y)), alignment::Horizontal::Center, muted());
    }
}

/// The band at full range (dashed) around the current curve (flat until the band gets loud;
/// what it does right now comes with the live data).
fn dynamic_eq(f: &mut Frame, plot: Plot, fc: f32, q: f32, range: f32, mode: DynamicMode) {
    let full = if mode == DynamicMode::Cut { -range } else { range };
    let fr = Freq::new(plot, 20.0, 20000.0, -24.0, 24.0);
    fr.grid(f);
    f.stroke(&fr.path(240, |hz| db(peak(hz, fc, q.max(0.1), full))), dashed(muted()).with_width(1.3));
    f.stroke(&Path::line(Point::new(fr.p.x, fr.y(0.0)), Point::new(fr.p.x + fr.p.w, fr.y(0.0))), curve());
    handle(f, fr.at(fc, 0.0));
}

// ---- dynamics --------------------------------------------------------------------------------------

/// One hit before (grey) and after (ink): the attack lifted or softened, the tail longer or shorter.
fn transient(f: &mut Frame, plot: Plot, attack_db: f32, sustain_db: f32) {
    const N: usize = 200;
    let (boost, sus) = (10f32.powf(attack_db / 20.0), sustain_db / 24.0);
    let env = |u: f32| if u < 0.05 { u / 0.05 } else { (-(u - 0.05) * 5.0).exp() };
    let shaped = |u: f32| {
        let a = if u < 0.05 { lerp(1.0, boost, u / 0.05) } else if u < 0.15 { lerp(boost, 1.0, (u - 0.05) / 0.1) } else { 1.0 };
        let s = if u > 0.15 { (sus * (u - 0.15) * 6.0).exp() } else { 1.0 };
        env(u) * a * s
    };
    let scale = 0.62 / boost.max(1.0);
    let at = |u: f32, y: f32| Point::new(plot.px(u), plot.py((y * scale).clamp(0.0, 1.0)));
    let area = Path::new(|p| {
        p.move_to(Point::new(plot.x, plot.y + plot.h));
        (0..=N).for_each(|i| { let u = i as f32 / N as f32; p.line_to(at(u, env(u))) });
        p.line_to(Point::new(plot.x + plot.w, plot.y + plot.h));
        p.close();
    });
    f.fill(&area, ink(0.1));
    f.stroke(&Path::new(|p| polyline(p, (0..=N).map(|i| { let u = i as f32 / N as f32; at(u, shaped(u)) }))), curve());
    label(f, "in ▒   out ━", Point::new(plot.x + plot.w, plot.y), alignment::Horizontal::Right, faint());
}

/// Input level → output level (−80…0 dB): below the threshold it falls away by the ratio, at most
/// by the range.
fn gate(f: &mut Frame, plot: Plot, thr: f32, ratio: f32, range: f32) {
    const FLOOR: f32 = -80.0;
    let side = plot.h;
    let sq = Plot { x: plot.x + (plot.w - side) / 2.0, y: plot.y, w: side, h: side };
    let to = |i: f32, o: f32| Point::new(sq.px(i / -FLOOR + 1.0), sq.py((o / -FLOOR + 1.0).clamp(0.0, 1.0)));
    let out = |i: f32| if i >= thr { i } else { (thr + (i - thr) * ratio).max(i - range) };
    for k in 0..=4 {
        let d = FLOOR + k as f32 * 20.0;
        f.stroke(&Path::line(to(d, FLOOR), to(d, 0.0)), grid());
        f.stroke(&Path::line(to(FLOOR, d), to(0.0, d)), grid());
    }
    f.stroke(&Path::line(to(FLOOR, FLOOR), to(0.0, 0.0)), dashed(faint()));
    f.stroke(&Path::line(to(thr, FLOOR), to(thr, 0.0)), dashed(muted()));
    f.stroke(&Path::new(|p| polyline(p, (0..=200).map(|k| { let i = FLOOR * (1.0 - k as f32 / 200.0); to(i, out(i)) }))), curve());
    handle(f, to(thr, thr));
    label(f, "threshold", Point::new(to(thr, 0.0).x + 4.0, sq.y), alignment::Horizontal::Left, muted());
    label(f, "in →", Point::new(sq.x + sq.w, sq.y + sq.h + 2.0), alignment::Horizontal::Right, faint());
}

/// The band it watches (shaded) and the most it takes off there (dashed).
fn de_esser(f: &mut Frame, plot: Plot, fc: f32, range: f32) {
    let fr = Freq::new(plot, 200.0, 20000.0, -24.0, 12.0);
    fr.grid(f);
    let x = fr.x(fc);
    f.fill(&Path::rectangle(Point::new(x, fr.p.y), Size::new(fr.p.x + fr.p.w - x, fr.p.h)), ink(0.035));
    f.stroke(&Path::line(Point::new(x, fr.p.y), Point::new(x, fr.p.y + fr.p.h)), dashed(muted()));
    f.stroke(&fr.path(200, |hz| db(high_shelf(hz, fc, 0.8, -range))), dashed(muted()).with_width(1.3));
    f.stroke(&Path::line(Point::new(fr.p.x, fr.y(0.0)), Point::new(fr.p.x + fr.p.w, fr.y(0.0))), curve());
    handle(f, fr.at(fc, 0.0));
    label(f, &format!("up to −{range:.0} dB"), Point::new(fr.p.x + fr.p.w, fr.y(-range) + 4.0), alignment::Horizontal::Right, muted());
}

/// The target level (dashed) and the window it rides the level in (±range).
fn rider(f: &mut Frame, plot: Plot, target: f32, range: f32) {
    const FLOOR: f32 = -48.0;
    let y = |d: f32| plot.py(((d - FLOOR) / -FLOOR).clamp(0.0, 1.0));
    for d in [-48.0f32, -36.0, -24.0, -12.0, 0.0] {
        f.stroke(&Path::line(Point::new(plot.x + 30.0, y(d)), Point::new(plot.x + plot.w, y(d))), if d == 0.0 { zero() } else { grid() });
        label(f, &format!("{d}"), Point::new(plot.x + 24.0, y(d) - 5.0), alignment::Horizontal::Right, faint());
    }
    let (top, bottom) = (y(target + range), y(target - range));
    f.fill(&Path::rectangle(Point::new(plot.x + 30.0, top), Size::new(plot.w - 30.0, bottom - top)), ink(0.06));
    f.stroke(&Path::line(Point::new(plot.x + 30.0, y(target)), Point::new(plot.x + plot.w, y(target))), dashed(theme::c().frame_ink));
    label(f, "target", Point::new(plot.x + plot.w, y(target) - 13.0), alignment::Horizontal::Right, muted());
    label(f, &format!("±{range:.0} dB"), Point::new(plot.x + plot.w, bottom + 3.0), alignment::Horizontal::Right, faint());
}

// ---- distortion ------------------------------------------------------------------------------------

/// A smooth sine (grey) and what's left of it (ink): steps as high as the bits allow, as wide as the
/// sample rate. Two periods of 660 Hz (3 ms).
fn bitcrusher(f: &mut Frame, plot: Plot, bits: f32, rate: f32, mix: f32) {
    let mid = plot.y + plot.h / 2.0;
    let amp = plot.h / 2.0 * 0.9;
    let levels = (2f32.powf(bits.round().clamp(1.0, 24.0)) - 1.0).max(1.0);
    let span = 0.003;
    let sine = |u: f32| (u * 2.0 * TAU).sin() * 0.9;
    if levels <= 31.0 {
        for k in 0..=levels as usize {
            let yv = -1.0 + 2.0 * k as f32 / levels;
            f.stroke(&Path::line(Point::new(plot.x, mid - yv * amp), Point::new(plot.x + plot.w, mid - yv * amp)), grid());
        }
    }
    f.stroke(&Path::line(Point::new(plot.x, mid), Point::new(plot.x + plot.w, mid)), zero());
    f.stroke(&Path::new(|p| polyline(p, (0..=200).map(|i| { let u = i as f32 / 200.0; Point::new(plot.px(u), mid - sine(u) * amp) }))), Stroke::default().with_color(muted()).with_width(1.3));
    let steps = (rate * span).clamp(2.0, 400.0) as usize;
    let quant = |y: f32| ((y + 1.0) / 2.0 * levels).round() / levels * 2.0 - 1.0;
    let path = Path::new(|p| {
        for k in 0..steps {
            let (u0, u1) = (k as f32 / steps as f32, (k + 1) as f32 / steps as f32);
            let x = sine(u0);
            let yv = x + mix * (quant(x) - x);
            let pt = Point::new(plot.px(u0), mid - yv * amp);
            if k == 0 { p.move_to(pt) } else { p.line_to(pt) }
            p.line_to(Point::new(plot.px(u1), mid - yv * amp));
        }
    });
    f.stroke(&path, curve().with_line_join(nice_plug_iced::iced::widget::canvas::LineJoin::Miter).with_width(1.8));
}

/// The input (grey), the carrier (dashed) and their product (ink), over 20 ms.
fn ring_mod(f: &mut Frame, plot: Plot, carrier: f32, mix: f32) {
    let mid = plot.y + plot.h / 2.0;
    let amp = plot.h / 2.0 * 0.9;
    let span = 0.02;
    // An input that isn't a common carrier, so the product reads as one.
    let input = |u: f32| (u * span * 65.0 * TAU).sin() * 0.8;
    let car = |u: f32| (u * span * carrier * TAU).sin();
    let n = 900;
    let line = |g: &dyn Fn(f32) -> f32| Path::new(|p| polyline(p, (0..=n).map(|i| { let u = i as f32 / n as f32; Point::new(plot.px(u), mid - g(u) * amp) })));
    f.stroke(&Path::line(Point::new(plot.x, mid), Point::new(plot.x + plot.w, mid)), zero());
    f.stroke(&line(&|u| car(u) * 0.35), dashed(faint()));
    f.stroke(&line(&input), Stroke::default().with_color(muted()).with_width(1.3));
    f.stroke(&line(&|u| input(u) * ((1.0 - mix) + mix * car(u))), curve().with_width(1.6));
}

/// The tape's colour (head bump, top-end loss; drive deepens both) and wow & flutter as pitch
/// drifting over two seconds.
fn tape(f: &mut Frame, plot: Plot, drive: f32, wow: f32, flutter: f32) {
    let top = Plot { h: plot.h - 54.0, ..plot };
    let fr = Freq::new(top, 20.0, 20000.0, -12.0, 6.0);
    fr.grid(f);
    let mag = |hz: f32| db(peak(hz, 70.0, 1.1, 1.5 + drive * 0.15) * lp2(hz, 18000.0 - drive * 300.0, 0.6));
    f.stroke(&fr.path(200, mag), curve());
    let (y0, y1) = (plot.y + plot.h - 38.0, plot.y + plot.h);
    let m = (y0 + y1) / 2.0;
    f.stroke(&Path::line(Point::new(plot.x, y0), Point::new(plot.x + plot.w, y0)), zero());
    f.stroke(&Path::line(Point::new(plot.x, m), Point::new(plot.x + plot.w, m)), dashed(faint()));
    label(f, "pitch", Point::new(plot.x, y0 + 3.0), alignment::Horizontal::Left, faint());
    if wow + flutter < 0.005 {
        return;
    }
    let dev = |u: f32| {
        let s = u * 2.0;
        wow * (s * TAU * 0.6).sin() * 0.8 + flutter * ((s * TAU * 9.0).sin() * 0.25 + (s * TAU * 16.0 + 1.0).sin() * 0.12)
    };
    f.stroke(&Path::new(|p| polyline(p, (0..=300).map(|i| { let u = i as f32 / 300.0; Point::new(plot.px(u), m - dev(u).clamp(-1.0, 1.0) * (y1 - y0) / 2.0) }))), curve().with_width(1.4));
}

// ---- space -----------------------------------------------------------------------------------------

/// The decay per band in dB over time: damping makes the highs die before the mids, the lows ring a
/// little longer. The gap at the start is the predelay; −60 dB marks RT60.
fn reverb(f: &mut Frame, plot: Plot, decay_s: f32, damping: f32, predelay_ms: f32) {
    let p = Plot { h: plot.h - 12.0, ..plot };
    let span = (decay_s * 1.6).clamp(1.0, 20.0);
    let x = |s: f32| p.px((s / span).clamp(0.0, 1.0));
    let y = |d: f32| p.py(((d + 66.0) / 66.0).clamp(0.0, 1.0));
    let mut d = 0.0;
    while d >= -60.0 {
        f.stroke(&Path::line(Point::new(p.x, y(d)), Point::new(p.x + p.w, y(d))), if d == 0.0 { zero() } else { grid() });
        d -= 12.0;
    }
    let step = nice_step(span / 4.0);
    let mut t = step;
    while t < span {
        label(f, &format!("{} s", trim(t)), Point::new(x(t), p.y + p.h + 4.0), alignment::Horizontal::Center, faint());
        t += step;
    }
    f.stroke(&Path::line(Point::new(p.x, y(-60.0)), Point::new(p.x + p.w, y(-60.0))), dashed(faint()));
    label(f, "−60", Point::new(p.x + p.w, y(-60.0) - 12.0), alignment::Horizontal::Right, faint());
    let pd = predelay_ms / 1000.0;
    let band = |rt: f32| Path::new(|b| {
        b.move_to(Point::new(x(0.0), y(-66.0)));
        b.line_to(Point::new(x(pd), y(-66.0)));
        b.line_to(Point::new(x(pd), y(-3.0)));
        b.line_to(Point::new(x(pd + rt * 57.0 / 60.0), y(-60.0)));
    });
    let (high, low) = (decay_s * (1.0 - 0.7 * damping), decay_s * 1.3);
    f.stroke(&band(low), dashed(faint()).with_width(1.3));
    f.stroke(&band(high), Stroke::default().with_color(muted()).with_width(1.5));
    f.stroke(&band(decay_s), curve());
    for k in 0..6 {
        let xx = x(pd + 0.006 + k as f32 * 0.01 * decay_s.min(3.0));
        f.stroke(&Path::line(Point::new(xx, y(-4.0 - k as f32 * 3.0)), Point::new(xx, y(-14.0 - k as f32 * 3.0))), Stroke::default().with_color(theme::c().frame_ink).with_width(1.0));
    }
    label(f, "high", Point::new(x(pd + high), y(-60.0) + 3.0), alignment::Horizontal::Center, muted());
    label(f, "mid", Point::new(x(pd + decay_s), y(-60.0) + 3.0), alignment::Horizontal::Center, theme::c().frame_ink);
}

/// The decay (ink) and the pitched layers it feeds (grey), blooming later and fading.
fn shimmer(f: &mut Frame, plot: Plot, decay_s: f32, pitch: f32, amount: f32) {
    let p = Plot { h: plot.h - 12.0, ..plot };
    let span = (decay_s * 1.5).clamp(1.0, 20.0);
    let at = |s: f32, a: f32| Point::new(p.px(s / span), p.py(a.clamp(0.0, 1.0)));
    let tau = decay_s / 6.9;
    let env = |delay: f32, rise: f32, g: f32| move |s: f32| { let x = s - delay; if x < 0.0 { 0.0 } else { g * (1.0 - (-x / rise).exp()) * (-x / tau).exp() } };
    let line = |e: &dyn Fn(f32) -> f32| Path::new(|b| polyline(b, (0..=200).map(|i| { let s = span * i as f32 / 200.0; at(s, e(s)) })));
    f.stroke(&Path::line(Point::new(p.x, p.y + p.h), Point::new(p.x + p.w, p.y + p.h)), zero());
    let main = env(0.0, 0.02, 0.95);
    let first = env(span * 0.04, span * 0.12, 0.75 * amount);
    let second = env(span * 0.12, span * 0.18, 0.45 * amount * amount);
    let area = Path::new(|b| {
        b.move_to(Point::new(p.x, p.y + p.h));
        (0..=200).for_each(|i| { let s = span * i as f32 / 200.0; b.line_to(at(s, main(s))) });
        b.line_to(Point::new(p.x + p.w, p.y + p.h));
        b.close();
    });
    f.fill(&area, ink(0.06));
    f.stroke(&line(&main), curve());
    f.stroke(&line(&first), Stroke::default().with_color(muted()).with_width(1.5));
    f.stroke(&line(&second), dashed(muted()).with_width(1.2));
    let peak_of = |e: &dyn Fn(f32) -> f32| (0..=200).map(|i| span * i as f32 / 200.0).fold((0.0, 0.0), |(bs, bv), s| if e(s) > bv { (s, e(s)) } else { (bs, bv) });
    let (s1, v1) = peak_of(&first);
    let (s2, v2) = peak_of(&second);
    if amount > 0.05 {
        label(f, &format!("{:+}", pitch.round()), Point::new(at(s1, v1).x, at(s1, v1).y - 13.0), alignment::Horizontal::Center, muted());
        label(f, &format!("{:+}", (2.0 * pitch).round()), Point::new(at(s2, v2).x, at(s2, v2).y - 13.0), alignment::Horizontal::Center, faint());
    }
    let step = nice_step(span / 4.0);
    let mut t = step;
    while t < span {
        label(f, &format!("{} s", trim(t)), Point::new(p.px(t / span), p.y + p.h + 4.0), alignment::Horizontal::Center, faint());
        t += step;
    }
}

// ---- pitch -----------------------------------------------------------------------------------------

/// The shift on a semitone ruler (octaves marked), and a stretch of a sine before (grey) and
/// after (ink).
fn pitch_shift(f: &mut Frame, plot: Plot, st: f32) {
    let ry = plot.y + 14.0;
    let x = |s: f32| plot.px((s + 24.0) / 48.0);
    f.stroke(&Path::line(Point::new(plot.x, ry), Point::new(plot.x + plot.w, ry)), zero());
    for s in -24..=24 {
        let octave = s % 12 == 0;
        let h = if octave { 6.0 } else { 3.0 };
        f.stroke(&Path::line(Point::new(x(s as f32), ry - h), Point::new(x(s as f32), ry + h)), Stroke::default().with_color(if octave { muted() } else { ink(0.16) }).with_width(1.0));
        if octave {
            label(f, &format!("{s:+}").replace("+0", "0"), Point::new(x(s as f32), ry + 9.0), alignment::Horizontal::Center, faint());
        }
    }
    f.stroke(&Path::line(Point::new(x(0.0), ry), Point::new(x(st), ry)), curve().with_width(3.0));
    f.fill(&Path::circle(Point::new(x(st), ry), 4.5), theme::c().frame_ink);
    let (y0, y1) = (ry + 26.0, plot.y + plot.h);
    let m = (y0 + y1) / 2.0;
    let amp = (y1 - y0) / 2.0 * 0.85;
    let r = 2f32.powf(st / 12.0);
    let sine = |k: f32| Path::new(|b| polyline(b, (0..=300).map(|i| { let u = i as f32 / 300.0; Point::new(plot.px(u), m - (u * PI * 8.0 * k).sin() * amp) })));
    f.stroke(&Path::line(Point::new(plot.x, m), Point::new(plot.x + plot.w, m)), zero());
    f.stroke(&sine(1.0), Stroke::default().with_color(muted()).with_width(1.3));
    f.stroke(&sine(r), curve());
}

// ---- stereo & utility ------------------------------------------------------------------------------

/// Where the sound sits on a half circle, and the gain each side gets (the other side is
/// turned down).
fn pan_view(f: &mut Frame, plot: Plot, pan: f32) {
    let r = (plot.h - 12.0).min(plot.w / 2.0 - 70.0).max(20.0);
    let c = Point::new(plot.x + plot.w / 2.0, plot.y + plot.h - 4.0);
    let arc = Path::new(|b| {
        b.move_to(Point::new(c.x - r, c.y));
        (0..=60).for_each(|i| { let a = PI * (1.0 - i as f32 / 60.0); b.line_to(Point::new(c.x + r * a.cos(), c.y - r * a.sin())) });
    });
    f.stroke(&arc, zero());
    for k in -10..=10 {
        let a = k as f32 / 10.0 * PI / 2.0;
        let major = k % 5 == 0;
        let inner = r - if major { 8.0 } else { 4.0 };
        f.stroke(&Path::line(Point::new(c.x + inner * a.sin(), c.y - inner * a.cos()), Point::new(c.x + r * a.sin(), c.y - r * a.cos())), Stroke::default().with_color(if major { muted() } else { ink(0.16) }).with_width(1.0));
    }
    let a = pan.clamp(-1.0, 1.0) * PI / 2.0;
    f.stroke(&Path::line(c, Point::new(c.x + (r - 14.0) * a.sin(), c.y - (r - 14.0) * a.cos())), curve().with_width(2.5));
    f.fill(&Path::circle(c, 4.0), theme::c().frame_ink);
    label(f, "L", Point::new(c.x - r - 10.0, c.y - 6.0), alignment::Horizontal::Right, muted());
    label(f, "R", Point::new(c.x + r + 10.0, c.y - 6.0), alignment::Horizontal::Left, muted());
    let (gl, gr) = ((1.0 - pan.max(0.0)).max(0.0), (1.0 + pan.min(0.0)).max(0.0));
    let bw = 56.0;
    let gain = |f: &mut Frame, x: f32, g: f32, name: &str, right: bool| {
        let y = c.y - 26.0;
        f.fill(&Path::rounded_rectangle(Point::new(x, y), Size::new(bw, 5.0), 2.5.into()), ink(0.08));
        let filled = bw * g;
        f.fill(&Path::rounded_rectangle(Point::new(if right { x + bw - filled } else { x }, y), Size::new(filled, 5.0), 2.5.into()), theme::c().frame_ink);
        let text = if g <= 0.001 { format!("{name} −∞") } else { format!("{name} {:.1} dB", db(g)) };
        label(f, &text, Point::new(if right { x + bw } else { x }, y - 13.0), if right { alignment::Horizontal::Right } else { alignment::Horizontal::Left }, muted());
    };
    gain(f, plot.x, gl, "L", false);
    gain(f, plot.x + plot.w - bw, gr, "R", true);
}

/// The same hit on both channels; the delayed one starts later by the set time (0–40 ms).
fn haas(f: &mut Frame, plot: Plot, delay_ms: f32, side: Side) {
    let p = Plot { h: plot.h - 12.0, ..plot };
    let x = |ms: f32| p.px(ms / 40.0);
    for ms in [0.0f32, 10.0, 20.0, 30.0, 40.0] {
        f.stroke(&Path::line(Point::new(x(ms), p.y), Point::new(x(ms), p.y + p.h)), grid());
        label(f, &format!("{ms} ms"), Point::new(x(ms).clamp(p.x + 14.0, p.x + p.w - 14.0), p.y + p.h + 4.0), alignment::Horizontal::Center, faint());
    }
    let lane = p.h / 2.0;
    let mids = [p.y + lane / 2.0, p.y + lane * 1.5];
    let delayed = if side == Side::Left { 0 } else { 1 };
    for (i, m) in mids.iter().enumerate() {
        let off = if i == delayed { delay_ms } else { 0.0 };
        let colour = if i == delayed { theme::c().frame_ink } else { muted() };
        f.stroke(&Path::line(Point::new(p.x, *m), Point::new(p.x + p.w, *m)), zero());
        let hit = Path::new(|b| polyline(b, (0..=200).map(|k| {
            let u = k as f32 / 200.0;
            let ms = off + u * 24.0;
            Point::new(x(ms.min(40.0)), m - (u * 24.0 * TAU / 2.2).sin() * (-u * 5.0).exp() * lane * 0.4)
        })));
        f.stroke(&hit, Stroke::default().with_color(colour).with_width(if i == delayed { 1.8 } else { 1.4 }));
        label(f, if i == 0 { "L" } else { "R" }, Point::new(p.x + 2.0, m - lane / 2.0 + 2.0), alignment::Horizontal::Left, colour);
    }
    let my = (mids[0] + mids[1]) / 2.0;
    f.stroke(&Path::line(Point::new(x(0.0), my), Point::new(x(delay_ms), my)), curve().with_width(1.2));
    label(f, &format!("{delay_ms:.1} ms"), Point::new(x(delay_ms / 2.0).max(p.x + 22.0), my - 13.0), alignment::Horizontal::Center, theme::c().frame_ink);
}

/// What happens to the side signal (ink, a 4th-order high pass) while the mid stays (dashed);
/// the mono zone hatched.
fn mono_bass(f: &mut Frame, plot: Plot, fc: f32) {
    let fr = Freq::new(plot, 20.0, 20000.0, -36.0, 6.0);
    fr.grid(f);
    let x1 = fr.x(fc);
    let mut x = fr.p.x - fr.p.h;
    while x < x1 {
        let (a, b) = (Point::new(x.max(fr.p.x), fr.p.y + fr.p.h - (x.max(fr.p.x) - x)), Point::new((x + fr.p.h).min(x1), fr.p.y + fr.p.h - ((x + fr.p.h).min(x1) - x)));
        f.stroke(&Path::line(a, b), Stroke::default().with_color(ink(0.07)).with_width(1.0));
        x += 7.0;
    }
    f.stroke(&Path::line(Point::new(fr.p.x, fr.y(0.0)), Point::new(fr.p.x + fr.p.w, fr.y(0.0))), dashed(muted()));
    let side = |hz: f32| db(hp2(hz, fc, 0.707).powi(2));
    f.stroke(&fr.path(240, side), curve());
    handle(f, fr.at(fc, side(fc)));
    label(f, "mono", Point::new(x1 - 6.0, fr.y(-30.0)), alignment::Horizontal::Right, muted());
    label(f, "mid", Point::new(fr.p.x + fr.p.w, fr.y(0.0) - 13.0), alignment::Horizontal::Right, muted());
    label(f, "side", Point::new(fr.p.x + fr.p.w, fr.y(0.0) + 3.0), alignment::Horizontal::Right, theme::c().frame_ink);
}

/// One stretch per channel; the inverted ones in ink, flipped.
fn invert(f: &mut Frame, plot: Plot, channels: Channels) {
    let lane = plot.h / 2.0;
    for (i, name) in ["L", "R"].iter().enumerate() {
        let flipped = matches!((channels, i), (Channels::Both, _) | (Channels::Left, 0) | (Channels::Right, 1));
        let m = plot.y + lane * (i as f32 + 0.5);
        let colour = if flipped { theme::c().frame_ink } else { muted() };
        let sign = if flipped { -1.0 } else { 1.0 };
        let x0 = plot.x + 36.0;
        f.stroke(&Path::line(Point::new(x0, m), Point::new(plot.x + plot.w, m)), zero());
        f.stroke(&Path::new(|b| polyline(b, (0..=200).map(|k| { let u = k as f32 / 200.0; Point::new(x0 + u * (plot.x + plot.w - x0), m - sign * (u * 3.0 * TAU).sin() * lane * 0.38) }))), Stroke::default().with_color(colour).with_width(if flipped { 2.0 } else { 1.4 }));
        label(f, &if flipped { format!("{name} Ø") } else { name.to_string() }, Point::new(plot.x, m - 5.0), alignment::Horizontal::Left, colour);
    }
}

// ---- modulation (LFO at phase 0 until the live phase comes) ------------------------------------------

fn lfo(shape: LfoShape, phase: f32) -> f32 {
    let p = phase.rem_euclid(1.0);
    match shape {
        LfoShape::Sine => (p * TAU).sin(),
        LfoShape::Triangle => if p < 0.25 { p * 4.0 } else if p < 0.75 { 2.0 - p * 4.0 } else { p * 4.0 - 4.0 },
        LfoShape::Square => if p < 0.5 { 1.0 } else { -1.0 },
    }
}

/// The delay time of each voice over two cycles (one ink, one grey, opposite), the depth as the
/// dashed band.
fn chorus(f: &mut Frame, plot: Plot, depth: f32) {
    let m = plot.y + plot.h / 2.0;
    let d = plot.h / 2.0 * 0.85 * depth.max(0.02);
    let full = plot.h / 2.0 * 0.85;
    f.stroke(&Path::line(Point::new(plot.x, m - full), Point::new(plot.x + plot.w, m - full)), dashed(faint()));
    f.stroke(&Path::line(Point::new(plot.x, m + full), Point::new(plot.x + plot.w, m + full)), dashed(faint()));
    f.stroke(&Path::line(Point::new(plot.x, m), Point::new(plot.x + plot.w, m)), zero());
    let voice = |off: f32| Path::new(|b| polyline(b, (0..=200).map(|k| { let u = k as f32 / 200.0; Point::new(plot.px(u), m - (u * 2.2 * TAU + off).sin() * d) })));
    f.stroke(&voice(PI), Stroke::default().with_color(muted()).with_width(1.5));
    f.stroke(&voice(0.0), curve());
    label(f, "delay", Point::new(plot.x, m - full - 13.0), alignment::Horizontal::Left, faint());
}

/// The comb at the middle of the sweep, the sweep's reach as the shaded band.
fn flanger(f: &mut Frame, plot: Plot, depth: f32, g: f32) {
    let fr = Freq::new(plot, 20.0, 20000.0, -30.0, 12.0);
    fr.grid(f);
    let (dmin, dmax) = (0.0002, 0.0002 + 0.004 * depth.max(0.01));
    let dmid = (dmin + dmax) / 2.0;
    let mag = |hz: f32| {
        let th = TAU * hz * dmid;
        let (re, im) = (1.0 - g * th.cos(), g * th.sin());
        // dry + delayed with feedback
        let n = re * re + im * im;
        let (wr, wi) = ((th.cos() * re - th.sin() * im) / n, (-th.sin() * re - th.cos() * im) / n);
        db((1.0 + wr).hypot(wi) / 2.0 * (1.0 - g.abs() * 0.5))
    };
    // the first notch moves between these
    let (n0, n1) = (0.5 / dmax, 0.5 / dmin);
    f.fill(&Path::rectangle(Point::new(fr.x(n0), fr.p.y), Size::new(fr.x(n1.min(20000.0)) - fr.x(n0), fr.p.h)), ink(0.035));
    f.stroke(&fr.path(2400, mag), curve().with_width(1.3));
    label(f, "sweep", Point::new(fr.x(n0) + 4.0, fr.p.y), alignment::Horizontal::Left, faint());
}

/// The notches (stages ÷ 2) around the centre frequency.
fn phaser(f: &mut Frame, plot: Plot, fc: f32, stages: u32, mix: f32) {
    let fr = Freq::new(plot, 20.0, 20000.0, -36.0, 12.0);
    fr.grid(f);
    let n = stages.max(2) as f32;
    let mag = |hz: f32| {
        let ph = -2.0 * n * (hz / fc).atan();
        db(((1.0 - mix) + mix * ph.cos()).hypot(mix * ph.sin()))
    };
    fr.fill(f, 400, mag);
    f.stroke(&fr.path(400, mag), curve());
    handle(f, fr.at(fc, mag(fc)));
}

/// The gain over 2.5 cycles (the line on top) and a steady signal it shapes (bars).
fn tremolo(f: &mut Frame, plot: Plot, depth: f32, shape: LfoShape, offset_deg: f32) {
    let gain = |u: f32| 1.0 - depth * (0.5 + 0.5 * lfo(shape, u * 2.5 + offset_deg / 360.0));
    let top = plot.y + 14.0;
    let (y0, y1) = (top + 14.0, plot.y + plot.h);
    let m = (y0 + y1) / 2.0;
    let amp = (y1 - y0) / 2.0 * 0.92;
    let n = (plot.w / 3.0) as usize;
    for k in 0..n {
        let u = k as f32 / n as f32;
        let x = plot.px(u);
        let a = amp * (0.82 + 0.18 * ((k * 37 % 11) as f32 / 11.0));
        f.fill(&Path::rectangle(Point::new(x, m - a), Size::new(1.7, 2.0 * a)), ink(0.12));
        let g = a * gain(u);
        f.fill(&Path::rectangle(Point::new(x, m - g), Size::new(1.7, 2.0 * g)), ink(0.8));
    }
    f.stroke(&Path::new(|b| polyline(b, (0..=200).map(|k| { let u = k as f32 / 200.0; Point::new(plot.px(u), top + (1.0 - gain(u)) * 12.0) }))), curve().with_width(1.5));
}

/// The left (ink) and right (grey) gain over 2.5 cycles, and where the sound sits now on L–R.
fn auto_pan(f: &mut Frame, plot: Plot, depth: f32, shape: LfoShape, offset_deg: f32) {
    let p = Plot { h: plot.h - 30.0, ..plot };
    let pos = |u: f32| lfo(shape, u * 2.5 + offset_deg / 360.0) * depth;
    f.stroke(&Path::line(Point::new(p.x, p.y), Point::new(p.x + p.w, p.y)), zero());
    f.stroke(&Path::line(Point::new(p.x, p.y + p.h), Point::new(p.x + p.w, p.y + p.h)), zero());
    let line = |sign: f32| Path::new(|b| polyline(b, (0..=200).map(|k| { let u = k as f32 / 200.0; Point::new(p.px(u), p.py(0.5 + sign * 0.5 * -pos(u))) })));
    f.stroke(&line(-1.0), Stroke::default().with_color(muted()).with_width(1.5));
    f.stroke(&line(1.0), curve());
    label(f, "R", Point::new(p.x + p.w, p.y - 13.0), alignment::Horizontal::Right, muted());
    label(f, "L", Point::new(p.x + p.w - 14.0, p.y - 13.0), alignment::Horizontal::Right, theme::c().frame_ink);
    let by = plot.y + plot.h - 8.0;
    let (cx, half) = (plot.x + plot.w / 2.0, 90f32.min(plot.w / 2.0 - 30.0));
    f.stroke(&Path::line(Point::new(cx - half, by), Point::new(cx + half, by)), zero());
    f.stroke(&Path::line(Point::new(cx, by - 4.0), Point::new(cx, by + 4.0)), zero());
    label(f, "L", Point::new(cx - half - 8.0, by - 5.0), alignment::Horizontal::Right, faint());
    label(f, "R", Point::new(cx + half + 8.0, by - 5.0), alignment::Horizontal::Left, faint());
    f.fill(&Path::circle(Point::new(cx + half * pos(0.0), by), 4.0), theme::c().frame_ink);
}

// ---- containers ------------------------------------------------------------------------------------

/// The bands' responses meeting at the crossovers (Linkwitz-Riley, 4th order), a handle at each.
fn band_split(f: &mut Frame, plot: Plot, crossovers: &[f32]) {
    let fr = Freq::new(plot, 20.0, 20000.0, -36.0, 6.0);
    fr.grid(f);
    let lp = |hz: f32, c: f32| 1.0 / (1.0 + (hz / c).powi(4));
    let hp = |hz: f32, c: f32| (hz / c).powi(4) / (1.0 + (hz / c).powi(4));
    let n = crossovers.len() + 1;
    let names: Vec<String> = match n {
        2 => vec!["low".into(), "high".into()],
        3 => vec!["low".into(), "mid".into(), "high".into()],
        _ => (1..=n).map(|i| i.to_string()).collect(),
    };
    for b in 0..n {
        let band = |hz: f32| {
            let mut g = 1.0;
            if b > 0 { g *= hp(hz, crossovers[b - 1]); }
            if b < n - 1 { g *= lp(hz, crossovers[b]); }
            db(g)
        };
        let colour = if b % 2 == 1 { theme::c().frame_ink } else { muted() };
        f.stroke(&fr.path(240, band), Stroke::default().with_color(colour).with_width(if b % 2 == 1 { 2.0 } else { 1.6 }));
        let lo = if b == 0 { 20.0 } else { crossovers[b - 1] };
        let hi = if b == n - 1 { 20000.0 } else { crossovers[b] };
        label(f, &names[b], Point::new(fr.x((lo * hi).sqrt()), fr.p.y + 2.0), alignment::Horizontal::Center, muted());
    }
    for c in crossovers {
        handle(f, fr.at(*c, -6.0));
    }
}

/// A container's routing: in, the split, a lane per sub-chain with its blocks as chips, the merge,
/// out; with `mix` (two lanes), the crossfade under it.
fn routing(f: &mut Frame, plot: Plot, lanes: &[(String, &Chain)], split: &str, merge: &str, mix: Option<f32>) {
    let p = Plot { h: if mix.is_some() { plot.h - 30.0 } else { plot.h }, ..plot };
    let cy = p.y + p.h / 2.0;
    let (xi, xo) = (p.x + 6.0, p.x + p.w - 6.0);
    let (xs, xm) = (p.x + 40.0, p.x + p.w - 40.0);
    let wire = Stroke::default().with_color(ink(0.22)).with_width(1.2);
    for x in [xi, xo] {
        f.stroke(&Path::circle(Point::new(x, cy), 4.0), Stroke::default().with_color(muted()).with_width(1.5));
    }
    f.stroke(&Path::line(Point::new(xi + 4.0, cy), Point::new(xs - 15.0, cy)), wire);
    f.stroke(&Path::line(Point::new(xm + 15.0, cy), Point::new(xo - 4.0, cy)), wire);
    let n = lanes.len().max(1) as f32;
    for (i, (name, chain)) in lanes.iter().enumerate() {
        let y = p.y + (i as f32 + 0.5) * p.h / n;
        let (x0, x1) = (xs + 15.0, xm - 15.0);
        let path = Path::new(|b| {
            b.move_to(Point::new(x0, cy));
            b.bezier_curve_to(Point::new(x0 + 14.0, cy), Point::new(x0 + 6.0, y), Point::new(x0 + 20.0, y));
            b.line_to(Point::new(x1 - 20.0, y));
            b.bezier_curve_to(Point::new(x1 - 6.0, y), Point::new(x1 - 14.0, cy), Point::new(x1, cy));
        });
        f.stroke(&path, wire);
        let label_x = x0 + 24.0;
        f.fill(&Path::rectangle(Point::new(label_x - 2.0, y - 7.0), Size::new(32.0, 14.0)), Color { a: 1.0, ..theme::c().frame_node });
        label(f, name, Point::new(label_x, y - 5.0), alignment::Horizontal::Left, muted());
        let mut cx = label_x + 36.0;
        let blocks: &[BlockSpec] = &chain.0;
        let room = ((x1 - 24.0 - cx) / 70.0).floor().max(0.0) as usize;
        for (k, b) in blocks.iter().enumerate() {
            if k >= room {
                label(f, &format!("+{}", blocks.len() - k), Point::new(cx, y - 5.0), alignment::Horizontal::Left, muted());
                break;
            }
            chip(f, Rectangle { x: cx, y: y - 11.0, width: 62.0, height: 22.0 }, &b.id);
            cx += 70.0;
        }
    }
    for (x, t) in [(xs, split), (xm, merge)] {
        let w = (t.chars().count() as f32 * 6.5 + 12.0).max(30.0);
        chip(f, Rectangle { x: x - w / 2.0, y: cy - 11.0, width: w, height: 22.0 }, t);
    }
    if let Some(m) = mix {
        let y = plot.y + plot.h - 10.0;
        let (x0, x1) = (plot.x + plot.w / 2.0 - 100.0, plot.x + plot.w / 2.0 + 100.0);
        f.fill(&Path::rounded_rectangle(Point::new(x0, y - 3.0), Size::new(x1 - x0, 6.0), 3.0.into()), ink(0.08));
        f.fill(&Path::circle(Point::new(lerp(x0, x1, m.clamp(0.0, 1.0)), y), 5.0), theme::c().frame_ink);
        let (a, b) = (lanes.first().map_or("", |l| l.0.as_str()), lanes.get(1).map_or("", |l| l.0.as_str()));
        label(f, a, Point::new(x0 - 8.0, y - 5.0), alignment::Horizontal::Right, muted());
        label(f, b, Point::new(x1 + 8.0, y - 5.0), alignment::Horizontal::Left, muted());
    }
}

/// A block chip in a routing view (its id, cut short to fit).
fn chip(f: &mut Frame, r: Rectangle, s: &str) {
    f.fill(&Path::rounded_rectangle(r.position(), r.size(), 6.0.into()), theme::c().frame_raised);
    f.stroke(&Path::rounded_rectangle(r.position(), r.size(), 6.0.into()), Stroke::default().with_color(theme::c().frame_hairline_strong).with_width(1.0));
    let max = ((r.width - 8.0) / 6.0) as usize;
    let text = if s.chars().count() > max { format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>()) } else { s.to_string() };
    f.fill_text(Text {
        content: text,
        position: Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0),
        color: theme::c().frame_ink_2,
        size: Pixels(10.5),
        font: theme::font::SANS,
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });
}

// ---- small helpers -----------------------------------------------------------------------------------

fn ink(a: f32) -> Color {
    theme::alpha(theme::c().frame_ink, a)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A position `t` (0..1) on a short track with labels spread along it.
fn track(f: &mut Frame, at: Point, width: f32, labels: &[&str], t: f32) {
    f.stroke(&Path::line(at, Point::new(at.x + width, at.y)), Stroke::default().with_color(ink(0.2)).with_width(1.0));
    let n = labels.len().saturating_sub(1).max(1) as f32;
    let nearest = (t.clamp(0.0, 1.0) * n).round() as usize;
    for (i, l) in labels.iter().enumerate() {
        label(f, l, Point::new(at.x + width * i as f32 / n, at.y + 6.0), alignment::Horizontal::Center, if i == nearest { muted() } else { faint() });
    }
    f.fill(&Path::circle(Point::new(at.x + width * t.clamp(0.0, 1.0), at.y), 3.2), theme::c().frame_ink);
}

/// 1, 2, 5 × 10^n just above `x`.
fn nice_step(x: f32) -> f32 {
    let e = 10f32.powf(x.max(1e-6).log10().floor());
    [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * e).find(|s| *s >= x).unwrap_or(10.0 * e)
}

fn hz_short(hz: f32) -> String {
    if hz >= 1000.0 { format!("{}k", trim(hz / 1000.0)) } else { trim(hz) }
}

fn trim(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn note_name(hz: f32) -> String {
    const NAMES: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
    let n = (69.0 + 12.0 * (hz / 440.0).log2()).round() as i32;
    format!("{}{}", NAMES[n.rem_euclid(12) as usize], n.div_euclid(12) - 1)
}

fn vowel_name(v: f32) -> String {
    const V: [&str; 5] = ["a", "e", "i", "o", "u"];
    let v = v.clamp(0.0, 4.0);
    let (i, t) = (v.floor() as usize, v - v.floor());
    if t < 0.05 || i >= 4 { V[i.min(4)].to_string() } else if t > 0.95 { V[i + 1].to_string() } else { format!("{} → {}", V[i], V[i + 1]) }
}

fn shape_name(s: LfoShape) -> &'static str {
    match s {
        LfoShape::Sine => "sine",
        LfoShape::Triangle => "triangle",
        LfoShape::Square => "square",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_steps() {
        assert_eq!(note_name(440.0), "A4");
        assert_eq!(note_name(261.63), "C4");
        assert_eq!(vowel_name(0.0), "a");
        assert_eq!(vowel_name(1.5), "e → i");
        assert_eq!(nice_step(380.0), 500.0);
        assert_eq!(nice_step(0.4), 0.5);
        assert_eq!(hz_short(2000.0), "2k");
    }
}
