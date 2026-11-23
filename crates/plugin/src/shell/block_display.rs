//! The block modal's display: a picture of what the block does, live (it follows the knobs and
//! their modulation). Designs: `docs/research/mockups/block-displays.html`, plan:
//! `docs/research/block-displays.md`. White and grey only (the theme's ink): ink for what the block
//! does, grey for references and the input, dashed for thresholds and the neutral line.
//!
//! - **Response** (low/high pass, peak, shelves): magnitude over 20 Hz–20 kHz, a handle on the curve.
//! - **Transfer** (drive, waveshaper, wavefolder, clipper): input → output on the left, a test sine
//!   through it on the right.
//! - **Dynamics** (compressors, limiter): the static curve with a ball where the block works now,
//!   and the gain reduction over the last seconds on the right.
//! - **Taps** (delay): the dry hit and the repeats.
//! - **Level** (meter): the level of the last seconds, from the block's meter.
//! - **From the knobs** (`params`): every other block that can be drawn from its settings alone
//!   (filters, distortion, space, stereo, the modulation blocks at LFO phase 0, containers).
//! - The rest (Gain, Width, Granular, Stutter, DC block) say what they do until their live
//!   displays come (`docs/research/block-displays.md` §1, step 3).
//!
//! The curves come from `dsp::preview`, i.e. the DSP's own formulas. A caption at the top left says
//! the main settings.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use dsp::preview;
use patch::chain::{BlockKind, ParamRef};
use patch::{BlockSpec, Patch, Values};

mod params;

use super::{chain_view, theme};
use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, Padding, Pixels, Point, Rectangle, Renderer, Size, Theme,
    alignment, mouse,
    widget::{
        canvas::{self, Frame, LineDash, Path, Stroke, Text},
        column, container, text,
    },
};

/// The display at `size` (the block modal gives it what its knobs leave).
pub fn display<'a, M: 'a>(patch: &'a Patch, values: &'a Values, block: &'a BlockSpec, size: Size) -> Element<'a, M> {
    let Some(picture) = Picture::of(&block.kind) else {
        return card(description(&block.kind), size);
    };
    let display = Display { patch, values, block, picture };
    container(canvas::Canvas::new(display).width(Fill).height(Fill)).width(size.width).height(size.height).style(ground).into()
}

// ---- which picture -----------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Picture {
    Response,
    Transfer,
    Dynamics,
    Taps,
    Level,
    Params,
}

impl Picture {
    fn of(kind: &BlockKind) -> Option<Self> {
        Some(match kind {
            BlockKind::LowPass(_) | BlockKind::HighPass(_) | BlockKind::PeakEq(_) | BlockKind::LowShelf(_) | BlockKind::HighShelf(_) => {
                Self::Response
            }
            BlockKind::Drive { .. } | BlockKind::Waveshaper { .. } | BlockKind::Wavefolder { .. } | BlockKind::Clipper { .. } => Self::Transfer,
            BlockKind::Compressor(_) | BlockKind::FetCompressor(_) | BlockKind::OptoCompressor(_) | BlockKind::VariMuCompressor(_) | BlockKind::Limiter { .. } => {
                Self::Dynamics
            }
            BlockKind::Delay { .. } => Self::Taps,
            BlockKind::Meter => Self::Level,
            k if params::draws(k) => Self::Params,
            _ => return None,
        })
    }
}

struct Display<'a> {
    patch: &'a Patch,
    values: &'a Values,
    block: &'a BlockSpec,
    picture: Picture,
}

impl Display<'_> {
    fn norm(&self, r: &ParamRef) -> f32 {
        let spec = &self.patch.params[r.0];
        self.values.params.get(r.0).copied().unwrap_or_else(|| spec.normalize(spec.default_plain()))
    }

    /// A param's current plain value (after modulation).
    fn value(&self, r: &ParamRef) -> f32 {
        self.patch.params[r.0].plain(self.norm(r))
    }

    /// A param's current value as it displays.
    fn show(&self, r: &ParamRef) -> String {
        self.patch.params[r.0].display(self.norm(r))
    }

    /// The block's live meter (0..1), if it has one.
    fn meter(&self) -> Option<f32> {
        let i = self.patch.meters.iter().position(|id| *id == self.block.id)?;
        self.values.blocks.get(i).copied()
    }

    /// The main settings, for the caption.
    fn caption(&self) -> String {
        let show = |rs: &[(&str, &ParamRef)]| rs.iter().map(|(label, r)| format!("{label}{}", self.show(r))).collect::<Vec<_>>().join(" · ");
        match &self.block.kind {
            BlockKind::PeakEq(e) | BlockKind::LowShelf(e) | BlockKind::HighShelf(e) => show(&[("", &e.freq), ("", &e.gain), ("Q ", &e.q)]),
            BlockKind::LowPass(f) | BlockKind::HighPass(f) => {
                format!("{} · {} dB/oct · Q {}", self.show(&f.freq), self.value(&f.slope).round(), self.show(&f.q))
            }
            BlockKind::Drive { amount, mix } => show(&[("drive ", amount), ("mix ", mix)]),
            BlockKind::Waveshaper { curve, drive, output, .. } if curve.len() <= MAX_POINTS => {
                format!("{}-point curve · {}", curve.len(), show(&[("drive ", drive), ("out ", output)]))
            }
            BlockKind::Waveshaper { drive, output, .. } => show(&[("drive ", drive), ("out ", output)]),
            BlockKind::Wavefolder { amount, bias, .. } => show(&[("", amount), ("bias ", bias)]),
            BlockKind::Clipper { ceiling, softness, .. } => show(&[("ceiling ", ceiling), ("softness ", softness)]),
            BlockKind::Compressor(c) | BlockKind::FetCompressor(c) | BlockKind::OptoCompressor(c) | BlockKind::VariMuCompressor(c) => {
                show(&[("", &c.threshold), ("ratio ", &c.ratio), ("knee ", &c.knee)])
            }
            BlockKind::Limiter { ceiling, release, .. } => show(&[("ceiling ", ceiling), ("release ", release)]),
            BlockKind::Delay { time, sync, feedback, pingpong, .. } => {
                let step = match sync {
                    Some(beats) => format!("every {} beat{}", trim(*beats), if *beats == 1.0 { "" } else { "s" }),
                    None => format!("every {}", self.show(time)),
                };
                format!("{step} · feedback {}{}", self.show(feedback), if *pingpong { " · ping-pong" } else { "" })
            }
            k => params::caption(k, &|r| self.value(r), &|r| self.show(r)),
        }
    }
}

/// Gain reduction over the last seconds (the dynamics picture), sampled once per frame.
#[derive(Default)]
pub struct History(RefCell<(Option<Instant>, VecDeque<f32>)>);

/// How many frames of history the dynamics picture keeps (~2.5 s).
const HISTORY: usize = 150;

impl History {
    fn push(&self, v: f32) -> Vec<f32> {
        let mut h = self.0.borrow_mut();
        let now = Instant::now();
        if h.0.is_none_or(|t| now.duration_since(t) >= Duration::from_millis(15)) {
            h.0 = Some(now);
            h.1.push_back(v);
            while h.1.len() > HISTORY {
                h.1.pop_front();
            }
        }
        h.1.iter().copied().collect()
    }
}

impl<M> canvas::Program<M> for Display<'_> {
    type State = History;

    fn draw(&self, history: &History, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let v = |r: &ParamRef| self.value(r);
        let (w, h) = (bounds.width, bounds.height);
        match self.picture {
            Picture::Response => response(&mut f, Plot { x: 16.0, y: 30.0, w: w - 32.0, h: h - 54.0 }, &self.block.kind, &v),
            Picture::Transfer => transfer(&mut f, Plot { x: 16.0, y: 28.0, w: w - 32.0, h: h - 42.0 }, &self.block.kind, &v),
            Picture::Dynamics => {
                let gr = self.meter().map(|m| m.clamp(0.0, 1.0) * GR_RANGE);
                let past = history.push(gr.unwrap_or(0.0));
                dynamics(&mut f, Plot { x: 16.0, y: 28.0, w: w - 32.0, h: h - 42.0 }, &self.block.kind, &v, gr.map(|g| (g, past)));
            }
            Picture::Taps => taps(&mut f, Plot { x: 16.0, y: 30.0, w: w - 32.0, h: h - 46.0 }, &self.block.kind, &v),
            Picture::Level => {
                let level = self.meter().unwrap_or(0.0);
                let past = history.push(level);
                level_history(&mut f, Plot { x: 16.0, y: 30.0, w: w - 32.0, h: h - 46.0 }, level, &past);
            }
            Picture::Params => params::draw(&mut f, Plot { x: 16.0, y: 30.0, w: w - 32.0, h: h - 46.0 }, &self.block.kind, &v),
        }
        caption(&mut f, &self.caption());
        vec![f.into_geometry()]
    }
}

// ---- pictures ----------------------------------------------------------------------------------

/// 20 Hz–20 kHz (log) at `t` 0..1.
fn freq_at(t: f32) -> f32 {
    20.0 * 1000f32.powf(t)
}

fn freq_t(hz: f32) -> f32 {
    (hz / 20.0).max(1e-6).log10() / 3.0
}

/// Magnitude over 20 Hz–20 kHz (log) with a handle on the curve. EQs: ±18 dB, the area between the
/// curve and 0 dB filled; filters: −48…+12 dB.
fn response(f: &mut Frame, plot: Plot, kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32) {
    const N: usize = 240;
    let is_filter = matches!(kind, BlockKind::LowPass(_) | BlockKind::HighPass(_));
    let (lo, hi) = if is_filter { (-48.0, 12.0) } else { (-18.0, 18.0) };
    let y = |db: f32| plot.py(((db - lo) / (hi - lo)).clamp(-0.01, 1.01));
    let freqs: Vec<f32> = (0..=N).map(|i| freq_at(i as f32 / N as f32)).collect();
    let Some(db) = preview::response_db(kind, v, &freqs) else { return };

    for hz in [50.0f32, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0] {
        let x = plot.px(freq_t(hz));
        f.stroke(&Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.h)), grid());
    }
    let mut d = (lo / 12.0).ceil() * 12.0;
    while d <= hi {
        f.stroke(&Path::line(Point::new(plot.x, y(d)), Point::new(plot.x + plot.w, y(d))), if d == 0.0 { zero() } else { grid() });
        d += 12.0;
    }
    for (hz, name) in [(100.0f32, "100"), (1000.0, "1k"), (10000.0, "10k")] {
        label(f, name, Point::new(plot.px(freq_t(hz)), plot.y + plot.h + 5.0), alignment::Horizontal::Center, faint());
    }

    let point = |i: usize| Point::new(plot.px(i as f32 / N as f32), y(db[i]));
    if !is_filter {
        let area = Path::new(|p| {
            p.move_to(Point::new(plot.x, y(0.0)));
            (0..=N).for_each(|i| p.line_to(point(i)));
            p.line_to(Point::new(plot.x + plot.w, y(0.0)));
            p.close();
        });
        f.fill(&area, theme::alpha(theme::c().frame_ink, 0.07));
    }
    f.stroke(&Path::new(|p| polyline(p, (0..=N).map(point))), curve());

    // The handle: at the frequency, on the curve (peak: at its gain; shelves: half-way).
    let (hz, at_db) = match kind {
        BlockKind::PeakEq(e) => (v(&e.freq), v(&e.gain)),
        BlockKind::LowShelf(e) | BlockKind::HighShelf(e) => (v(&e.freq), v(&e.gain) / 2.0),
        BlockKind::LowPass(fs) | BlockKind::HighPass(fs) => {
            let hz = v(&fs.freq);
            (hz, preview::response_db(kind, v, &[hz]).and_then(|d| d.first().copied()).unwrap_or(0.0))
        }
        _ => return,
    };
    handle(f, Point::new(plot.px(freq_t(hz).clamp(0.0, 1.0)), y(at_db)));
}

/// Input → output over ±1 in a square on the left (the identity dashed), and a test sine through it
/// on the right (the clean sine dashed).
fn transfer(f: &mut Frame, plot: Plot, kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32) {
    let shape = |x: f32| preview::transfer(kind, v, x);
    // The clipper's ceiling.
    let ceiling = match kind {
        BlockKind::Clipper { ceiling, .. } => Some(10f32.powf(v(ceiling) / 20.0).min(1.0)),
        _ => None,
    };
    let to = shaper(f, plot, &shape, ceiling);
    // The waveshaper's points are its data (when there are few; a dense table is just the curve).
    if let BlockKind::Waveshaper { curve: points, .. } = kind
        && (2..=MAX_POINTS).contains(&points.len())
    {
        for i in 0..points.len() {
            let x = i as f32 / (points.len() - 1) as f32 * 2.0 - 1.0;
            if let Some(y) = shape(x) {
                handle_small(f, to(x, y));
            }
        }
    }
}

/// A shape over ±1: the square with the curve (and `ceiling` dashed at ±), the test sine through
/// it. Returns the square's mapping (x, y) → point.
fn shaper(f: &mut Frame, plot: Plot, shape: &dyn Fn(f32) -> Option<f32>, ceiling: Option<f32>) -> impl Fn(f32, f32) -> Point + use<> {
    const N: usize = 200;
    let side = plot.h;
    let sq = Plot { x: plot.x, y: plot.y, w: side, h: side };
    let to = move |x: f32, y: f32| Point::new(sq.px((x + 1.0) / 2.0), sq.py(((y + 1.0) / 2.0).clamp(-0.03, 1.03)));

    for i in 0..=4 {
        let t = i as f32 / 4.0;
        f.stroke(&Path::line(Point::new(sq.px(t), sq.y), Point::new(sq.px(t), sq.y + sq.h)), grid());
        f.stroke(&Path::line(Point::new(sq.x, sq.py(t)), Point::new(sq.x + sq.w, sq.py(t))), grid());
    }
    f.stroke(&Path::line(to(-1.0, -1.0), to(1.0, 1.0)), dashed(faint()));
    if let Some(c) = ceiling {
        for y in [c, -c] {
            f.stroke(&Path::line(to(-1.0, y), to(1.0, y)), dashed(muted()));
        }
    }
    f.stroke(&Path::new(|p| polyline(p, (0..=N).filter_map(|i| {
        let x = i as f32 / N as f32 * 2.0 - 1.0;
        shape(x).map(|y| to(x, y))
    }))), curve());

    // A test sine through the shape, two periods.
    let x0 = sq.x + sq.w + 20.0;
    let (x1, mid, amp) = (plot.x + plot.w, sq.y + sq.h / 2.0, sq.h / 2.0 * 0.92);
    if x1 - x0 >= 40.0 {
        let sine = |u: f32| (u * 2.0 * std::f32::consts::TAU).sin() * 0.8;
        let at = |u: f32, y: f32| Point::new(x0 + u * (x1 - x0), mid - y.clamp(-1.1, 1.1) * amp);
        f.stroke(&Path::line(Point::new(x0, mid), Point::new(x1, mid)), zero());
        f.stroke(&Path::new(|p| polyline(p, (0..=N).map(|i| { let u = i as f32 / N as f32; at(u, sine(u)) }))), dashed(muted()).with_width(1.2));
        f.stroke(&Path::new(|p| polyline(p, (0..=N).filter_map(|i| { let u = i as f32 / N as f32; shape(sine(u)).map(|y| at(u, y)) }))), curve());
    }
    to
}

/// A waveshaper curve with up to this many points shows them as handles.
const MAX_POINTS: usize = 32;

/// The gain reduction meter's range: 0..1 = 0..24 dB.
const GR_RANGE: f32 = 24.0;

/// Input level → output level (−60…0 dB) in a square on the left, a ball where the block works now;
/// on the right, the gain reduction of the last seconds hanging from the top (0…24 dB).
fn dynamics(f: &mut Frame, plot: Plot, kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32, live: Option<(f32, Vec<f32>)>) {
    const FLOOR: f32 = -60.0;
    const N: usize = 160;
    let side = plot.h;
    let sq = Plot { x: plot.x, y: plot.y, w: side, h: side };
    let to = |input: f32, output: f32| Point::new(sq.px(input / -FLOOR + 1.0), sq.py((output / -FLOOR + 1.0).clamp(-0.02, 1.0)));
    let gr_at = |level: f32| preview::dynamics(kind, v, level).unwrap_or(0.0);

    for i in 0..=5 {
        let d = FLOOR + i as f32 * 12.0;
        f.stroke(&Path::line(to(d, FLOOR), to(d, 0.0)), grid());
        f.stroke(&Path::line(to(FLOOR, d), to(0.0, d)), grid());
    }
    f.stroke(&Path::line(to(FLOOR, FLOOR), to(0.0, 0.0)), dashed(faint()));
    f.stroke(&Path::new(|p| polyline(p, (0..=N).map(|i| {
        let input = FLOOR * (1.0 - i as f32 / N as f32);
        to(input, input - gr_at(input))
    }))), curve());

    let Some((gr, past)) = live else { return };
    // Where on the curve the block works now: the level whose static reduction is the live one
    // (attack and release aside). Below the knee there's nothing to show.
    if gr > 0.1 {
        let (mut a, mut b) = (FLOOR, 0.0f32);
        for _ in 0..24 {
            let m = (a + b) / 2.0;
            if gr_at(m) < gr { a = m } else { b = m }
        }
        let level = (a + b) / 2.0;
        if gr_at(0.0) >= gr {
            f.fill(&Path::circle(to(level, level - gr), 3.5), theme::c().frame_ink);
        }
    }

    // The last seconds of gain reduction.
    let x0 = sq.x + sq.w + 18.0;
    let x1 = plot.x + plot.w;
    if x1 - x0 < 40.0 {
        return;
    }
    let gy = |db: f32| sq.y + (db / GR_RANGE).clamp(0.0, 1.0) * sq.h;
    f.stroke(&Path::line(Point::new(x0, sq.y), Point::new(x1, sq.y)), zero());
    f.stroke(&Path::line(Point::new(x0, gy(12.0)), Point::new(x1, gy(12.0))), dashed(faint()));
    label(f, "−12 dB", Point::new(x1, gy(12.0) + 3.0), alignment::Horizontal::Right, faint());
    label(f, "GR", Point::new(x1, sq.y + sq.h - 11.0), alignment::Horizontal::Right, muted());
    if past.len() > 1 {
        let step = (x1 - x0) / (HISTORY - 1) as f32;
        let start = x1 - (past.len() - 1) as f32 * step;
        let points = past.iter().enumerate().map(|(i, g)| Point::new(start + i as f32 * step, gy(*g)));
        let area = Path::new(|p| {
            p.move_to(Point::new(start, sq.y));
            points.clone().for_each(|pt| p.line_to(pt));
            p.line_to(Point::new(x1, sq.y));
            p.close();
        });
        f.fill(&area, theme::alpha(theme::c().frame_ink, 0.07));
        f.stroke(&Path::new(|p| polyline(p, points)), curve().with_width(1.5));
    }
}

/// The dry hit (grey) and the repeats, each as high as it's loud; ping-pong alternates them above
/// (left) and below (right) the line.
fn taps(f: &mut Frame, plot: Plot, kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32) {
    const REPEATS: usize = 8;
    let BlockKind::Delay { feedback, mix, pingpong, .. } = kind else { return };
    let (fb, wet) = ((v(feedback) / 100.0).clamp(0.0, 1.0), (v(mix) / 100.0).clamp(0.0, 1.0));
    let x = |k: usize| plot.px(0.03 + k as f32 / REPEATS as f32 * 0.94);
    // Ping-pong: around the middle, L up and R down; else from the bottom.
    let (base, reach) = if *pingpong { (plot.y + plot.h / 2.0, plot.h / 2.0) } else { (plot.y + plot.h, plot.h) };
    f.stroke(&Path::line(Point::new(plot.x, base), Point::new(plot.x + plot.w, base)), zero());
    if *pingpong {
        label(f, "L", Point::new(plot.x, plot.y), alignment::Horizontal::Left, faint());
        label(f, "R", Point::new(plot.x, plot.y + plot.h - 11.0), alignment::Horizontal::Left, faint());
    }
    let mut stem = |k: usize, level: f32, colour: Color| {
        let down = *pingpong && k % 2 == 0 && k > 0;
        let len = reach * level.clamp(0.0, 1.0);
        let top = Point::new(x(k), if down { base + len } else { base - len });
        f.stroke(&Path::line(Point::new(x(k), base), top), Stroke::default().with_color(colour).with_width(3.0).with_line_cap(canvas::LineCap::Round));
    };
    stem(0, 1.0 - wet, muted());
    for k in 1..=REPEATS {
        let level = wet * fb.powi(k as i32 - 1);
        if level < 0.004 {
            break;
        }
        stem(k, level, theme::c().frame_ink);
    }
}

/// The meter block: a level bar (peak, dBFS) and the level of the last seconds below it. The meter
/// arrives as 0..1 over −60…0 dB.
fn level_history(f: &mut Frame, plot: Plot, level: f32, past: &[f32]) {
    const FLOOR: f32 = -60.0;
    let t = |m: f32| m.clamp(0.0, 1.0);
    let bar = Rectangle { x: plot.x, y: plot.y + 2.0, width: plot.w, height: 8.0 };
    f.fill(&Path::rounded_rectangle(bar.position(), bar.size(), 3.0.into()), theme::alpha(theme::c().frame_ink, 0.06));
    f.fill(&Path::rounded_rectangle(bar.position(), Size::new(bar.width * t(level), bar.height), 3.0.into()), theme::c().frame_ink);
    for d in [-48.0f32, -36.0, -24.0, -12.0, -6.0, 0.0] {
        let x = plot.px((d - FLOOR) / -FLOOR);
        f.stroke(&Path::line(Point::new(x, bar.y + bar.height + 3.0), Point::new(x, bar.y + bar.height + 7.0)), zero());
        label(f, &format!("{d}"), Point::new(x, bar.y + bar.height + 9.0), alignment::Horizontal::Center, faint());
    }
    let h = Plot { x: plot.x, y: plot.y + 42.0, w: plot.w, h: plot.h - 42.0 };
    f.stroke(&Path::line(Point::new(h.x, h.y + h.h), Point::new(h.x + h.w, h.y + h.h)), zero());
    if past.len() > 1 {
        let step = h.w / (HISTORY - 1) as f32;
        let start = h.x + h.w - (past.len() - 1) as f32 * step;
        let points = past.iter().enumerate().map(|(i, l)| Point::new(start + i as f32 * step, h.py(t(*l))));
        let area = Path::new(|p| {
            p.move_to(Point::new(start, h.y + h.h));
            points.clone().for_each(|pt| p.line_to(pt));
            p.line_to(Point::new(h.x + h.w, h.y + h.h));
            p.close();
        });
        f.fill(&area, theme::alpha(theme::c().frame_ink, 0.07));
        f.stroke(&Path::new(|p| polyline(p, points)), curve().with_width(1.4));
    }
}

// ---- fallback card -----------------------------------------------------------------------------

/// Blocks without a display yet: what the block does, in words.
fn card<'a, M: 'a>(description: String, size: Size) -> Element<'a, M> {
    container(column![text(description).size(13).font(theme::font::SANS_MEDIUM).color(theme::c().frame_ink_2)].align_x(Center))
        .width(size.width)
        .center_y(size.height)
        .padding(Padding::from(18))
        .align_x(Center)
        .style(ground)
        .into()
}

fn description(kind: &BlockKind) -> String {
    match kind {
        BlockKind::BandSplit { bands, .. } => format!("Splits into {} frequency bands and sums them again", bands.len()),
        BlockKind::Parallel { branches, .. } => format!("Runs {} branches side by side and mixes them", branches.len()),
        BlockKind::MidSide { .. } => "Processes mid and side separately".into(),
        BlockKind::LeftRight { .. } => "Processes left and right separately".into(),
        BlockKind::Reverb(_) => "Reverberation".into(),
        BlockKind::Shimmer(_) => "Reverberation with pitch-shifted feedback".into(),
        BlockKind::Meter => "Measures the level at this point of the chain".into(),
        _ => chain_view::type_name(kind),
    }
}

// ---- drawing helpers ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Plot {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Plot {
    /// `t` 0 = left, 1 = right.
    fn px(self, t: f32) -> f32 {
        self.x + self.w * t
    }

    /// `t` 0 = bottom, 1 = top.
    fn py(self, t: f32) -> f32 {
        self.y + self.h * (1.0 - t)
    }
}

fn polyline(p: &mut canvas::path::Builder, mut points: impl Iterator<Item = Point>) {
    if let Some(first) = points.next() {
        p.move_to(first);
        points.for_each(|pt| p.line_to(pt));
    }
}

/// What the block does: the theme's ink.
fn curve<'a>() -> Stroke<'a> {
    Stroke::default().with_color(theme::c().frame_ink).with_width(2.0).with_line_join(canvas::LineJoin::Round)
}

fn grid<'a>() -> Stroke<'a> {
    Stroke::default().with_color(theme::alpha(theme::c().frame_ink, 0.06)).with_width(1.0)
}

fn zero<'a>() -> Stroke<'a> {
    Stroke::default().with_color(theme::alpha(theme::c().frame_ink, 0.16)).with_width(1.0)
}

/// References and the input.
fn muted() -> Color {
    theme::alpha(theme::c().frame_ink, 0.5)
}

/// Axis labels, neutral lines.
fn faint() -> Color {
    theme::alpha(theme::c().frame_ink, 0.3)
}

fn dashed<'a>(colour: Color) -> Stroke<'a> {
    Stroke { line_dash: LineDash { segments: &[3.0, 4.0], offset: 0 }, ..Stroke::default().with_color(colour).with_width(1.0) }
}

/// A handle on a curve: a ring in the ink around the display's ground.
fn handle(f: &mut Frame, at: Point) {
    f.fill(&Path::circle(at, 4.0), theme::c().frame_node);
    f.stroke(&Path::circle(at, 4.0), Stroke::default().with_color(theme::c().frame_ink).with_width(1.5));
}

fn handle_small(f: &mut Frame, at: Point) {
    f.fill(&Path::circle(at, 3.0), theme::c().frame_node);
    f.stroke(&Path::circle(at, 3.0), Stroke::default().with_color(theme::c().frame_ink).with_width(1.3));
}

fn label(f: &mut Frame, s: &str, at: Point, align: alignment::Horizontal, colour: Color) {
    f.fill_text(Text {
        content: s.to_string(),
        position: at,
        color: colour,
        size: Pixels(9.0),
        font: theme::font::MONO,
        align_x: align.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });
}

/// The main settings, top left.
fn caption(f: &mut Frame, s: &str) {
    if s.is_empty() {
        return;
    }
    f.fill_text(Text {
        content: s.to_string(),
        position: Point::new(12.0, 9.0),
        color: theme::alpha(theme::c().frame_ink, 0.55),
        size: Pixels(10.5),
        font: theme::font::SANS,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });
}

fn ground(_: &Theme) -> container::Style {
    container::Style {
        background: Some(theme::alpha(theme::c().frame_ink, 0.025).into()),
        border: Border { radius: theme::radius(10.0), width: 1.0, color: theme::c().frame_hairline },
        ..Default::default()
    }
}

/// "250", "0.5": no trailing zeros.
fn trim(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
