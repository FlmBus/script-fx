//! The displays drawn from the knobs (`docs/research/block-displays.md` §1, step 2; designs:
//! `docs/research/mockups/block-displays.html`).
//!
//! **Fixed axes:** every display has labelled axes that never move; turning a knob changes only the
//! content. The filters' curves are the DSP's transfer functions (`dsp::preview`). The rest are
//! illustrations of what the settings do (reverb bands, tape colour, the shape of a hit). The
//! modulation blocks show their LFO from phase 0 (the live phase comes with the taps, step 3).

use std::f32::consts::{PI, TAU};

use dsp::preview;
use patch::chain::{BlockKind, BlockSpec, Chain, Channels, DynamicMode, LfoShape, ParamRef, RectifyMode, Side};

use super::super::theme;
use super::{Plot, curve, dashed, faint, grid, handle, label, muted, polyline, shaper, zero};
use nice_plug_iced::iced::{
    Color, Pixels, Point, Rectangle, Size,
    alignment,
    widget::canvas::{Frame, LineJoin, Path, Stroke, Text},
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
            | BlockKind::Granular { .. }
            | BlockKind::Stutter { .. }
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
        BlockKind::Haas { delay, channel } => {
            let d = haas_signed(v(delay), *channel);
            format!("{} later by {:.1} ms", if d < 0.0 { "left" } else { "right" }, d.abs())
        }
        BlockKind::MonoBass { freq, slope } => format!("mono below {} · {} dB/oct", show(freq), v(slope).round()),
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
        BlockKind::BandSplit { crossovers, slope, .. } => format!("{} dB/oct · {}", v(slope).round(), crossovers.iter().map(show).collect::<Vec<_>>().join(" · ")),
        BlockKind::Parallel { branches, mix } => match mix {
            Some(m) => format!("{} branches · mix {}", branches.len(), show(m)),
            None => format!("{} branches, summed", branches.len()),
        },
        BlockKind::Granular { grain, density, spread, .. } => join(&[("grain ", grain), ("", density), ("spread ", spread)]),
        BlockKind::Stutter { active, length, sync, .. } => {
            let on = if v(active) > 0.5 { "repeating" } else { "off" };
            match sync {
                Some(beats) => format!("{} beat slice · {on}", trim(*beats)),
                None => format!("{} slice · {on}", show(length)),
            }
        }
        BlockKind::MidSide { .. } => "encode · mid ‖ side · decode".into(),
        BlockKind::LeftRight { .. } => "left ‖ right".into(),
        _ => String::new(),
    }
}

/// Draws the block's display into `plot`.
pub(super) fn draw(f: &mut Frame, plot: Plot, kind: &BlockKind, v: V, tempo: f32) {
    let hz = |rate: &ParamRef, sync: &Option<f32>| match sync {
        Some(beats) => tempo / 60.0 / beats,
        None => v(rate),
    };
    match kind {
        BlockKind::MorphFilter { freq, morph, .. } => {
            let fr = filter_curve(f, plot, kind, v, -48.0, 24.0, &[v(freq)]);
            handle(f, fr.on(kind, v, v(freq)));
            track(f, Point::new(fr.p.x + fr.p.w - 108.0, fr.p.y + 6.0), 100.0, &["LP", "BP", "HP"], v(morph) / 100.0);
        }
        BlockKind::LadderFilter { freq, .. } => {
            let fr = filter_curve(f, plot, kind, v, -48.0, 24.0, &[v(freq)]);
            handle(f, fr.on(kind, v, v(freq)));
        }
        BlockKind::CombFilter { freq, .. } => comb(f, plot, kind, v, v(freq)),
        BlockKind::Formant { vowel, .. } => {
            let fr = filter_curve(f, plot, kind, v, -36.0, 24.0, &[]);
            track(f, Point::new(fr.p.x + fr.p.w - 108.0, fr.p.y + 6.0), 100.0, &["a", "e", "i", "o", "u"], v(vowel) / 4.0);
        }
        BlockKind::Resonator { notes, .. } => {
            let fr = filter_curve(f, plot, kind, v, -48.0, 12.0, notes);
            // Close notes: their names alternate between two heights.
            for (i, n) in notes.iter().enumerate() {
                let at = fr.on(kind, v, *n);
                let lift = if i % 2 == 0 { 13.0 } else { 24.0 };
                label(f, &note_name(*n), Point::new(at.x, (at.y - lift).max(fr.p.y)), alignment::Horizontal::Center, muted());
            }
        }
        BlockKind::DynamicEq { freq, q, range, mode, .. } => dynamic_eq(f, plot, v(freq), v(q), v(range), *mode),
        BlockKind::Transient { attack, sustain, .. } => transient(f, plot, v(attack), v(sustain)),
        BlockKind::Gate { threshold, ratio, range, .. } => gate(f, plot, v(threshold), v(ratio), v(range)),
        BlockKind::DeEsser { freq, range, .. } => de_esser(f, plot, v(freq), v(range)),
        BlockKind::Rider { target, range, .. } => rider(f, plot, v(target), v(range)),
        BlockKind::Bitcrusher { bits, rate, .. } => bitcrusher(f, plot, v(bits), rate.as_ref().map_or(48_000.0, v)),
        BlockKind::RingMod { freq, .. } => ring_mod(f, plot, v(freq)),
        BlockKind::Rectifier { mode, mix } => {
            let (full, mix) = (*mode == RectifyMode::Full, v(mix) / 100.0);
            let shape = move |x: f32| Some(x + mix * (if full { x.abs() } else { x.max(0.0) * 2.0 } - x));
            let _ = shaper(f, plot, &shape, None);
        }
        BlockKind::Tape { drive, wow, flutter, .. } => tape(f, plot, v(drive), v(wow) / 100.0, v(flutter) / 100.0),
        BlockKind::Reverb(r) => reverb(f, plot, v(&r.decay), v(&r.damping) / 100.0, v(&r.predelay)),
        BlockKind::Shimmer(r) => shimmer(f, plot, v(&r.decay), v(&r.predelay), r.pitch.as_ref().map_or(12.0, v), r.amount.as_ref().map_or(50.0, v) / 100.0),
        BlockKind::PitchShift { semitones, .. } => pitch_shift(f, plot, v(semitones)),
        BlockKind::Granular { grain, density, spread, pitch, jitter, reverse, .. } => {
            granular(f, plot, v(grain), v(density), v(spread), v(pitch), v(jitter), v(reverse) / 100.0)
        }
        BlockKind::Stutter { active, length, sync, .. } => {
            let ms = match sync {
                Some(beats) => beats * 60_000.0 / tempo,
                None => v(length),
            };
            stutter(f, plot, ms, v(active) > 0.5)
        }
        BlockKind::Pan { pan } => pan_view(f, plot, v(pan) / 100.0),
        BlockKind::Haas { delay, channel } => haas(f, plot, haas_signed(v(delay), *channel)),
        BlockKind::MonoBass { freq, slope } => mono_bass(f, plot, v(freq), v(slope)),
        BlockKind::Invert { channels } => invert(f, plot, *channels),
        BlockKind::Chorus { rate, sync, depth, .. } => chorus(f, plot, hz(rate, sync), v(depth) / 100.0),
        BlockKind::Flanger { .. } => swept(f, plot, kind, v, None),
        BlockKind::Phaser { freq, .. } => swept(f, plot, kind, v, Some(v(freq))),
        BlockKind::Tremolo { rate, sync, depth, shape, phase_offset } => tremolo(f, plot, hz(rate, sync), v(depth) / 100.0, *shape, v(phase_offset)),
        BlockKind::AutoPan { rate, sync, depth, shape, phase_offset } => auto_pan(f, plot, hz(rate, sync), v(depth) / 100.0, *shape, v(phase_offset)),
        BlockKind::BandSplit { crossovers, slope, .. } => band_split(f, plot, &crossovers.iter().map(v).collect::<Vec<_>>(), v(slope)),
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

/// > 0 = the right channel later, < 0 the left (`channel = "left"` flips it, as in the DSP).
fn haas_signed(delay: f32, channel: Side) -> f32 {
    if channel == Side::Left { -delay } else { delay }
}

// ---- axes --------------------------------------------------------------------------------------

/// The area inside the axis labels.
pub(super) fn inner(plot: Plot) -> Plot {
    Plot { x: plot.x + 28.0, y: plot.y, w: plot.w - 28.0, h: plot.h - 13.0 }
}

/// A vertical grid line at `t` (0..1) with its label under the plot.
pub(super) fn xtick(f: &mut Frame, p: Plot, t: f32, s: &str) {
    let x = p.px(t);
    f.stroke(&Path::line(Point::new(x, p.y), Point::new(x, p.y + p.h)), grid());
    if !s.is_empty() {
        label(f, s, Point::new(x, p.y + p.h + 3.0), alignment::Horizontal::Center, faint());
    }
}

/// A horizontal grid line at `t` (0 = bottom) with its label left of the plot.
pub(super) fn ytick(f: &mut Frame, p: Plot, t: f32, s: &str) {
    let y = p.py(t);
    f.stroke(&Path::line(Point::new(p.x, y), Point::new(p.x + p.w, y)), grid());
    if !s.is_empty() {
        label(f, s, Point::new(p.x - 4.0, y - 5.0), alignment::Horizontal::Right, faint());
    }
}

/// The y axis' unit, above it.
pub(super) fn unit(f: &mut Frame, p: Plot, s: &str) {
    label(f, s, Point::new(p.x - 4.0, p.y - 12.0), alignment::Horizontal::Left, faint());
}

fn time_axis(f: &mut Frame, p: Plot, span: f32, step: f32, suffix: &str) {
    let n = (span / step).round() as usize;
    for i in 0..=n {
        let at = i as f32 * step;
        xtick(f, p, at / span, &format!("{}{suffix}", trim(at)));
    }
}

// ---- a frequency plot --------------------------------------------------------------------------

const F0: f32 = 20.0;
const F1: f32 = 20000.0;

/// Magnitude in dB over 20 Hz–20 kHz (log), fixed for every block.
pub(super) struct Freq {
    p: Plot,
    lo: f32,
    hi: f32,
}

impl Freq {
    pub(super) fn new(plot: Plot, lo: f32, hi: f32) -> Self {
        Self { p: inner(plot), lo, hi }
    }

    fn x(&self, hz: f32) -> f32 {
        self.p.px(((hz / F0).max(1e-6).ln() / (F1 / F0).ln()).clamp(0.0, 1.0))
    }

    fn y(&self, db: f32) -> f32 {
        self.p.py(((db - self.lo) / (self.hi - self.lo)).clamp(0.0, 1.0))
    }

    pub(super) fn at(&self, hz: f32, db: f32) -> Point {
        Point::new(self.x(hz), self.y(db))
    }

    /// The point on `kind`'s curve at `hz`.
    fn on(&self, kind: &BlockKind, v: V, hz: f32) -> Point {
        self.at(hz, preview::response_db(kind, v, &[hz]).and_then(|d| d.first().copied()).unwrap_or(0.0))
    }

    pub(super) fn grid(&self, f: &mut Frame) {
        for hz in [50.0f32, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0] {
            let t = (hz / F0).ln() / (F1 / F0).ln();
            let s = match hz as u32 { 100 => "100", 1000 => "1k", 10000 => "10k", _ => "" };
            xtick(f, self.p, t, s);
        }
        label(f, "Hz", Point::new(self.p.x + self.p.w, self.p.y + self.p.h + 3.0), alignment::Horizontal::Right, faint());
        let step = if self.hi - self.lo > 48.0 { 24.0 } else if self.hi - self.lo > 24.0 { 12.0 } else { 6.0 };
        let mut d = (self.lo / step).ceil() * step;
        while d <= self.hi {
            let t = (d - self.lo) / (self.hi - self.lo);
            ytick(f, self.p, t, &format!("{d:+}").replace("+0", "0"));
            if d == 0.0 {
                f.stroke(&Path::line(Point::new(self.p.x, self.y(0.0)), Point::new(self.p.x + self.p.w, self.y(0.0))), zero());
            }
            d += step;
        }
        unit(f, self.p, "dB");
    }

    /// `n` log-spaced frequencies.
    pub(super) fn freqs(&self, n: usize) -> Vec<f32> {
        (0..=n).map(|i| F0 * (F1 / F0).powf(i as f32 / n as f32)).collect()
    }

    /// The curve; where it falls off the bottom of the scale it isn't drawn.
    pub(super) fn path(&self, freqs: &[f32], db: &[f32]) -> Path {
        Path::new(|p| {
            let mut drawing = false;
            for i in 0..freqs.len() {
                let visible = db[i] >= self.lo || db.get(i + 1).is_some_and(|d| *d >= self.lo) || (i > 0 && db[i - 1] >= self.lo);
                let at = self.at(freqs[i], db[i]);
                match (visible, drawing) {
                    (true, true) => p.line_to(at),
                    (true, false) => p.move_to(at),
                    _ => {}
                }
                drawing = visible;
            }
        })
    }

    /// The area between the curve and 0 dB.
    pub(super) fn fill(&self, f: &mut Frame, freqs: &[f32], db: &[f32]) {
        let y0 = self.y(0.0);
        let area = Path::new(|p| {
            p.move_to(Point::new(self.p.x, y0));
            freqs.iter().zip(db).for_each(|(hz, d)| p.line_to(self.at(*hz, *d)));
            p.line_to(Point::new(self.p.x + self.p.w, y0));
            p.close();
        });
        f.fill(&area, ink(0.07));
    }

    fn func(&self, n: usize, g: impl Fn(f32) -> f32) -> Path {
        let freqs = self.freqs(n);
        let db: Vec<f32> = freqs.iter().map(|hz| g(*hz)).collect();
        self.path(&freqs, &db)
    }
}

fn lp2(f: f32, fc: f32, q: f32) -> f32 {
    let w = f / fc;
    1.0 / (1.0 - w * w).hypot(w / q)
}
fn peak(f: f32, fc: f32, q: f32, gain_db: f32) -> f32 {
    let a = 10f32.powf(gain_db / 40.0);
    let w = f / fc;
    (1.0 - w * w).hypot(a / q * w) / (1.0 - w * w).hypot(w / (a * q))
}
fn high_shelf(f: f32, fc: f32, q: f32, gain_db: f32) -> f32 {
    let a = 10f32.powf(gain_db / 40.0);
    let s = a.sqrt();
    let w = f / fc;
    a * (1.0 - a * w * w).hypot(s / q * w) / (a - w * w).hypot(s / q * w)
}
fn db(x: f32) -> f32 {
    20.0 * x.max(1e-6).log10()
}

// ---- filters -----------------------------------------------------------------------------------

/// The block's real response; `exact`: frequencies that must be on the curve (narrow peaks).
fn filter_curve(f: &mut Frame, plot: Plot, kind: &BlockKind, v: V, lo: f32, hi: f32, exact: &[f32]) -> Freq {
    let fr = Freq::new(plot, lo, hi);
    fr.grid(f);
    let mut freqs = fr.freqs(700);
    for hz in exact {
        freqs.extend((-8..=8).map(|k| hz * 2f32.powf(k as f32 / 400.0)));
    }
    freqs.sort_by(f32::total_cmp);
    if let Some(d) = preview::response_db(kind, v, &freqs) {
        f.stroke(&fr.path(&freqs, &d), curve().with_width(1.7));
    }
    fr
}

/// The comb on a fixed, linear 0–5 kHz axis, so its teeth are evenly spaced.
fn comb(f: &mut Frame, plot: Plot, kind: &BlockKind, v: V, f0: f32) {
    const SPAN: f32 = 5000.0;
    const LO: f32 = -36.0;
    const HI: f32 = 12.0;
    let p = inner(plot);
    for k in 0..=5 {
        xtick(f, p, k as f32 / 5.0, &if k == 0 { "0".to_string() } else { format!("{k}k") });
    }
    let mut d = LO;
    while d <= HI {
        ytick(f, p, (d - LO) / (HI - LO), &format!("{d:+}").replace("+0", "0"));
        d += 12.0;
    }
    unit(f, p, "dB");
    let y = |d: f32| p.py(((d - LO) / (HI - LO)).clamp(0.0, 1.0));
    f.stroke(&Path::line(Point::new(p.x, y(0.0)), Point::new(p.x + p.w, y(0.0))), zero());
    let freqs: Vec<f32> = (1..=1600).map(|i| SPAN * i as f32 / 1600.0).collect();
    let Some(mag) = preview::response_db(kind, v, &freqs) else { return };
    f.stroke(&Path::new(|b| polyline(b, freqs.iter().zip(&mag).map(|(hz, d)| Point::new(p.px(hz / SPAN), y(*d))))), curve().with_width(1.5));
    if f0 <= SPAN
        && let Some(at) = preview::response_db(kind, v, &[f0])
    {
        handle(f, Point::new(p.px(f0 / SPAN), y(at[0])));
    }
}

/// Flanger, phaser: the response at the sweep's middle (ink) and at its two ends (faint).
fn swept(f: &mut Frame, plot: Plot, kind: &BlockKind, v: V, centre: Option<f32>) {
    let fr = Freq::new(plot, -48.0, 24.0);
    fr.grid(f);
    let mut freqs = fr.freqs(1800);
    freqs.dedup();
    for (sweep, stroke) in [(-1.0, Stroke::default().with_color(ink(0.18)).with_width(1.0)), (1.0, Stroke::default().with_color(ink(0.18)).with_width(1.0)), (0.0, curve().with_width(1.4))] {
        if let Some(d) = preview::sweep_db(kind, v, &freqs, sweep) {
            f.stroke(&fr.path(&freqs, &d), stroke);
        }
    }
    if let Some(hz) = centre {
        handle(f, fr.on(kind, v, hz));
    }
}

/// The band at full range (dashed) around 0 dB (what it does right now comes with the live data).
fn dynamic_eq(f: &mut Frame, plot: Plot, fc: f32, q: f32, range: f32, mode: DynamicMode) {
    let full = if mode == DynamicMode::Cut { -range } else { range };
    let fr = Freq::new(plot, -24.0, 24.0);
    fr.grid(f);
    f.stroke(&fr.func(320, |hz| db(peak(hz, fc, q.max(0.1), full))), dashed(muted()).with_width(1.3));
    f.stroke(&Path::line(fr.at(F0, 0.0), fr.at(F1, 0.0)), curve());
    handle(f, fr.at(fc, 0.0));
}

// ---- dynamics ----------------------------------------------------------------------------------

/// One hit before (grey) and after (ink) in dB over 200 ms: the attack lifted or softened, the
/// sustain raised or lowered. An illustration.
fn transient(f: &mut Frame, plot: Plot, attack_db: f32, sustain_db: f32) {
    const SPAN: f32 = 200.0;
    const LO: f32 = -48.0;
    const HI: f32 = 24.0;
    let p = inner(plot);
    time_axis(f, p, SPAN, 50.0, "");
    label(f, "ms", Point::new(p.x + p.w, p.y + p.h - 11.0), alignment::Horizontal::Right, faint());
    let mut d = LO;
    while d <= HI {
        ytick(f, p, (d - LO) / (HI - LO), &format!("{d:+}").replace("+0", "0"));
        d += 24.0;
    }
    unit(f, p, "dB");
    let y = |d: f32| p.py(((d - LO) / (HI - LO)).clamp(0.0, 1.0));
    // Input: 5 ms rise, then −0.2 dB per ms.
    let input = |ms: f32| if ms < 5.0 { LO + (0.0 - LO) * ms / 5.0 } else { -(ms - 5.0) * 0.2 };
    let output = |ms: f32| {
        let a = if ms < 5.0 { attack_db * ms / 5.0 } else if ms < 25.0 { attack_db * (1.0 - (ms - 5.0) / 20.0) } else { 0.0 };
        let s = sustain_db * ((ms - 10.0) / 30.0).clamp(0.0, 1.0);
        input(ms) + a + s
    };
    let at = |ms: f32, g: &dyn Fn(f32) -> f32| Point::new(p.px(ms / SPAN), y(g(ms)));
    let area = Path::new(|b| {
        b.move_to(Point::new(p.x, p.y + p.h));
        (0..=200).for_each(|i| b.line_to(at(i as f32, &input)));
        b.line_to(Point::new(p.x + p.w, p.y + p.h));
        b.close();
    });
    f.fill(&area, ink(0.1));
    f.stroke(&Path::new(|b| polyline(b, (0..=400).map(|i| at(i as f32 / 2.0, &output)))), curve());
    label(f, "in ▒   out ━", Point::new(p.x + p.w, p.y), alignment::Horizontal::Right, faint());
}

/// Input → output level, exactly the DSP's static curve: below the threshold the level falls by
/// (ratio − 1) dB per dB, at most by the range; from there it runs parallel to the identity.
fn gate(f: &mut Frame, plot: Plot, thr: f32, ratio: f32, range: f32) {
    const FLOOR: f32 = -90.0;
    let a = inner(plot);
    let sq = Plot { x: a.x + (a.w - a.h) / 2.0, y: a.y, w: a.h, h: a.h };
    let to = |i: f32, o: f32| Point::new(sq.px(i / -FLOOR + 1.0), sq.py(o / -FLOOR + 1.0));
    let out = |i: f32| if i >= thr { i } else { i - ((thr - i) * (ratio - 1.0)).min(range) };
    for k in 0..=3 {
        let d = FLOOR + k as f32 * 30.0;
        let t = k as f32 / 3.0;
        xtick(f, sq, t, &format!("{d}"));
        ytick(f, sq, t, &format!("{d}"));
    }
    unit(f, sq, "out dB");
    label(f, "in dB", Point::new(sq.x + sq.w + 18.0, sq.y + sq.h + 3.0), alignment::Horizontal::Left, faint());
    f.stroke(&Path::line(to(FLOOR, FLOOR), to(0.0, 0.0)), dashed(faint()));
    f.stroke(&Path::line(to(thr, FLOOR), to(thr, 0.0)), dashed(muted()));
    // The curve only rises: find where it enters the plot, then its corners.
    let (mut lo, mut hi) = (FLOOR, thr);
    for _ in 0..32 {
        let m = (lo + hi) / 2.0;
        if out(m) < FLOOR { lo = m } else { hi = m }
    }
    let start = hi;
    let knee = if ratio > 1.0 { thr - range / (ratio - 1.0) } else { thr };
    let mut xs = vec![start];
    if knee > start {
        xs.push(knee);
    }
    // The drop at the threshold is continuous, so these corners are the whole curve.
    xs.extend([thr, 0.0]);
    f.stroke(&Path::new(|p| polyline(p, xs.iter().map(|i| to(*i, out(*i).max(FLOOR))))), curve());
    handle(f, to(thr, thr));
}

/// The band it watches (shaded) and the most it takes off there (dashed).
fn de_esser(f: &mut Frame, plot: Plot, fc: f32, range: f32) {
    let fr = Freq::new(plot, -24.0, 12.0);
    fr.grid(f);
    let x = fr.x(fc);
    f.fill(&Path::rectangle(Point::new(x, fr.p.y), Size::new(fr.p.x + fr.p.w - x, fr.p.h)), ink(0.035));
    f.stroke(&Path::line(Point::new(x, fr.p.y), Point::new(x, fr.p.y + fr.p.h)), dashed(muted()));
    f.stroke(&fr.func(240, |hz| db(high_shelf(hz, fc, 0.707, -range))), dashed(muted()).with_width(1.3));
    f.stroke(&Path::line(fr.at(F0, 0.0), fr.at(F1, 0.0)), curve());
    handle(f, fr.at(fc, 0.0));
}

/// The target level (dashed) and the window it rides the level in (±range).
fn rider(f: &mut Frame, plot: Plot, target: f32, range: f32) {
    const FLOOR: f32 = -72.0;
    let p = Plot { h: plot.h, ..inner(plot) };
    let y = |d: f32| p.py(((d - FLOOR) / -FLOOR).clamp(0.0, 1.0));
    for d in [-72.0f32, -48.0, -24.0, 0.0] {
        ytick(f, p, (d - FLOOR) / -FLOOR, &format!("{d}"));
    }
    unit(f, p, "dB");
    let (top, bottom) = (y(target + range), y(target - range));
    f.fill(&Path::rectangle(Point::new(p.x, top), Size::new(p.w, bottom - top)), ink(0.06));
    f.stroke(&Path::line(Point::new(p.x, y(target)), Point::new(p.x + p.w, y(target))), dashed(theme::c().frame_ink));
    label(f, "target", Point::new(p.x + p.w, y(target) - 13.0), alignment::Horizontal::Right, muted());
    label(f, &format!("±{range:.0} dB"), Point::new(p.x + p.w, bottom + 3.0), alignment::Horizontal::Right, faint());
}

// ---- distortion --------------------------------------------------------------------------------

/// 5 ms of a 400 Hz sine (grey) and what the crusher makes of it (ink), the DSP's own quantizer and
/// sample-and-hold; the wet signal, whatever the mix.
fn bitcrusher(f: &mut Frame, plot: Plot, bits: f32, rate: f32) {
    const SPAN: f32 = 0.005;
    let p = inner(plot);
    time_axis(f, p, 5.0, 1.0, "");
    label(f, "ms", Point::new(p.x + p.w, p.y + p.h - 11.0), alignment::Horizontal::Right, faint());
    for (t, s) in [(0.0, "-1"), (0.5, "0"), (1.0, "1")] {
        ytick(f, p, t, s);
    }
    let mid = p.y + p.h / 2.0;
    let amp = p.h / 2.0;
    let levels = 2f32.powf(bits.clamp(1.0, 24.0) - 1.0);
    let sine = |u: f32| (u * 2.0 * TAU).sin() * 0.9;
    if levels <= 16.0 {
        for k in -(levels as i32)..=levels as i32 {
            let yv = k as f32 / levels;
            f.stroke(&Path::line(Point::new(p.x, mid - yv * amp), Point::new(p.x + p.w, mid - yv * amp)), grid());
        }
    }
    f.stroke(&Path::new(|b| polyline(b, (0..=200).map(|i| { let u = i as f32 / 200.0; Point::new(p.px(u), mid - sine(u) * amp) }))), Stroke::default().with_color(muted()).with_width(1.2));
    let steps = (rate * SPAN).clamp(2.0, 480.0) as usize;
    let path = Path::new(|b| {
        for k in 0..steps {
            let (u0, u1) = (k as f32 / steps as f32, (k + 1) as f32 / steps as f32);
            let q = (sine(u0) * levels).round() / levels;
            let pt = Point::new(p.px(u0), mid - q * amp);
            if k == 0 { b.move_to(pt) } else { b.line_to(pt) }
            b.line_to(Point::new(p.px(u1), mid - q * amp));
        }
    });
    f.stroke(&path, curve().with_line_join(LineJoin::Miter).with_width(1.7));
}

/// 20 ms: a 65 Hz input (grey), the carrier (dashed) and their product (ink, the wet signal).
fn ring_mod(f: &mut Frame, plot: Plot, carrier: f32) {
    const SPAN: f32 = 0.02;
    let p = inner(plot);
    time_axis(f, p, 20.0, 5.0, "");
    label(f, "ms", Point::new(p.x + p.w, p.y + p.h - 11.0), alignment::Horizontal::Right, faint());
    for (t, s) in [(0.0, "-1"), (0.5, "0"), (1.0, "1")] {
        ytick(f, p, t, s);
    }
    let mid = p.y + p.h / 2.0;
    let amp = p.h / 2.0 * 0.95;
    let input = |u: f32| (u * SPAN * 65.0 * TAU).sin() * 0.8;
    let car = |u: f32| (u * SPAN * carrier * TAU).sin();
    let n = 1200;
    let line = |g: &dyn Fn(f32) -> f32| Path::new(|b| polyline(b, (0..=n).map(|i| { let u = i as f32 / n as f32; Point::new(p.px(u), mid - g(u) * amp) })));
    f.stroke(&line(&|u| car(u) * 0.35), dashed(faint()));
    f.stroke(&line(&input), Stroke::default().with_color(muted()).with_width(1.2));
    f.stroke(&line(&|u| input(u) * car(u)), curve().with_width(1.5));
}

/// The tape's colour (head bump, top-end loss; drive deepens both) and wow & flutter as pitch
/// drifting over two seconds. An illustration.
fn tape(f: &mut Frame, plot: Plot, drive: f32, wow: f32, flutter: f32) {
    let top = Plot { h: plot.h - 46.0, ..plot };
    let fr = Freq::new(top, -12.0, 6.0);
    fr.grid(f);
    let mag = |hz: f32| db(peak(hz, 70.0, 1.1, 1.5 + drive * 0.15) * lp2(hz, 18000.0 - drive * 300.0, 0.6));
    f.stroke(&fr.func(240, mag), curve());
    let lane = Plot { x: fr.p.x, y: plot.y + plot.h - 34.0, w: fr.p.w, h: 34.0 };
    let m = lane.y + lane.h / 2.0;
    f.stroke(&Path::line(Point::new(lane.x, lane.y), Point::new(lane.x + lane.w, lane.y)), zero());
    f.stroke(&Path::line(Point::new(lane.x, m), Point::new(lane.x + lane.w, m)), dashed(faint()));
    label(f, "pitch", Point::new(lane.x - 4.0, m - 5.0), alignment::Horizontal::Right, faint());
    let dev = |u: f32| {
        let s = u * 2.0;
        wow * (s * TAU * 0.6).sin() * 0.8 + flutter * ((s * TAU * 9.0).sin() * 0.25 + (s * TAU * 16.0 + 1.0).sin() * 0.12)
    };
    f.stroke(&Path::new(|b| polyline(b, (0..=300).map(|i| { let u = i as f32 / 300.0; Point::new(lane.px(u), m - dev(u).clamp(-1.0, 1.0) * lane.h / 2.0) }))), curve().with_width(1.4));
}

// ---- space -------------------------------------------------------------------------------------

/// The fixed time axis of the reverbs, seconds.
const REVERB_SPAN: f32 = 10.0;

/// The decay per band in dB on a fixed 0–10 s axis: damping makes the highs die before the mids, the
/// lows ring a little longer. The gap at the start is the predelay; −60 dB marks RT60.
fn reverb(f: &mut Frame, plot: Plot, decay_s: f32, damping: f32, predelay_ms: f32) {
    let p = inner(plot);
    time_axis(f, p, REVERB_SPAN, 2.0, " s");
    let x = |s: f32| p.px((s / REVERB_SPAN).clamp(0.0, 1.0));
    let y = |d: f32| p.py(((d + 72.0) / 72.0).clamp(0.0, 1.0));
    for d in [0.0f32, -24.0, -48.0, -60.0, -72.0] {
        ytick(f, p, (d + 72.0) / 72.0, &format!("{d}"));
    }
    unit(f, p, "dB");
    f.stroke(&Path::line(Point::new(p.x, y(-60.0)), Point::new(p.x + p.w, y(-60.0))), dashed(faint()));
    let pd = predelay_ms / 1000.0;
    // A straight line in dB: −60 dB after `rt`; cut where it leaves the plot.
    let band = |rt: f32| Path::new(|b| {
        let end = (pd + rt * 72.0 / 60.0).min(REVERB_SPAN);
        b.move_to(Point::new(x(0.0), y(-72.0)));
        b.line_to(Point::new(x(pd), y(-72.0)));
        b.line_to(Point::new(x(pd), y(0.0)));
        b.line_to(Point::new(x(end), y(-(end - pd) * 60.0 / rt)));
    });
    let (high, low) = (decay_s * (1.0 - 0.7 * damping), decay_s * 1.3);
    f.stroke(&band(low), dashed(faint()).with_width(1.3));
    f.stroke(&band(high), Stroke::default().with_color(muted()).with_width(1.5));
    f.stroke(&band(decay_s), curve());
    label(f, "low ┄  high ─  mid ━", Point::new(p.x + p.w, p.y), alignment::Horizontal::Right, faint());
}

/// The decay (ink) and the pitched layers it feeds (grey), after the predelay, on the reverb's axis.
fn shimmer(f: &mut Frame, plot: Plot, decay_s: f32, predelay_ms: f32, pitch: f32, amount: f32) {
    let p = inner(plot);
    time_axis(f, p, REVERB_SPAN, 2.0, " s");
    for (t, s) in [(0.0, "0"), (0.5, "50"), (1.0, "100")] {
        ytick(f, p, t, s);
    }
    unit(f, p, "%");
    let pd = predelay_ms / 1000.0;
    let at = |s: f32, a: f32| Point::new(p.px((s / REVERB_SPAN).clamp(0.0, 1.0)), p.py(a.clamp(0.0, 1.0)));
    let tau = decay_s / 6.9;
    let env = |delay: f32, rise: f32, g: f32| move |s: f32| { let x = s - pd - delay; if x < 0.0 { 0.0 } else { g * (1.0 - (-x / rise).exp()) * (-x / tau).exp() } };
    let line = |e: &dyn Fn(f32) -> f32| Path::new(|b| polyline(b, (0..=500).map(|i| { let s = REVERB_SPAN * i as f32 / 500.0; at(s, e(s)) })));
    let main = env(0.0, 0.01, 0.95);
    let first = env(decay_s * 0.06, decay_s * 0.18, 0.75 * amount);
    let second = env(decay_s * 0.18, decay_s * 0.27, 0.45 * amount * amount);
    let area = Path::new(|b| {
        b.move_to(Point::new(p.x, p.y + p.h));
        (0..=500).for_each(|i| { let s = REVERB_SPAN * i as f32 / 500.0; b.line_to(at(s, main(s))) });
        b.line_to(Point::new(p.x + p.w, p.y + p.h));
        b.close();
    });
    f.fill(&area, ink(0.06));
    f.stroke(&line(&main), curve());
    f.stroke(&line(&first), Stroke::default().with_color(muted()).with_width(1.5));
    f.stroke(&line(&second), dashed(muted()).with_width(1.2));
    label(f, &format!("{:+} st ─  {:+} st ┄", pitch.round(), (2.0 * pitch).round()), Point::new(p.x + p.w, p.y), alignment::Horizontal::Right, faint());
}

// ---- pitch & rhythm ----------------------------------------------------------------------------

/// The shift on a semitone ruler (octaves marked), and 10 ms of a sine before (grey) and after (ink).
fn pitch_shift(f: &mut Frame, plot: Plot, st: f32) {
    let a = inner(plot);
    let ry = plot.y + 4.0;
    let x = |s: f32| a.px((s + 24.0) / 48.0);
    f.stroke(&Path::line(Point::new(a.x, ry), Point::new(a.x + a.w, ry)), zero());
    for s in -24..=24 {
        let octave = s % 12 == 0;
        let h = if octave { 6.0 } else { 3.0 };
        f.stroke(&Path::line(Point::new(x(s as f32), ry - h), Point::new(x(s as f32), ry + h)), Stroke::default().with_color(if octave { muted() } else { ink(0.16) }).with_width(1.0));
        if octave {
            label(f, &format!("{s:+}").replace("+0", "0"), Point::new(x(s as f32), ry + 8.0), alignment::Horizontal::Center, faint());
        }
    }
    label(f, "st", Point::new(a.x - 4.0, ry - 5.0), alignment::Horizontal::Right, faint());
    f.stroke(&Path::line(Point::new(x(0.0), ry), Point::new(x(st), ry)), curve().with_width(3.0));
    f.fill(&Path::circle(Point::new(x(st), ry), 4.5), theme::c().frame_ink);
    let p = Plot { x: a.x, y: ry + 24.0, w: a.w, h: a.y + a.h - ry - 24.0 };
    time_axis(f, p, 10.0, 2.0, "");
    label(f, "ms", Point::new(p.x + p.w, p.y + p.h - 11.0), alignment::Horizontal::Right, faint());
    for (t, s) in [(0.0, "-1"), (0.5, "0"), (1.0, "1")] {
        ytick(f, p, t, s);
    }
    let m = p.y + p.h / 2.0;
    let amp = p.h / 2.0 * 0.92;
    let r = 2f32.powf(st / 12.0);
    let sine = |k: f32| Path::new(|b| polyline(b, (0..=300).map(|i| { let u = i as f32 / 300.0; Point::new(p.px(u), m - (u * 4.0 * TAU * k).sin() * amp) })));
    f.stroke(&sine(1.0), Stroke::default().with_color(muted()).with_width(1.2));
    f.stroke(&sine(r), curve());
}

fn hash(n: u32) -> f32 {
    let s = (n as f32 * 12.9898 + 4.1414).sin() * 43758.547;
    s - s.floor()
}

/// One second of grains: x = how far back in the buffer it reads (the spread, shaded), width = its
/// length, y = its pitch (with the jitter); hollow = reversed. The positions are an example of the
/// random ones the DSP picks.
#[allow(clippy::too_many_arguments)]
fn granular(f: &mut Frame, plot: Plot, grain_ms: f32, density: f32, spread_ms: f32, pitch: f32, jitter_ct: f32, reverse: f32) {
    const SPAN: f32 = 2000.0;
    let p = inner(plot);
    time_axis(f, p, 2.0, 0.5, " s");
    for st in [-24.0f32, -12.0, 0.0, 12.0, 24.0] {
        ytick(f, p, (st + 24.0) / 48.0, &format!("{st:+}").replace("+0", "0"));
    }
    unit(f, p, "st");
    label(f, "ago", Point::new(p.x + p.w, p.y + p.h - 11.0), alignment::Horizontal::Right, faint());
    let x = |ms: f32| p.px((ms / SPAN).clamp(0.0, 1.0));
    f.fill(&Path::rectangle(Point::new(p.x, p.y), Size::new(x(spread_ms) - p.x, p.h)), ink(0.04));
    let n = density.round().clamp(1.0, 100.0) as u32;
    for k in 0..n {
        let at = hash(k * 3 + 1) * spread_ms;
        let st = pitch + jitter_ct / 100.0 * (2.0 * hash(k * 5 + 2) - 1.0);
        // Grains at the same pitch stack a little so each stays visible.
        let lane = (hash(k * 7 + 3) - 0.5) * 16.0;
        let y = p.py(((st + 24.0) / 48.0).clamp(0.0, 1.0)) + lane;
        let w = (x(at + grain_ms) - x(at)).max(2.0);
        let r = Path::rounded_rectangle(Point::new(x(at), y - 2.0), Size::new(w, 4.0), 2.0.into());
        if hash(k * 11 + 4) < reverse {
            f.stroke(&r, Stroke::default().with_color(ink(0.75)).with_width(1.0));
        } else {
            f.fill(&r, ink(0.75));
        }
    }
}

/// A rhythm of hits over 2 s; at 0.8 s the last `ms` is captured (outlined) and repeats. Off: the
/// repeats it would play, faint.
fn stutter(f: &mut Frame, plot: Plot, ms: f32, active: bool) {
    const SPAN: f32 = 2000.0;
    const TRIGGER: f32 = 800.0;
    let p = inner(plot);
    time_axis(f, p, 2.0, 0.5, " s");
    for (t, s) in [(0.0, "-1"), (0.5, "0"), (1.0, "1")] {
        ytick(f, p, t, s);
    }
    let slice = ms.clamp(5.0, TRIGGER);
    let mid = p.y + p.h / 2.0;
    let amp = p.h / 2.0 * 0.92;
    // A hit every 125 ms, each a little different.
    let level = |t: f32| {
        let i = (t / 125.0).floor();
        (-(t - i * 125.0) / 32.0).exp() * (0.55 + 0.45 * hash(i as u32 + 9))
    };
    let mut px = p.x;
    while px < p.x + p.w {
        let t = (px - p.x) / p.w * SPAN;
        let (src, colour) = if t < TRIGGER {
            (t, ink(0.4))
        } else {
            (TRIGGER - slice + (t - TRIGGER) % slice, if active { ink(0.9) } else { ink(0.2) })
        };
        let a = level(src) * amp;
        f.fill(&Path::rectangle(Point::new(px, mid - a), Size::new(1.5, 2.0 * a)), colour);
        px += 2.5;
    }
    let (x0, x1) = (p.px((TRIGGER - slice) / SPAN), p.px(TRIGGER / SPAN));
    f.stroke(&Path::rounded_rectangle(Point::new(x0, p.y), Size::new(x1 - x0, p.h), 3.0.into()), Stroke::default().with_color(theme::c().frame_ink).with_width(1.3));
    let step = (x1 - x0).max(1.0);
    if step >= 6.0 {
        let mut x = x1 + step;
        while x < p.x + p.w {
            f.stroke(&Path::line(Point::new(x, p.y), Point::new(x, p.y + p.h)), dashed(faint()));
            x += step;
        }
    }
}

// ---- stereo & utility --------------------------------------------------------------------------

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

/// The same hit on both channels on a fixed 0–40 ms axis; `delay` > 0 = the right one later, < 0
/// the left.
fn haas(f: &mut Frame, plot: Plot, delay: f32) {
    let p = inner(plot);
    time_axis(f, p, 40.0, 10.0, "");
    label(f, "ms", Point::new(p.x + p.w, p.y + p.h - 11.0), alignment::Horizontal::Right, faint());
    let x = |ms: f32| p.px((ms / 40.0).clamp(0.0, 1.0));
    let lane = p.h / 2.0;
    let mids = [p.y + lane / 2.0, p.y + lane * 1.5];
    let later = if delay < 0.0 { 0 } else { 1 };
    let d = delay.abs();
    for (i, m) in mids.iter().enumerate() {
        let off = if i == later { d } else { 0.0 };
        let colour = if i == later && d > 0.0 { theme::c().frame_ink } else { muted() };
        f.stroke(&Path::line(Point::new(p.x, *m), Point::new(p.x + p.w, *m)), zero());
        let hit = Path::new(|b| polyline(b, (0..=240).filter_map(|k| {
            let ms = k as f32 / 240.0 * 24.0;
            (off + ms <= 40.0).then(|| Point::new(x(off + ms), m - (ms * TAU / 2.2).sin() * (-ms / 4.8).exp() * lane * 0.42))
        })));
        f.stroke(&hit, Stroke::default().with_color(colour).with_width(if i == later { 1.8 } else { 1.4 }));
        label(f, if i == 0 { "L" } else { "R" }, Point::new(p.x - 8.0, m - 5.0), alignment::Horizontal::Right, colour);
    }
    let my = (mids[0] + mids[1]) / 2.0;
    f.stroke(&Path::line(Point::new(x(0.0), my), Point::new(x(d), my)), curve().with_width(1.2));
}

/// What happens to the side signal (ink, a Linkwitz-Riley high pass of the set slope) while the mid
/// stays (dashed); the mono zone hatched.
fn mono_bass(f: &mut Frame, plot: Plot, fc: f32, slope: f32) {
    let fr = Freq::new(plot, -48.0, 6.0);
    fr.grid(f);
    let x1 = fr.x(fc);
    let mut x = fr.p.x - fr.p.h;
    while x < x1 {
        let (x_a, x_b) = (x.max(fr.p.x), (x + fr.p.h).min(x1));
        let bottom = fr.p.y + fr.p.h;
        f.stroke(&Path::line(Point::new(x_a, bottom - (x_a - x)), Point::new(x_b, bottom - (x_b - x))), Stroke::default().with_color(ink(0.07)).with_width(1.0));
        x += 7.0;
    }
    f.stroke(&Path::line(fr.at(F0, 0.0), fr.at(F1, 0.0)), dashed(muted()));
    let power = if slope >= 36.0 { 8 } else { 4 };
    let side = |hz: f32| { let w = (hz / fc).powi(power); db(w / (1.0 + w)) };
    f.stroke(&fr.func(320, side), curve());
    handle(f, fr.at(fc, side(fc)));
    label(f, "mono", Point::new(x1 - 6.0, fr.y(-40.0)), alignment::Horizontal::Right, muted());
    label(f, "mid", Point::new(fr.p.x + fr.p.w, fr.y(0.0) - 13.0), alignment::Horizontal::Right, muted());
    label(f, "side", Point::new(fr.p.x + fr.p.w, fr.y(0.0) + 3.0), alignment::Horizontal::Right, theme::c().frame_ink);
}

/// One stretch per channel; the inverted ones in ink, flipped.
fn invert(f: &mut Frame, plot: Plot, channels: Channels) {
    let p = inner(plot);
    time_axis(f, p, 3.0, 1.0, "");
    label(f, "cycles", Point::new(p.x + p.w, p.y + p.h - 11.0), alignment::Horizontal::Right, faint());
    let lane = p.h / 2.0;
    for (i, name) in ["L", "R"].iter().enumerate() {
        let flipped = matches!((channels, i), (Channels::Both, _) | (Channels::Left, 0) | (Channels::Right, 1));
        let m = p.y + lane * (i as f32 + 0.5);
        let colour = if flipped { theme::c().frame_ink } else { muted() };
        let sign = if flipped { -1.0 } else { 1.0 };
        f.stroke(&Path::line(Point::new(p.x, m), Point::new(p.x + p.w, m)), zero());
        f.stroke(&Path::new(|b| polyline(b, (0..=200).map(|k| { let u = k as f32 / 200.0; Point::new(p.px(u), m - sign * (u * 3.0 * TAU).sin() * lane * 0.4) }))), Stroke::default().with_color(colour).with_width(if flipped { 2.0 } else { 1.4 }));
        label(f, &if flipped { format!("{name} Ø") } else { name.to_string() }, Point::new(p.x - 4.0, m - 5.0), alignment::Horizontal::Right, colour);
    }
}

// ---- modulation (the LFO from phase 0 until the live phase comes) ------------------------------

/// The time axis of the modulation blocks, seconds.
const LFO_SPAN: f32 = 2.0;
const LFO_POINTS: usize = 1000;

fn lfo(shape: LfoShape, phase: f32) -> f32 {
    let p = phase.rem_euclid(1.0);
    match shape {
        LfoShape::Sine => (p * TAU).sin(),
        LfoShape::Triangle => if p < 0.25 { p * 4.0 } else if p < 0.75 { 2.0 - p * 4.0 } else { p * 4.0 - 4.0 },
        LfoShape::Square => if p < 0.5 { 1.0 } else { -1.0 },
    }
}

/// The delay time of the two voices over 2 s, around the DSP's 12 ms (±4 ms × depth).
fn chorus(f: &mut Frame, plot: Plot, hz: f32, depth: f32) {
    let p = inner(plot);
    time_axis(f, p, LFO_SPAN, 0.5, " s");
    for (ms, t) in [(8, 0.0), (10, 0.25), (12, 0.5), (14, 0.75), (16, 1.0)] {
        ytick(f, p, t, &ms.to_string());
    }
    unit(f, p, "ms");
    let voice = |off: f32| Path::new(|b| polyline(b, (0..=LFO_POINTS).map(|k| {
        let u = k as f32 / LFO_POINTS as f32;
        Point::new(p.px(u), p.py(0.5 + 0.5 * depth * (u * LFO_SPAN * hz * TAU + off).sin()))
    })));
    f.stroke(&voice(PI), Stroke::default().with_color(muted()).with_width(1.4));
    f.stroke(&voice(0.0), curve().with_width(1.7));
}

/// The gain over 2 s, the area under it filled.
fn tremolo(f: &mut Frame, plot: Plot, hz: f32, depth: f32, shape: LfoShape, offset_deg: f32) {
    let p = inner(plot);
    time_axis(f, p, LFO_SPAN, 0.5, " s");
    for (t, s) in [(0.0, "0"), (0.5, "50"), (1.0, "100")] {
        ytick(f, p, t, s);
    }
    unit(f, p, "%");
    let at = |k: usize| {
        let u = k as f32 / LFO_POINTS as f32;
        Point::new(p.px(u), p.py(1.0 - depth * (0.5 + 0.5 * lfo(shape, u * LFO_SPAN * hz + offset_deg / 360.0))))
    };
    let area = Path::new(|b| {
        b.move_to(Point::new(p.x, p.y + p.h));
        (0..=LFO_POINTS).for_each(|k| b.line_to(at(k)));
        b.line_to(Point::new(p.x + p.w, p.y + p.h));
        b.close();
    });
    f.fill(&area, ink(0.08));
    f.stroke(&Path::new(|b| polyline(b, (0..=LFO_POINTS).map(at))), curve().with_width(1.7));
}

/// Where the sound sits over 2 s, from left (top) to right (bottom).
fn auto_pan(f: &mut Frame, plot: Plot, hz: f32, depth: f32, shape: LfoShape, offset_deg: f32) {
    let p = inner(plot);
    time_axis(f, p, LFO_SPAN, 0.5, " s");
    for (t, s) in [(1.0, "L"), (0.5, "C"), (0.0, "R")] {
        ytick(f, p, t, s);
    }
    f.stroke(&Path::line(Point::new(p.x, p.py(0.5)), Point::new(p.x + p.w, p.py(0.5))), zero());
    f.stroke(&Path::new(|b| polyline(b, (0..=LFO_POINTS).map(|k| {
        let u = k as f32 / LFO_POINTS as f32;
        Point::new(p.px(u), p.py(0.5 - 0.5 * depth * lfo(shape, u * LFO_SPAN * hz + offset_deg / 360.0)))
    }))), curve().with_width(1.7));
}

// ---- containers --------------------------------------------------------------------------------

/// The bands' responses meeting at the crossovers (Linkwitz-Riley of the set slope), a handle at each.
fn band_split(f: &mut Frame, plot: Plot, crossovers: &[f32], slope: f32) {
    let fr = Freq::new(plot, -48.0, 6.0);
    fr.grid(f);
    let power = if slope >= 36.0 { 8 } else { 4 };
    let lp = |hz: f32, c: f32| 1.0 / (1.0 + (hz / c).powi(power));
    let hp = |hz: f32, c: f32| (hz / c).powi(power) / (1.0 + (hz / c).powi(power));
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
        f.stroke(&fr.func(320, band), Stroke::default().with_color(colour).with_width(if b % 2 == 1 { 2.0 } else { 1.6 }));
        let lo = if b == 0 { F0 } else { crossovers[b - 1] };
        let hi = if b == n - 1 { F1 } else { crossovers[b] };
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

// ---- small helpers -----------------------------------------------------------------------------

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
    fn names() {
        assert_eq!(note_name(440.0), "A4");
        assert_eq!(note_name(261.63), "C4");
        assert_eq!(vowel_name(0.0), "a");
        assert_eq!(vowel_name(1.5), "e → i");
    }
}
