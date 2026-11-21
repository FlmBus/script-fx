//! The block page's display: a picture of what the block does, live (it follows the knobs and
//! the macros' modulation).
//!
//! - **Response** (low/high pass, peak, shelves): magnitude over 20 Hz–20 kHz, ±18 dB.
//! - **Transfer** (drive, waveshaper, wavefolder, clipper): input → output over ±1.
//! - **Dynamics** (compressors, limiter): input level → output level, −60…0 dB, plus the live
//!   gain reduction as a bar on the right.
//! - **Taps** (delay): the dry hit and the repeats, each as high as it's loud.
//! - Everything else: a card that says what the block does (designs for those are still open,
//!   `docs/research/chain-editor.md`).
//!
//! The curves come from `dsp::preview`, i.e. the DSP's own formulas.

use dsp::preview;
use patch::chain::{BlockKind, ParamRef};
use patch::{BlockSpec, Patch, Values};

use super::{chain_view, theme};
use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, Padding, Pixels, Point, Rectangle, Renderer, Size, Theme,
    alignment, mouse,
    widget::{
        canvas::{self, Frame, Path, Stroke, Text},
        column, container, text,
    },
};

const WIDTH: f32 = 520.0;
const HEIGHT: f32 = 180.0;

pub fn display<'a, M: 'a>(patch: &'a Patch, values: &'a Values, block: &'a BlockSpec) -> Element<'a, M> {
    let Some(picture) = Picture::of(&block.kind) else {
        return card(description(&block.kind));
    };
    let display = Display { patch, values, block, picture };
    container(canvas::Canvas::new(display).width(Fill).height(HEIGHT)).max_width(WIDTH).style(ground).into()
}

// ---- which picture -----------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Picture {
    Response,
    Transfer,
    Dynamics,
    Taps,
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
    /// A param's current plain value (after modulation).
    fn value(&self, r: &ParamRef) -> f32 {
        let spec = &self.patch.params[r.0];
        spec.plain(self.values.params.get(r.0).copied().unwrap_or_else(|| spec.normalize(spec.default_plain())))
    }

    /// The block's live meter (0..1), if it has one.
    fn meter(&self) -> Option<f32> {
        let i = self.patch.meters.iter().position(|id| *id == self.block.id)?;
        self.values.blocks.get(i).copied()
    }
}

impl<M> canvas::Program<M> for Display<'_> {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let plot = Plot { x: 40.0, y: 14.0, w: bounds.width - 56.0, h: bounds.height - 36.0 };
        let v = |r: &ParamRef| self.value(r);
        match self.picture {
            Picture::Response => response(&mut f, plot, &self.block.kind, &v),
            Picture::Transfer => transfer(&mut f, plot, &self.block.kind, &v),
            Picture::Dynamics => dynamics(&mut f, plot, &self.block.kind, &v, self.meter()),
            Picture::Taps => taps(&mut f, plot, &self.block.kind, &v),
        }
        vec![f.into_geometry()]
    }
}

// ---- pictures ----------------------------------------------------------------------------------

/// Magnitude over 20 Hz–20 kHz (log), ±18 dB, the area between the curve and 0 dB filled.
fn response(f: &mut Frame, plot: Plot, kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32) {
    const RANGE: f32 = 18.0;
    const N: usize = 160;
    let freqs: Vec<f32> = (0..=N).map(|i| 20.0 * 1000f32.powf(i as f32 / N as f32)).collect();
    let Some(db) = preview::response_db(kind, v, &freqs) else { return };

    for (hz, name) in [(100.0f32, "100"), (1000.0, "1k"), (10000.0, "10k")] {
        let x = plot.px((hz / 20.0).log10() / 3.0);
        f.stroke(&Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.h)), grid());
        axis_label(f, name, Point::new(x, plot.y + plot.h + 4.0), alignment::Horizontal::Center);
    }
    for d in [-12.0f32, -6.0, 0.0, 6.0, 12.0] {
        let y = plot.py((d + RANGE) / (2.0 * RANGE));
        f.stroke(&Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.w, y)), if d == 0.0 { zero() } else { grid() });
        axis_label(f, &format!("{d:+.0}"), Point::new(plot.x - 6.0, y - 5.0), alignment::Horizontal::Right);
    }

    let point = |i: usize| Point::new(plot.px(i as f32 / N as f32), plot.py(((db[i] + RANGE) / (2.0 * RANGE)).clamp(-0.02, 1.02)));
    let zero_y = plot.py(0.5);
    let area = Path::new(|p| {
        p.move_to(Point::new(plot.x, zero_y));
        (0..=N).for_each(|i| p.line_to(point(i)));
        p.line_to(Point::new(plot.x + plot.w, zero_y));
        p.close();
    });
    f.fill(&area, theme::alpha(theme::c().frame_ink, 0.06));
    f.stroke(&Path::new(|p| polyline(p, (0..=N).map(point))), curve());
}

/// Input → output over ±1, with the identity as a reference line.
fn transfer(f: &mut Frame, plot: Plot, kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32) {
    const N: usize = 200;
    let sq = plot.square();
    let to = |x: f32, y: f32| Point::new(sq.px((x + 1.0) / 2.0), sq.py(((y + 1.0) / 2.0).clamp(-0.02, 1.02)));

    f.stroke(&Path::rectangle(Point::new(sq.x, sq.y), Size::new(sq.w, sq.h)), grid());
    f.stroke(&Path::line(to(-1.0, 0.0), to(1.0, 0.0)), zero());
    f.stroke(&Path::line(to(0.0, -1.0), to(0.0, 1.0)), zero());
    f.stroke(&Path::line(to(-1.0, -1.0), to(1.0, 1.0)), grid());
    axis_label(f, "in", Point::new(sq.x + sq.w, sq.y + sq.h + 4.0), alignment::Horizontal::Right);
    axis_label(f, "out", Point::new(sq.x - 6.0, sq.y), alignment::Horizontal::Right);

    let points = (0..=N).filter_map(|i| {
        let x = i as f32 / N as f32 * 2.0 - 1.0;
        preview::transfer(kind, v, x).map(|y| to(x, y))
    });
    f.stroke(&Path::new(|p| polyline(p, points)), curve());
}

/// Input level → output level (−60…0 dB) and, right of it, the live gain reduction (0…24 dB).
fn dynamics(f: &mut Frame, plot: Plot, kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32, meter: Option<f32>) {
    const FLOOR: f32 = -60.0;
    const N: usize = 160;
    let sq = plot.square();
    let to = |input: f32, output: f32| Point::new(sq.px(input / -FLOOR + 1.0), sq.py((output / -FLOOR + 1.0).clamp(-0.02, 1.0)));

    f.stroke(&Path::rectangle(Point::new(sq.x, sq.y), Size::new(sq.w, sq.h)), grid());
    for d in [-48.0, -36.0, -24.0, -12.0] {
        f.stroke(&Path::line(to(d, FLOOR), to(d, 0.0)), grid());
        f.stroke(&Path::line(to(FLOOR, d), to(0.0, d)), grid());
    }
    f.stroke(&Path::line(to(FLOOR, FLOOR), to(0.0, 0.0)), zero());
    axis_label(f, "in", Point::new(sq.x + sq.w, sq.y + sq.h + 4.0), alignment::Horizontal::Right);
    axis_label(f, "out", Point::new(sq.x - 6.0, sq.y), alignment::Horizontal::Right);

    let points = (0..=N).filter_map(|i| {
        let input = FLOOR * (1.0 - i as f32 / N as f32);
        preview::dynamics(kind, v, input).map(|gr| to(input, input - gr))
    });
    f.stroke(&Path::new(|p| polyline(p, points)), curve());

    // Gain reduction: a bar hanging from the top, the meter's 0..1 = 0..24 dB.
    if let Some(m) = meter {
        let bar = Rectangle { x: sq.x + sq.w + 24.0, y: sq.y, width: 6.0, height: sq.h };
        f.fill(&Path::rounded_rectangle(bar.position(), bar.size(), 3.0.into()), theme::alpha(theme::c().frame_ink, 0.06));
        let h = bar.height * m.clamp(0.0, 1.0);
        if h > 0.5 {
            f.fill(&Path::rounded_rectangle(bar.position(), Size::new(bar.width, h), 3.0.into()), theme::c().led_reduce);
        }
        axis_label(f, &format!("GR {:.1}", m * 24.0), Point::new(bar.x + bar.width / 2.0, bar.y + bar.height + 4.0), alignment::Horizontal::Center);
    }
}

/// The dry hit at 0 and up to eight repeats, each stem as high as it's loud (mix, feedback).
fn taps(f: &mut Frame, plot: Plot, kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32) {
    const REPEATS: usize = 8;
    let BlockKind::Delay { time, sync, feedback, mix, pingpong, .. } = kind else { return };
    let (fb, wet) = ((v(feedback) / 100.0).clamp(0.0, 1.0), (v(mix) / 100.0).clamp(0.0, 1.0));
    let x = |k: usize| plot.px(k as f32 / REPEATS as f32 * 0.94 + 0.03);
    let base = plot.y + plot.h;

    f.stroke(&Path::line(Point::new(plot.x, base), Point::new(plot.x + plot.w, base)), zero());
    let stem = |f: &mut Frame, k: usize, level: f32, color: Color| {
        let top = Point::new(x(k), plot.py(level.clamp(0.0, 1.0)));
        f.stroke(&Path::line(Point::new(x(k), base), top), Stroke::default().with_color(color).with_width(2.0));
        f.fill(&Path::circle(top, 2.5), color);
    };
    stem(f, 0, 1.0 - wet, theme::alpha(theme::c().frame_ink, 0.35));
    for k in 1..=REPEATS {
        let level = wet * fb.powi(k as i32 - 1);
        if level < 0.004 {
            break;
        }
        stem(f, k, level, theme::c().frame_ink);
        if *pingpong {
            axis_label(f, if k % 2 == 1 { "L" } else { "R" }, Point::new(x(k), base + 4.0), alignment::Horizontal::Center);
        }
    }
    let step = match sync {
        Some(beats) => format!("every {} beat{}", trim(*beats), if *beats == 1.0 { "" } else { "s" }),
        None => format!("every {} ms", trim(v(time))),
    };
    axis_label(f, &step, Point::new(plot.x + plot.w, plot.y), alignment::Horizontal::Right);
}

// ---- fallback card -----------------------------------------------------------------------------

/// Blocks without a picture yet: what the block does, in words.
fn card<'a, M: 'a>(description: String) -> Element<'a, M> {
    container(column![text(description).size(13).font(theme::font::SANS_MEDIUM).color(theme::c().frame_ink_2)].align_x(Center))
        .width(Fill)
        .max_width(WIDTH)
        .center_y(HEIGHT)
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
    x: f32,
    y: f32,
    w: f32,
    h: f32,
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

    /// The largest square, centred horizontally (same scale on both axes).
    fn square(self) -> Self {
        let side = self.w.min(self.h);
        Self { x: self.x + (self.w - side) / 2.0, y: self.y, w: side, h: side }
    }
}

fn polyline(p: &mut canvas::path::Builder, mut points: impl Iterator<Item = Point>) {
    if let Some(first) = points.next() {
        p.move_to(first);
        points.for_each(|pt| p.line_to(pt));
    }
}

fn curve<'a>() -> Stroke<'a> {
    Stroke::default().with_color(theme::c().frame_ink).with_width(1.8).with_line_join(canvas::LineJoin::Round)
}

fn grid<'a>() -> Stroke<'a> {
    Stroke::default().with_color(theme::alpha(theme::c().frame_ink, 0.07)).with_width(1.0)
}

fn zero<'a>() -> Stroke<'a> {
    Stroke::default().with_color(theme::alpha(theme::c().frame_ink, 0.18)).with_width(1.0)
}

fn axis_label(f: &mut Frame, s: &str, at: Point, align: alignment::Horizontal) {
    f.fill_text(Text {
        content: s.to_string(),
        position: at,
        color: theme::c().frame_muted,
        size: Pixels(9.0),
        font: theme::font::MONO,
        align_x: align.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });
}

fn ground(_: &Theme) -> container::Style {
    container::Style {
        background: Some(theme::alpha(theme::c().frame_ink, 0.025).into()),
        border: Border { radius: theme::radius(12.0), width: 1.0, color: theme::c().frame_hairline },
        ..Default::default()
    }
}

/// "250", "0.5": no trailing zeros.
fn trim(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
