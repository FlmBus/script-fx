//! A block's modal (chain view → click a block): one fixed size for every block. The block grows
//! into it (`motion`), so the card itself is drawn by the shell; this is what's on it: a header
//! (LED, id, type, what modulates it, ✕), a display of what the block does, and its knobs.
//!
//! The knobs are the block's own params in the order its type declares them. All of them turn
//! (saved with the project); each route into one is a meta knob on it, in its source's colour.
//! While a source is dragged, every knob takes it (drop = new route). Up to 4 × 2 knobs fit;
//! blocks with more scroll their knob column until they get knob groups.
//! The display takes the width the knobs leave. See `block_display`.

use patch::{BlockSpec, Patch, Values};

use super::{block_display, chain_view, icons, knob, routing, theme};
use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, Padding, Size, Theme,
    widget::{Column, Row, button, column, container, row, scrollable, space, text},
};

#[derive(Debug, Clone)]
pub enum Message {
    /// ✕: zoom back out.
    Close,
    /// A press on the blurred chain around the modal.
    Backdrop,
    /// ‹ › outside the modal: the previous / next block of the chain.
    Walk(bool),
    /// Input on a knob (param index).
    Knob(usize, knob::KnobEvent),
}

impl From<(usize, knob::KnobEvent)> for Message {
    fn from((i, e): (usize, knob::KnobEvent)) -> Self {
        Message::Knob(i, e)
    }
}

/// The modal, for every block: fits a 4 × 2 knob grid next to a 300 px display.
pub const SIZE: Size = Size::new(626.0, 266.0);
pub const PAD: f32 = 18.0;
/// The header row's height (the block's label lands in it).
pub const HEADER: f32 = 26.0;
const BODY: f32 = 190.0;
const GAP: f32 = 16.0;
const KNOB_GAP: f32 = 6.0;

/// How many knob columns `n` knobs get.
fn columns(n: usize) -> usize {
    if n <= 4 { n } else { n.div_ceil(2).min(4) }
}

/// `show_title`: off while the block's label is flying into the header.
pub fn view<'a>(patch: &'a Patch, values: &'a Values, block: &'a BlockSpec, drag: Option<&routing::SourceDrag>, show_title: bool) -> Element<'a, Message> {
    let drop = drag.and_then(routing::SourceDrag::param_drop);
    let ink = if show_title { theme::c().frame_ink } else { Color::TRANSPARENT };
    let led = if show_title { theme::c().led_ok } else { Color::TRANSPARENT };

    let mut header = Row::new()
        .push(dot(led, 6.0))
        .push(text(&block.id).size(16).font(theme::font::SANS_SEMIBOLD).color(ink))
        .push(text(chain_view::type_name(&block.kind)).size(12.5).color(theme::c().frame_muted))
        .push(space::horizontal().width(Fill))
        .spacing(7)
        .height(HEADER)
        .align_y(Center);
    for source in routing::sources_of(patch, &block.params) {
        header = header.push(chip(routing::name(patch, source), routing::colour(source)));
    }
    let close = button(container(icons::icon(icons::CLOSE, 11.0, 2.5, theme::c().frame_ink_2)).center(Fill))
        .on_press(Message::Close)
        .width(HEADER)
        .height(HEADER)
        .padding(0)
        .style(theme::frame_ghost(false));
    let header = header.push(close);

    let n = block.params.len();
    let cols = columns(n).max(1);
    let rows = n.div_ceil(cols);
    let knobs_w = cols as f32 * knob::PARAM_SIZE.width + (cols - 1) as f32 * KNOB_GAP;
    let display_w = if n == 0 { SIZE.width - 2.0 * PAD } else { SIZE.width - 2.0 * PAD - GAP - knobs_w };
    let display = block_display::display(patch, values, block, display_w, BODY);

    let mut grid = Column::new().spacing(KNOB_GAP);
    for chunk in block.params.chunks(cols) {
        let cells = chunk.iter().map(|r| {
            let i = r.0;
            let spec = &patch.params[i];
            // The knob edits its base value; macros modulate around it (the arc shows where to).
            let v = values.base(i);
            knob::param_cell(knob::Param {
                index: i,
                label: &spec.name,
                value_text: spec.display(v),
                value: v,
                effective: values.params.get(i).copied().unwrap_or(v),
                default: spec.normalize(spec.default_plain()),
                mods: routing::metas(patch, i).1,
                drop,
                modes: None,
                colours: None,
            })
        });
        grid = grid.push(Row::with_children(cells).spacing(KNOB_GAP));
    }
    let knobs: Element<'a, Message> = if rows > 2 {
        let bar = scrollable::Scrollbar::new().width(3).scroller_width(3).margin(0);
        scrollable(grid).direction(scrollable::Direction::Vertical(bar)).width(knobs_w).height(BODY).style(theme::frame_hscrollbar).into()
    } else {
        container(grid).width(knobs_w).height(BODY).center_y(BODY).into()
    };

    let body: Element<'a, Message> = if n == 0 { display } else { row![display, knobs].spacing(GAP).into() };
    container(column![header, body].spacing(14))
        .width(SIZE.width)
        .height(SIZE.height)
        .padding(Padding::from(PAD))
        .into()
}

fn dot<'a>(c: Color, size: f32) -> Element<'a, Message> {
    container(space::horizontal().width(size).height(size))
        .style(move |_: &Theme| container::Style {
            background: Some(c.into()),
            border: Border { radius: theme::radius(size / 2.0), ..Default::default() },
            ..Default::default()
        })
        .into()
}

/// "● LFO 3": what modulates this block.
fn chip<'a>(name: &'a str, c: Color) -> Element<'a, Message> {
    container(row![dot(c, 6.0), text(name).size(10.5).color(theme::c().frame_ink_2)].spacing(5).align_y(Center))
        .padding(Padding { top: 3.0, bottom: 3.0, left: 7.0, right: 8.0 })
        .style(|_: &Theme| container::Style {
            background: Some(theme::alpha(theme::c().frame_ink, 0.06).into()),
            border: Border { radius: theme::radius(theme::RADIUS_PILL), ..Default::default() },
            ..Default::default()
        })
        .into()
}
