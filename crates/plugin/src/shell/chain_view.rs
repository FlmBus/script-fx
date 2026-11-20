//! The chain view (notch: Panel | **Chain** | Matrix): the patch's audio tree on a dotted ground.
//!
//! IN → blocks → OUT on one wire that snakes through rows (left to right, down, right to left,
//! down, …) when the chain is wider than the window. **Containers are blocks with slots** (band
//! split: high/mid/low, parallel: dry/wet, mid/side, left/right, a delay's feedback chain): a card
//! with a header (id, type, settings) and one row per slot. There are no split/merge wires; containment
//! says it. Only the main line has wires.
//!
//! A block shows its LED (amber while a compressor reduces, as the signal line did) and small
//! dots in the colours of what moves it: yellow for a macro (modulators come later).
//!
//! Every node and card gets an exact width, measured bottom-up from its text: iced can't stretch
//! a card's slots to the widest one on its own; oversized cards scroll rather than squeeze.
//!
//! Clicking a block (or a container's header) opens its page; the shell zooms into the point of
//! the click (`Event::Pointer` keeps it). A press anywhere closes what floats (the shell does that
//! for every page).

use patch::chain::{BlockKind, BlockSpec, ParamRef};
use patch::{Patch, Values};

use super::{knob, theme};
use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, Font, Length, Padding, Pixels, Point, Rectangle, Renderer, Size, Theme,
    alignment,
    core::text::{self as core_text, Paragraph as _},
    mouse,
    widget::{Column, Row, canvas, column, container, mouse_area, responsive, row, scrollable, space, stack, text},
};

// ---- sizes -------------------------------------------------------------------------------------

/// The wire between two blocks on the main line, and inside a slot.
const WIRE: f32 = 16.0;
const SLOT_WIRE: f32 = 10.0;
const NODE_HEIGHT: f32 = 30.0;
/// IN / OUT: the ring with its label (about as wide as "OUT").
const TERMINAL_WIDTH: f32 = 24.0;
const NODE_PAD: (f32, f32) = (11.0, 12.0);
const LED: f32 = 6.0;
const GAP: f32 = 7.0;
const SLOT_LABEL: f32 = 38.0;
/// Room for a slot's trailing wire, and the slot's right padding.
const SLOT_END: f32 = 16.0 + 8.0;
const CARD_PAD: f32 = 6.0;
/// Room above the chain for the notch.
const NOTCH_CLEARANCE: f32 = 84.0;
/// Room below the chain for the collapsed drawer, which hangs into the window.
const DRAWER_CLEARANCE: f32 = theme::SIZE_DRAWER_BAR + 16.0;

const NAME: f32 = 12.5;
const TYPE: f32 = 11.5;
const SETTINGS: f32 = 10.5;
const EMPTY: &str = "empty · passes through";

/// What the chain view asks of the shell.
#[derive(Debug, Clone)]
pub enum Event {
    /// Open this block's page.
    Open(String),
    /// The pointer moved, in the view's coordinates (where a zoom would start).
    Pointer(Point),
}

pub fn view<'a>(patch: &'a Patch, values: &'a Values) -> Element<'a, Event> {
    // The rows depend on the width: lay out inside `responsive`.
    let content = responsive(move |size| snake(Cx { patch, values }, size.width));
    let ground = container(canvas(Dots).width(Fill).height(Fill)).style(|_: &Theme| container::Style {
        background: Some(theme::c().frame_raised.into()),
        border: Border { radius: theme::radius(theme::RADIUS_WINDOW), ..Default::default() },
        ..Default::default()
    });
    mouse_area(stack![ground, content].width(Fill).height(Fill)).on_move(Event::Pointer).into()
}

// ---- the snake ---------------------------------------------------------------------------------
//
// IN → blocks → OUT, wrapped into rows that alternate direction: left to right, down, right to
// left, down, … Every row is as wide as the widest one. A row starts right at the turn it comes
// from (one normal wire); the spare width goes to the side it leaves by, as a longer wire into
// the next turn (or empty, after OUT). Rows are as tall as their tallest card; the turns fill
// that height, so the wire meets each row at its middle.

/// The turn column at either end of a row, and the vertical gap between rows.
const TURN: f32 = 12.0;
const ROW_GAP: f32 = 22.0;
/// Horizontal room around the snake.
const SIDE_PAD: f32 = 24.0;

fn snake<'a>(cx: Cx<'a>, width: f32) -> Element<'a, Event> {
    let mut items: Vec<(Element<'a, Event>, f32)> = vec![(terminal("IN"), TERMINAL_WIDTH)];
    items.extend(cx.patch.chain.iter().map(|b| cx.block(b)));
    items.push((terminal("OUT"), TERMINAL_WIDTH));
    let widths: Vec<f32> = items.iter().map(|(_, w)| *w).collect();

    // Per row: the turn columns plus the entry wire and at least one exit wire.
    let frame = 2.0 * TURN + 2.0 * WIRE;
    let ranges = wrap_rows(&widths, (width - 2.0 * SIDE_PAD - frame).max(1.0));
    let row_width = ranges.iter().map(|&(a, b)| row_width(&widths[a..b])).fold(0.0, f32::max) + frame;

    let count = ranges.len();
    let mut items = items.into_iter();
    let mut rows = Column::new().width(row_width);
    for (i, (a, b)) in ranges.into_iter().enumerate() {
        let mut row_items: Vec<_> = items.by_ref().take(b - a).map(|(e, _)| e).collect();
        let rtl = i % 2 == 1;
        if rtl {
            row_items.reverse();
        }
        let from_above = i > 0;
        let down = i + 1 < count;
        // Entry side: a normal wire from the turn (the first row has no turn, just room).
        let entry: Element<'a, Event> = if from_above { wire(WIRE) } else { space::horizontal().width(WIRE).into() };
        // Exit side: the spare width, as wire into the next turn or empty after OUT.
        let exit: Element<'a, Event> = if down { wire(Fill) } else { space::horizontal().width(Fill).into() };
        let mut middle = Row::new().align_y(Center);
        for (n, item) in row_items.into_iter().enumerate() {
            if n > 0 {
                middle = middle.push(wire(WIRE));
            }
            middle = middle.push(item);
        }
        let (left, right) = if rtl {
            (turn(Side::Left, false, down), turn(Side::Right, from_above, false))
        } else {
            (turn(Side::Left, from_above, false), turn(Side::Right, false, down))
        };
        let row = if rtl { row![left, exit, middle, entry, right] } else { row![left, entry, middle, exit, right] };
        rows = rows.push(row.align_y(Center));
        if down {
            // The way down: on the right after a left-to-right row, else on the left.
            let side = if rtl { Side::Left } else { Side::Right };
            let gap = canvas(Turn { side, up: true, down: true, through: true }).width(TURN).height(ROW_GAP);
            rows = rows.push(match side {
                Side::Left => row![gap, space::horizontal()],
                Side::Right => row![space::horizontal(), gap],
            });
        }
    }

    // Centred while it fits; a card wider than the window scrolls.
    let padded = container(rows).padding(Padding { left: SIDE_PAD, right: SIDE_PAD, top: NOTCH_CLEARANCE, bottom: DRAWER_CLEARANCE });
    let scroll = scrollable(padded)
        .direction(scrollable::Direction::Both { vertical: thin(), horizontal: thin() })
        .style(theme::frame_hscrollbar);
    container(scroll).center(Fill).into()
}

fn thin() -> scrollable::Scrollbar {
    scrollable::Scrollbar::new().width(4).scroller_width(4).margin(2)
}

fn row_width(widths: &[f32]) -> f32 {
    widths.iter().sum::<f32>() + WIRE * widths.len().saturating_sub(1) as f32
}

/// Splits the items into rows of at most `available` width, in order. A card wider than that
/// gets a row of its own (and scrolls).
fn wrap_rows(widths: &[f32], available: f32) -> Vec<(usize, usize)> {
    let mut rows = Vec::new();
    let mut start = 0;
    let mut used = 0.0;
    for (i, &width) in widths.iter().enumerate() {
        if i > start && used + WIRE + width > available {
            rows.push((start, i));
            start = i;
            used = 0.0;
        }
        if i > start {
            used += WIRE;
        }
        used += width;
    }
    if start < widths.len() {
        rows.push((start, widths.len()));
    }
    rows
}

#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

/// A turn column at the `side` end of a row (fills the row's height).
fn turn<'a>(side: Side, up: bool, down: bool) -> Element<'a, Event> {
    canvas(Turn { side, up, down, through: false }).width(TURN).height(Fill).into()
}

/// The wire in a turn column: from the row's middle (coming from the inside of the row) up
/// and/or down, with a rounded corner; `through` = a straight vertical (the gap between rows).
struct Turn {
    side: Side,
    up: bool,
    down: bool,
    through: bool,
}

impl<M> canvas::Program<M> for Turn {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (x, mid, h) = (bounds.width / 2.0, bounds.height / 2.0, bounds.height);
        // The inner edge (towards the row) and the corner's radius, signed towards it.
        let (inner, r) = match self.side {
            Side::Left => (bounds.width, x),
            Side::Right => (0.0, -x),
        };
        let path = canvas::Path::new(|p| {
            if self.through {
                p.move_to(Point::new(x, 0.0));
                p.line_to(Point::new(x, h));
                return;
            }
            if self.up {
                p.move_to(Point::new(x, 0.0));
                p.line_to(Point::new(x, mid - r.abs()));
                p.quadratic_curve_to(Point::new(x, mid), Point::new(x + r, mid));
                p.line_to(Point::new(inner, mid));
            }
            if self.down {
                p.move_to(Point::new(inner, mid));
                p.line_to(Point::new(x + r, mid));
                p.quadratic_curve_to(Point::new(x, mid), Point::new(x, mid + r.abs()));
                p.line_to(Point::new(x, h));
            }
        });
        frame.stroke(&path, canvas::Stroke::default().with_color(theme::c().frame_wire).with_width(1.0));
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::wrap_rows;

    #[test]
    fn wraps_in_order_without_splitting_oversized_items() {
        assert_eq!(wrap_rows(&[], 50.0), Vec::<(usize, usize)>::new());
        assert_eq!(wrap_rows(&[20.0, 20.0, 100.0, 20.0], 56.0), vec![(0, 2), (2, 3), (3, 4)]);
        assert_eq!(wrap_rows(&[20.0, 20.0], 55.0), vec![(0, 1), (1, 2)]);
    }
}

/// What every node needs: the patch (params, macros, meters) and the live values (LEDs).
#[derive(Clone, Copy)]
struct Cx<'a> {
    patch: &'a Patch,
    values: &'a Values,
}

impl<'a> Cx<'a> {
    /// A plain block as a node, a container as a card with its slots; with its width.
    fn block(self, b: &'a BlockSpec) -> (Element<'a, Event>, f32) {
        match slots(&b.kind) {
            Some(slots) => self.card(b, slots),
            None => self.node(b),
        }
    }

    /// LED, id, the dots of what moves it.
    fn node(self, b: &'a BlockSpec) -> (Element<'a, Event>, f32) {
        let mut r = row![led(self.led(b)), text(&b.id).size(NAME).font(theme::font::SANS_MEDIUM).color(theme::c().frame_ink)].spacing(GAP).align_y(Center);
        let mut width = NODE_PAD.0 + LED + GAP + measure(&b.id, NAME, theme::font::SANS_MEDIUM) + NODE_PAD.1;
        if self.moved_by_macro(b) {
            r = r.push(dot(knob::MACRO, LED));
            width += GAP + LED;
        }
        let width = width.ceil();
        let node = container(r)
            .width(width)
            .height(NODE_HEIGHT)
            .padding(Padding { left: NODE_PAD.0, right: NODE_PAD.1, ..Padding::ZERO })
            .align_y(Center)
            .style(node_style);
        (open(node, &b.id), width)
    }

    /// Header (LED, id, type, settings), then one row per slot; as wide as its widest part.
    fn card(self, b: &'a BlockSpec, slots: Vec<(String, &'a [BlockSpec])>) -> (Element<'a, Event>, f32) {
        let kind = type_name(&b.kind);
        let mut header_width = 4.0 + LED + GAP + measure(&b.id, NAME, theme::font::SANS_SEMIBOLD) + GAP + measure(&kind, TYPE, theme::font::SANS) + 4.0;
        let mut header = row![
            led(self.led(b)),
            text(&b.id).size(NAME).font(theme::font::SANS_SEMIBOLD).color(theme::c().frame_ink),
            text(kind).size(TYPE).color(theme::c().frame_muted),
        ]
        .spacing(GAP)
        .align_y(Center);
        if self.moved_by_macro(b) {
            header = header.push(dot(knob::MACRO, LED));
            header_width += GAP + LED;
        }
        header = header.push(space::horizontal().width(Fill).height(1));
        if let Some(s) = self.settings(&b.kind) {
            header_width += 24.0 + measure(&s, SETTINGS, theme::font::MONO);
            header = header.push(text(s).size(SETTINGS).font(theme::font::MONO).color(theme::c().frame_muted));
        }

        let slots: Vec<_> = slots.into_iter().map(|(label, chain)| self.slot(label, chain)).collect();
        let inner = slots.iter().map(|(_, w)| *w).fold(header_width, f32::max);
        let width = (inner + 2.0 * CARD_PAD + 2.0).ceil();

        // The header opens the container's own page (crossovers, mix); the slots hold its blocks.
        let header = container(header).height(22).padding(Padding { left: 4.0, right: 4.0, ..Padding::ZERO });
        let mut col = Column::new().spacing(4).push(open(header, &b.id));
        for (slot, _) in slots {
            col = col.push(slot);
        }
        let card = container(col).width(width).padding(CARD_PAD).style(|_: &Theme| container::Style {
            background: Some(theme::alpha(theme::c().frame_ink, 0.02).into()),
            border: Border { radius: theme::radius(12.0), width: 1.0, color: theme::c().frame_hairline_strong },
            ..Default::default()
        });
        (card.into(), width)
    }

    /// A slot: its label, its blocks on a wire that runs to the card's edge; with its least width.
    fn slot(self, label: String, chain: &'a [BlockSpec]) -> (Element<'a, Event>, f32) {
        let mut r = Row::new()
            .align_y(Center)
            .push(container(text(label).size(11).color(theme::c().frame_muted)).width(SLOT_LABEL).padding(Padding { left: 8.0, ..Padding::ZERO }));
        let mut width = SLOT_LABEL + SLOT_END;
        if chain.is_empty() {
            let empty = container(text(EMPTY).size(10.5).color(theme::c().frame_subtle)).padding(Padding { left: 6.0, right: 6.0, ..Padding::ZERO });
            r = r.push(wire(Fill)).push(empty).push(wire(Fill));
            width += 12.0 + measure(EMPTY, 10.5, theme::font::SANS) + 2.0 * SLOT_WIRE;
        } else {
            for b in chain {
                let (node, w) = self.block(b);
                r = r.push(wire(SLOT_WIRE)).push(node);
                width += SLOT_WIRE + w;
            }
            r = r.push(wire(Fill));
        }
        let slot = container(r.width(Fill)).width(Fill).padding(Padding { top: 4.0, bottom: 4.0, right: 8.0, ..Padding::ZERO }).style(|_: &Theme| {
            container::Style {
                background: Some(theme::alpha(theme::c().frame_ink, 0.025).into()),
                border: Border { radius: theme::radius(8.0), ..Default::default() },
                ..Default::default()
            }
        });
        (slot.into(), width)
    }

    /// Amber while the block's meter is away from idle (a compressor reducing), else green.
    fn led(self, b: &BlockSpec) -> Color {
        let Some(idle) = b.kind.idle_meter() else { return theme::c().led_ok };
        let v = self.patch.meters.iter().position(|m| *m == b.id).and_then(|i| self.values.blocks.get(i)).copied();
        match v {
            Some(v) if (v - idle).abs() > 1.0 / 24.0 && !matches!(b.kind, BlockKind::Meter) => theme::c().led_reduce,
            _ => theme::c().led_ok,
        }
    }

    /// Does one of the block's own knobs follow a macro?
    fn moved_by_macro(self, b: &BlockSpec) -> bool {
        b.params.iter().any(|r| self.patch.is_modulated(r.0))
    }

    /// A container's settings for its header: crossovers ("200 Hz · 3 kHz"), mix.
    fn settings(self, kind: &BlockKind) -> Option<String> {
        match kind {
            BlockKind::BandSplit { crossovers, .. } => Some(crossovers.iter().map(|r| hz(self.plain(*r))).collect::<Vec<_>>().join(" · ")),
            BlockKind::Parallel { mix: Some(m), .. } => Some(format!("mix {}", self.show(*m))),
            _ => None,
        }
    }

    /// A knob's current value in plain units.
    fn plain(self, r: ParamRef) -> f32 {
        self.patch.params[r.0].plain(self.values.params.get(r.0).copied().unwrap_or(0.0))
    }

    /// A knob's current value as its param displays it.
    fn show(self, r: ParamRef) -> String {
        self.patch.params[r.0].display(self.values.params.get(r.0).copied().unwrap_or(0.0))
    }
}

/// A container's slots, top to bottom, with their labels. `None` for plain blocks.
fn slots(kind: &BlockKind) -> Option<Vec<(String, &[BlockSpec])>> {
    fn s(c: &patch::chain::Chain) -> &[BlockSpec] {
        c.0.as_slice()
    }
    Some(match kind {
        // High on top, like a spectrum.
        BlockKind::BandSplit { bands, .. } => {
            let names: &[&str] = match bands.len() {
                2 => &["low", "high"],
                3 => &["low", "mid", "high"],
                _ => &[],
            };
            let mut out: Vec<_> = bands.iter().enumerate().map(|(i, c)| (names.get(i).map_or(format!("{}", i + 1), |n| n.to_string()), s(c))).collect();
            out.reverse();
            out
        }
        // The empty branch is the dry signal.
        BlockKind::Parallel { branches, .. } => {
            let wet = branches.iter().filter(|c| !c.0.is_empty()).count();
            let mut n = 0;
            branches
                .iter()
                .map(|c| {
                    let label = if c.0.is_empty() {
                        "dry".to_string()
                    } else {
                        n += 1;
                        if wet == 1 { "wet".to_string() } else { format!("wet {n}") }
                    };
                    (label, s(c))
                })
                .collect()
        }
        BlockKind::MidSide { mid, side } => vec![("mid".into(), s(mid)), ("side".into(), s(side))],
        BlockKind::LeftRight { left, right } => vec![("left".into(), s(left)), ("right".into(), s(right))],
        BlockKind::Delay { feedback_chain, .. } if !feedback_chain.0.is_empty() => vec![("fb".into(), s(feedback_chain))],
        _ => return None,
    })
}

/// "Band split", "FET compressor": the kind's variant name in words.
pub fn type_name(kind: &BlockKind) -> String {
    let debug = format!("{kind:?}");
    let variant = debug.split(|c: char| !c.is_alphanumeric()).next().unwrap_or_default();
    let mut out = String::new();
    for (i, ch) in variant.chars().enumerate() {
        if i > 0 && ch.is_uppercase() {
            out.push(' ');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out.replace("Fet ", "FET ").replace("Dc ", "DC ").replace("De esser", "De-esser")
}

fn hz(v: f32) -> String {
    if v >= 1000.0 { format!("{} kHz", trim(v / 1000.0)) } else { format!("{} Hz", trim(v)) }
}

fn trim(v: f32) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

/// The width `s` takes on one line (logical px), measured with the shell's fonts.
fn measure(s: &str, size: f32, font: Font) -> f32 {
    iced_graphics::text::Paragraph::with_text(core_text::Text {
        content: s,
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: core_text::LineHeight::default(),
        font,
        align_x: core_text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping: core_text::Shaping::Advanced,
        wrapping: core_text::Wrapping::None,
    })
    .min_bounds()
    .width
}

// ---- pieces ------------------------------------------------------------------------------------

/// Clickable: opens the block's page.
fn open<'a>(content: impl Into<Element<'a, Event>>, id: &str) -> Element<'a, Event> {
    mouse_area(content).on_press(Event::Open(id.to_string())).interaction(mouse::Interaction::Pointer).into()
}

fn node_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(theme::c().frame_node.into()),
        border: Border { radius: theme::radius(theme::RADIUS_BUTTON), width: 1.0, color: theme::c().frame_hairline_strong },
        ..Default::default()
    }
}

fn wire<'a>(width: impl Into<Length>) -> Element<'a, Event> {
    container(space::horizontal().width(width).height(1)).style(theme::rule(theme::c().frame_wire)).into()
}

fn dot<'a>(c: Color, size: f32) -> Element<'a, Event> {
    container(space::horizontal().width(size).height(size))
        .style(move |_: &Theme| container::Style {
            background: Some(c.into()),
            border: Border { radius: theme::radius(size / 2.0), ..Default::default() },
            ..Default::default()
        })
        .into()
}

fn led<'a>(c: Color) -> Element<'a, Event> {
    dot(c, LED)
}

/// IN / OUT: a caps label over a ring that sits on the wire.
fn terminal<'a>(label: &'a str) -> Element<'a, Event> {
    let ring = container(space::horizontal().width(10).height(10)).style(|_: &Theme| container::Style {
        border: Border { radius: theme::radius(5.0), width: 1.5, color: theme::c().frame_muted },
        ..Default::default()
    });
    // The label above is balanced by the same height below, so the ring is on the wire.
    column![
        container(text(label).size(10).font(theme::font::SANS_SEMIBOLD).color(theme::c().frame_muted)).height(18),
        ring,
        space::vertical().height(18),
    ]
    .align_x(Center)
    .into()
}

/// The ground: a faint dot grid.
struct Dots;

impl<M> canvas::Program<M> for Dots {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        const STEP: f32 = 18.0;
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let colour = theme::alpha(theme::c().frame_ink, 0.07);
        let mut y = STEP / 2.0;
        while y < bounds.height {
            let mut x = STEP / 2.0;
            while x < bounds.width {
                f.fill_rectangle(Point::new(x, y), Size::new(1.2, 1.2), colour);
                x += STEP;
            }
            y += STEP;
        }
        vec![f.into_geometry()]
    }
}
