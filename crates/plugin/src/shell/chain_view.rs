//! The chain view (notch: Panel | **Chain** | Matrix): the patch's audio tree on a dotted ground.
//!
//! IN → blocks → OUT on one wire that snakes through rows (left to right, down, right to left,
//! down, …) when the chain is wider than the window. **Containers are blocks with slots** (band
//! split: high/mid/low, parallel: dry/wet, mid/side, left/right, a delay's feedback chain): a card
//! with a header (id, type, settings) and one row per slot. There are no split/merge wires; containment
//! says it. Only the main line has wires.
//!
//! A block shows its LED (amber while a compressor reduces, as the signal line did) and small
//! dots in the colours of what moves it: macro yellow, the modulators' colours.
//!
//! Every node and card gets an exact width, measured bottom-up from its text: iced can't stretch
//! a card's slots to the widest one on its own; oversized cards scroll rather than squeeze.
//!
//! Clicking a block (or a container's header bar) opens its modal; both light up under the
//! pointer. The shell zooms the camera into the block: every block records where it is on
//! screen ([`rect`]) while the chain is drawn unmoved. A press anywhere closes what floats (the
//! shell does that for every page).

use std::cell::RefCell;

use nice_plug_iced::iced::advanced::Renderer as _;
use std::collections::HashMap;

use patch::chain::{BlockKind, BlockSpec, ParamRef};
use patch::{Patch, RouteSource, Values};

use super::{routing, theme};
use nice_plug_iced::iced::{
    Background, Border, Center, Color, Element, Event as IcedEvent, Fill, Font, Length, Padding, Pixels, Point, Rectangle, Renderer, Size, Theme,
    Vector, alignment,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
        widget::{self as adv_widget, Tree},
    },
    core::text::{self as core_text, Paragraph as _},
    mouse,
    widget::{Column, Row, canvas, column, container, responsive, row, scrollable, space, stack, text},
};

// ---- sizes -------------------------------------------------------------------------------------

/// The wire between two blocks on the main line, and inside a slot.
const WIRE: f32 = 16.0;
const SLOT_WIRE: f32 = 10.0;
pub const NODE_HEIGHT: f32 = 30.0;
/// IN / OUT: the ring with its label (about as wide as "OUT").
const TERMINAL_WIDTH: f32 = 24.0;
pub const NODE_PAD: (f32, f32) = (11.0, 12.0);
const LED: f32 = 6.0;
const GAP: f32 = 7.0;
const SLOT_LABEL: f32 = 38.0;
/// Room for a slot's trailing wire, and the slot's right padding.
const SLOT_END: f32 = 16.0 + 8.0;
pub const CARD_PAD: f32 = 6.0;
/// Room above the chain for the notch.
const NOTCH_CLEARANCE: f32 = 84.0;
/// Room below the chain for the collapsed drawer, which hangs into the window.
const DRAWER_CLEARANCE: f32 = theme::SIZE_DRAWER_BAR + 16.0;

/// The card's header bar (the container's button).
const CARD_HEADER: f32 = 22.0;
const DOT_GAP: f32 = 4.0;

/// The block name's size in a node (the modal's title grows from it).
pub const NAME: f32 = 12.5;
const TYPE: f32 = 11.5;
const SETTINGS: f32 = 10.5;
const EMPTY: &str = "empty · passes through";

/// What the chain view asks of the shell.
#[derive(Debug, Clone)]
pub enum Event {
    /// Open this block's modal.
    Open(String),
}

/// `hidden`: blocks not drawn (a block modal stands in for them; they still record where they are).
/// `record`: note where every block is ([`rect`]); only while the chain is drawn unmoved (no
/// camera on it), so the rectangles are the ones the user sees and clicks.
/// `ground`: with the editor's ground (off under a block modal, which draws its own).
pub fn view<'a>(patch: &'a Patch, values: &'a Values, record: bool, hidden: [Option<&'a str>; 2], ground: bool) -> Element<'a, Event> {
    // The rows depend on the width: lay out inside `responsive`.
    let content = responsive(move |size| snake(Cx { patch, values, record, hidden }, size.width));
    let ground_layer = container(dots(1.0).height(Fill)).style(|_: &Theme| container::Style {
        background: Some(theme::c().frame_raised.into()),
        border: Border { radius: theme::radius(theme::RADIUS_WINDOW), ..Default::default() },
        ..Default::default()
    });
    let layers = if ground { stack![ground_layer, content] } else { stack![content] };
    Mark { content: layers.width(Fill).height(Fill).into(), kind: MarkKind::Root, record, hidden: false }.into()
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
    let padded = Mark { content: padded.into(), kind: MarkKind::Scrolled, record: cx.record, hidden: false };
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
    record: bool,
    hidden: [Option<&'a str>; 2],
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
        let (dots, w) = self.dots(b);
        if let Some(dots) = dots {
            r = r.push(dots);
            width += GAP + w;
        }
        let width = width.ceil();
        let node = container(r)
            .width(width)
            .height(NODE_HEIGHT)
            .padding(Padding { left: NODE_PAD.0, right: NODE_PAD.1, ..Padding::ZERO })
            .align_y(Center)
            .style(node_style);
        (self.mark(node, &b.id, None), width)
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
        let (dots, w) = self.dots(b);
        if let Some(dots) = dots {
            header = header.push(dots);
            header_width += GAP + w;
        }
        header = header.push(space::horizontal().width(Fill).height(1));
        if let Some(s) = self.settings(&b.kind) {
            header_width += 24.0 + measure(&s, SETTINGS, theme::font::MONO);
            header = header.push(text(s).size(SETTINGS).font(theme::font::MONO).color(theme::c().frame_muted));
        }

        let slots: Vec<_> = slots.into_iter().map(|(label, chain)| self.slot(label, chain)).collect();
        let inner = slots.iter().map(|(_, w)| *w).fold(header_width, f32::max);
        let width = (inner + 2.0 * CARD_PAD + 2.0).ceil();

        // The header bar opens the container's own modal (crossovers, mix); the slots hold its
        // blocks. The whole card is what the camera zooms into.
        let header = container(header).height(CARD_HEADER).padding(Padding { left: 4.0, right: 4.0, ..Padding::ZERO });
        let mut col = Column::new().spacing(4).push(header);
        for (slot, _) in slots {
            col = col.push(slot);
        }
        let card = container(col).width(width).padding(CARD_PAD).style(|_: &Theme| container::Style {
            background: Some(theme::alpha(theme::c().frame_ink, 0.02).into()),
            border: Border { radius: theme::radius(12.0), width: 1.0, color: theme::c().frame_hairline_strong },
            ..Default::default()
        });
        (self.mark(card, &b.id, Some(CARD_HEADER)), width)
    }

    /// Clickable, lights up under the pointer, records where it is; `header`: only the card's
    /// header bar (this tall) is the button.
    fn mark(self, content: impl Into<Element<'a, Event>>, id: &str, header: Option<f32>) -> Element<'a, Event> {
        Mark { content: content.into(), kind: MarkKind::Block { id: id.to_string(), header }, record: self.record, hidden: self.hidden.contains(&Some(id)) }.into()
    }

    /// A dot per source that moves the block (macro yellow, the modulators' colours); with the
    /// row's width.
    fn dots(self, b: &BlockSpec) -> (Option<Element<'a, Event>>, f32) {
        let sources = sources(self.patch, b);
        if sources.is_empty() {
            return (None, 0.0);
        }
        let n = sources.len() as f32;
        let row = Row::with_children(sources.into_iter().map(|s| dot(routing::colour(s), LED))).spacing(DOT_GAP);
        (Some(row.into()), n * LED + (n - 1.0) * DOT_GAP)
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

    fn led(self, b: &BlockSpec) -> Color {
        led_colour(self.patch, self.values, b)
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

/// The block's LED: amber while its meter is away from idle (a compressor reducing), else green.
pub fn led_colour(patch: &Patch, values: &Values, b: &BlockSpec) -> Color {
    let Some(idle) = b.kind.idle_meter() else { return theme::c().led_ok };
    let v = patch.meters.iter().position(|m| *m == b.id).and_then(|i| values.blocks.get(i)).copied();
    match v {
        Some(v) if (v - idle).abs() > 1.0 / 24.0 && !matches!(b.kind, BlockKind::Meter) => theme::c().led_reduce,
        _ => theme::c().led_ok,
    }
}

/// Containers show as cards (the camera zooms into the whole card).
pub fn is_container(kind: &BlockKind) -> bool {
    slots(kind).is_some()
}

/// Where a block's label (LED, id) starts inside it, for a label `height` tall: in a node, after
/// its padding; in a card, in the header bar.
pub fn label_offset(container: bool, height: f32) -> Vector {
    if container {
        Vector::new(1.0 + CARD_PAD + 4.0, 1.0 + CARD_PAD + (CARD_HEADER - height) / 2.0)
    } else {
        Vector::new(NODE_PAD.0, (NODE_HEIGHT - height) / 2.0)
    }
}

/// What moves the block's own knobs, each source once: macros (by slot), then the modulators in
/// their tab order.
pub fn sources(patch: &Patch, b: &BlockSpec) -> Vec<RouteSource> {
    let mut out: Vec<RouteSource> = b.params.iter().flat_map(|r| patch.routes_to_param(r.0)).map(|(_, s, _)| s).collect();
    out.sort_by_key(|s| match s {
        RouteSource::Macro(slot) => *slot,
        RouteSource::Modulator(id) => 1000 + id.index(),
    });
    out.dedup();
    out
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
    out.replace("Fet ", "FET ").replace("Dc ", "DC ").replace("De esser", "De-esser").replace("Vari mu", "Vari-mu")
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

/// The dot grid's spacing; its first dot sits at half of it.
pub const DOT_STEP: f32 = 18.0;

/// The editor's ground: a faint dot grid (`alpha` 1 = as on the chain view). The block modal
/// has it too.
pub fn dots<'a, M: 'a>(alpha: f32) -> canvas::Canvas<Dots, M> {
    canvas(Dots(alpha)).width(Fill).height(Fill)
}

pub struct Dots(f32);

/// The ground's dot grid zooming endlessly: `zoom` octaves (log2) with the grid point `(9, 9)` of the
/// unzoomed view at `anchor`. As the dots spread apart, new ones fade in between them; at a whole
/// number of octaves it is the normal grid again.
pub fn zoom_dots<'a, M: 'a>(anchor: Point, zoom: f32) -> canvas::Canvas<ZoomDots, M> {
    canvas(ZoomDots { anchor, zoom }).width(Fill).height(Fill)
}

pub struct ZoomDots {
    anchor: Point,
    zoom: f32,
}

impl<M> canvas::Program<M> for ZoomDots {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let f = self.zoom - self.zoom.floor();
        let step = DOT_STEP * 2f32.powf(f);
        let colour = theme::alpha(theme::c().frame_ink, 0.07);
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // The grid at `step`, plus the one at half of it fading in (where it adds dots).
        for (st, coarse_too, alpha) in [(step, true, 1.0), (step / 2.0, false, f)] {
            if alpha <= 0.01 {
                continue;
            }
            let c = theme::alpha(colour, colour.a * alpha);
            let (i0, j0) = (((0.0 - self.anchor.x) / st).floor() as i64, ((0.0 - self.anchor.y) / st).floor() as i64);
            let mut j = j0;
            loop {
                let y = self.anchor.y + j as f32 * st;
                if y > bounds.height {
                    break;
                }
                let mut i = i0;
                loop {
                    let x = self.anchor.x + i as f32 * st;
                    if x > bounds.width {
                        break;
                    }
                    if coarse_too || i % 2 != 0 || j % 2 != 0 {
                        frame.fill_rectangle(Point::new(x, y), Size::new(1.2, 1.2), c);
                    }
                    i += 1;
                }
                j += 1;
            }
        }
        vec![frame.into_geometry()]
    }
}

impl<M> canvas::Program<M> for Dots {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        const STEP: f32 = DOT_STEP;
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let colour = theme::alpha(theme::c().frame_ink, 0.07 * self.0);
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

// ---- where blocks are, and their buttons ----------------------------------------------------------

/// Where the blocks were last drawn unmoved (`view(.., record: true)`).
#[derive(Default)]
struct Registry {
    /// The chain view's top-left (window coordinates).
    origin: Point,
    /// How far the content is scrolled.
    scrolled: Vector,
    blocks: HashMap<String, Rectangle>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

/// Where block `id` is on screen, relative to the chain view's top-left (a container: its whole
/// card). `None` until the chain has been drawn with it.
pub fn rect(id: &str) -> Option<Rectangle> {
    REGISTRY.with_borrow(|r| {
        let b = r.blocks.get(id)?;
        Some(Rectangle { x: b.x - r.scrolled.x - r.origin.x, y: b.y - r.scrolled.y - r.origin.y, ..*b })
    })
}

enum MarkKind {
    /// The whole view: its top-left.
    Root,
    /// The scrolled content: how far it's scrolled.
    Scrolled,
    /// A block (or a container's card, `header` = the height of its header bar).
    Block { id: String, header: Option<f32> },
}

/// Wraps a part of the chain view: records where it is, and for blocks is the button.
struct Mark<'a> {
    content: Element<'a, Event>,
    kind: MarkKind,
    record: bool,
    /// Not drawn (it still records where it is).
    hidden: bool,
}

impl Mark<'_> {
    /// The button's area: the node, or the card's header bar.
    fn zone(&self, bounds: Rectangle) -> Option<Rectangle> {
        match &self.kind {
            MarkKind::Block { header: None, .. } => Some(bounds),
            MarkKind::Block { header: Some(h), .. } => {
                let inset = CARD_PAD + 1.0;
                Some(Rectangle { x: bounds.x + inset, y: bounds.y + inset, width: bounds.width - 2.0 * inset, height: *h })
            }
            _ => None,
        }
    }
}

impl Widget<Event, Theme, Renderer> for Mark<'_> {
    fn tag(&self) -> adv_widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> adv_widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn adv_widget::Operation) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &IcedEvent,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Event>,
        viewport: &Rectangle,
    ) {
        // Blocks inside a card take their press first.
        self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
        if let IcedEvent::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && !shell.is_event_captured()
            && let MarkKind::Block { id, .. } = &self.kind
            && self.zone(layout.bounds()).is_some_and(|z| cursor.is_over(z))
        {
            shell.publish(Event::Open(id.clone()));
            shell.capture_event();
        }
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        let inner = self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer);
        if inner == mouse::Interaction::default() && self.zone(layout.bounds()).is_some_and(|z| cursor.is_over(z)) {
            mouse::Interaction::Pointer
        } else {
            inner
        }
    }

    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        let bounds = layout.bounds();
        if self.record {
            REGISTRY.with_borrow_mut(|r| match &self.kind {
                MarkKind::Root => {
                    r.origin = bounds.position();
                    r.blocks.clear();
                }
                // Inside the scrollable, the viewport is the visible part in content coordinates.
                MarkKind::Scrolled => r.scrolled = Vector::new((viewport.x - bounds.x).max(0.0), (viewport.y - bounds.y).max(0.0)),
                MarkKind::Block { id, .. } => {
                    r.blocks.insert(id.clone(), bounds);
                }
            });
        }
        if self.hidden {
            return;
        }
        // A card's header bar lights up behind its text; a node gets a light veil over it.
        let zone = self.zone(bounds).filter(|z| cursor.is_over(*z));
        if let (Some(z), MarkKind::Block { header: Some(_), .. }) = (zone, &self.kind) {
            self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
            renderer.fill_quad(
                renderer::Quad { bounds: z, border: Border { radius: theme::radius(6.0), ..Default::default() }, ..Default::default() },
                Background::Color(theme::alpha(theme::c().frame_ink, 0.06)),
            );
            return;
        }
        self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
        if let Some(z) = zone {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: z,
                    border: Border { radius: theme::radius(theme::RADIUS_BUTTON), width: 1.0, color: theme::alpha(theme::c().frame_ink, 0.22) },
                    ..Default::default()
                },
                Background::Color(theme::alpha(theme::c().frame_ink, 0.04)),
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Event, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation)
    }
}

impl<'a> From<Mark<'a>> for Element<'a, Event> {
    fn from(m: Mark<'a>) -> Self {
        Element::new(m)
    }
}
