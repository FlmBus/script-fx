//! A block's modal (chain view → click a block): the block, grown. One fixed size for every
//! block (`docs/research/mockups/block-modal.html`): a header (LED, id, type, the sources that
//! move it, close), a display of what the block does on the left, its knobs on the right. The
//! display takes the width the knobs leave (8 knobs: 4 × 2, 300 px; 3 knobs: 370 px); more than
//! eight knobs scroll.
//!
//! The knobs are the block's own params in the order its type declares them. All of them turn
//! (saved with the project); each route into one is a meta knob on it, in its source's colour.
//! While a source is dragged, every knob takes it (drop = new route).
//!
//! The shell draws the box, the arrows (‹ › walk the chain) and the transition (`motion`); the
//! title is its own element ([`title`]) so the block's label can grow into it.

use patch::{BlockSpec, Patch, Values};

use super::{block_display, chain_view, icons, knob, routing, theme};
use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding, Point, Size, Theme,
    widget::{Row, button, column, container, row, scrollable, space, text},
};

#[derive(Debug, Clone)]
pub enum Message {
    /// Back to the chain (✕, Escape, a click beside the modal).
    Close,
    /// Walk the chain: the previous / next block.
    Walk(bool),
    /// Input on a knob (param index).
    Knob(usize, knob::KnobEvent),
}

impl From<(usize, knob::KnobEvent)> for Message {
    fn from((i, e): (usize, knob::KnobEvent)) -> Self {
        Message::Knob(i, e)
    }
}

/// Every block's modal.
pub const SIZE: Size = Size::new(626.0, 266.0);
const PAD: f32 = 18.0;
/// The header row (as tall as the close button), from the top.
const HEADER_Y: f32 = 15.0;
const HEADER_H: f32 = 26.0;
/// The display and the knobs, from the top.
const BODY_Y: f32 = 58.0;
const BODY_H: f32 = SIZE.height - BODY_Y - PAD;
const GAP: f32 = 16.0;
const CELL_GAP: f32 = 6.0;

/// The title (LED and id): its size, and where it sits in the modal.
pub const TITLE_SIZE: f32 = 16.0;
pub const TITLE_H: f32 = 20.0;
pub const TITLE_AT: Point = Point::new(PAD, HEADER_Y + (HEADER_H - TITLE_H) / 2.0);
const TITLE_LED: f32 = 8.0;

/// Knob columns for `n` knobs: one row up to four, then two rows (at most four columns).
pub fn knob_columns(n: usize) -> usize {
    match n {
        0..=4 => n,
        _ => n.div_ceil(2).min(4),
    }
}

/// The display's width for a block with `n` knobs.
pub fn display_width(n: usize) -> f32 {
    let c = knob_columns(n);
    let inner = SIZE.width - 2.0 * PAD;
    if c == 0 { inner } else { inner - GAP - knobs_width(c) }
}

fn knobs_width(columns: usize) -> f32 {
    columns as f32 * knob::PARAM_SIZE.width + (columns.saturating_sub(1)) as f32 * CELL_GAP
}

/// The modal's content, exactly [`SIZE`] (the shell draws the box behind it). `title`: draw the
/// title (off while the block's label is still growing into it).
pub fn view<'a>(patch: &'a Patch, values: &'a Values, block: &'a BlockSpec, drag: Option<&routing::SourceDrag>, title: bool) -> Element<'a, Message> {
    let drop = drag.and_then(routing::SourceDrag::param_drop);

    let mut header = Row::new()
        .push(self::title(patch, values, block, if title { 1.0 } else { 0.0 }))
        .push(text(chain_view::type_name(&block.kind)).size(12).color(theme::c().frame_muted))
        .push(space::horizontal().width(Fill))
        .spacing(10)
        .align_y(Center)
        .height(HEADER_H);
    // What moves the block: a chip per source (three, then "+N").
    let sources = chain_view::sources(patch, block);
    for s in sources.iter().take(3) {
        header = header.push(chip(routing::colour(*s), routing::name(patch, *s).to_string()));
    }
    if sources.len() > 3 {
        header = header.push(text(format!("+{}", sources.len() - 3)).size(11).color(theme::c().frame_muted));
    }
    let close = button(container(icons::icon(icons::CLOSE, 10.0, 2.5, theme::c().frame_ink_2)).center(Fill))
        .on_press(Message::Close)
        .width(HEADER_H)
        .height(HEADER_H)
        .padding(0)
        .style(theme::frame_ghost(false));
    header = header.push(close);

    let n = block.params.len();
    let display = block_display::display(patch, values, block, Size::new(display_width(n), BODY_H));
    let mut body = Row::new().push(display).spacing(GAP);
    if n > 0 {
        let cells = block.params.iter().map(move |r| {
            let i = r.0;
            let spec = &patch.params[i];
            // The knob edits its base value; routes modulate around it (the arc shows where to).
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
        let width = knobs_width(knob_columns(n));
        let grid = Row::with_children(cells).spacing(CELL_GAP).wrap().vertical_spacing(CELL_GAP);
        let knobs: Element<'a, Message> = if n > 8 {
            // More than the modal holds (the compressor): the knobs scroll.
            scrollable(container(grid).width(width))
                .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(3).scroller_width(3).margin(1)))
                .style(theme::frame_hscrollbar)
                .width(width + 6.0)
                .height(BODY_H)
                .into()
        } else {
            container(grid).width(width).height(BODY_H).center_y(BODY_H).into()
        };
        body = body.push(knobs);
    }

    container(column![header, space::vertical().height(BODY_Y - HEADER_Y - HEADER_H), body.height(BODY_H)])
        .width(SIZE.width)
        .height(SIZE.height)
        .padding(Padding { top: HEADER_Y, left: PAD, right: PAD, bottom: PAD })
        .into()
}

/// The title: LED and id at [`TITLE_SIZE`], [`TITLE_H`] tall. `alpha` 0 keeps its room
/// but draws nothing.
pub fn title<'a, M: 'a>(patch: &Patch, values: &Values, block: &'a BlockSpec, alpha: f32) -> Element<'a, M> {
    let led = theme::alpha(chain_view::led_colour(patch, values, block), alpha);
    let dot = container(space::horizontal().width(TITLE_LED).height(TITLE_LED)).style(move |_: &Theme| container::Style {
        background: Some(led.into()),
        border: Border { radius: theme::radius(TITLE_LED / 2.0), ..Default::default() },
        ..Default::default()
    });
    let ink = theme::alpha(theme::c().frame_ink, alpha);
    row![dot, text(&block.id).size(TITLE_SIZE).font(theme::font::SANS_SEMIBOLD).color(ink)].spacing(8).align_y(Center).height(TITLE_H).into()
}

/// A source that moves the block: its colour and name.
fn chip<'a>(colour: nice_plug_iced::iced::Color, name: String) -> Element<'a, Message> {
    let dot = container(space::horizontal().width(6).height(6)).style(move |_: &Theme| container::Style {
        background: Some(colour.into()),
        border: Border { radius: theme::radius(3.0), ..Default::default() },
        ..Default::default()
    });
    container(row![dot, text(name).size(10.5).color(theme::c().frame_ink_2)].spacing(5).align_y(Center))
        .padding(Padding { top: 3.0, bottom: 3.0, left: 7.0, right: 8.0 })
        .style(|_: &Theme| container::Style {
            background: Some(theme::alpha(theme::c().frame_ink, 0.06).into()),
            border: Border { radius: theme::radius(theme::RADIUS_PILL), ..Default::default() },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_display_takes_what_the_knobs_leave() {
        assert_eq!(knob_columns(3), 3);
        assert_eq!(knob_columns(5), 3);
        assert_eq!(knob_columns(8), 4);
        assert_eq!(knob_columns(14), 4);
        assert_eq!(display_width(8), 300.0);
        assert_eq!(display_width(3), 370.0);
        assert_eq!(display_width(0), 590.0);
    }
}
