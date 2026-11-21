//! A block's page (chain view → click a block): the block in the colour of its node, a header
//! (‹ Chain, id, type), a display of what the block does, and its knobs.
//!
//! The knobs are the block's own params in the order its type declares them. All of them turn
//! (saved with the project); each route into one is a meta knob on it, in its source's colour.
//! While a source is dragged, every knob takes it (drop = new route). The page keeps its knobs
//! above the drawer (`bottom` grows as the drawer opens).
//! The display: see `block_display`.

use patch::{BlockSpec, Patch, Values};

use super::{block_display, chain_view, icons, knob, routing, theme};
use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding, Theme,
    widget::{Row, button, column, container, row, space, text},
};

#[derive(Debug, Clone)]
pub enum Message {
    /// ‹ Chain: zoom back out.
    Back,
    /// Input on a knob (param index).
    Knob(usize, knob::KnobEvent),
}

impl From<(usize, knob::KnobEvent)> for Message {
    fn from((i, e): (usize, knob::KnobEvent)) -> Self {
        Message::Knob(i, e)
    }
}

/// Room around the page (the notch sits centred; the header is left of it).
const PAD: f32 = 20.0;

pub fn view<'a>(patch: &'a Patch, values: &'a Values, block: &'a BlockSpec, drag: Option<&routing::SourceDrag>, bottom: f32) -> Element<'a, Message> {
    let drop = drag.and_then(routing::SourceDrag::param_drop);
    let back = button(row![icons::icon_rotated(icons::CHEVRON_RIGHT, 10.0, 2.5, theme::c().frame_ink_2, 180.0), text("Chain").size(12)].spacing(4).align_y(Center))
        .on_press(Message::Back)
        .padding(Padding { top: 5.0, bottom: 5.0, left: 8.0, right: 10.0 })
        .style(theme::frame_ghost(true));
    let header = row![
        back,
        space::horizontal().width(6),
        text(&block.id).size(16).font(theme::font::SANS_SEMIBOLD).color(theme::c().frame_ink),
        text(chain_view::type_name(&block.kind)).size(12.5).color(theme::c().frame_muted),
    ]
    .spacing(8)
    .align_y(Center);

    let display = block_display::display(patch, values, block);

    let knobs: Element<'a, Message> = if block.params.is_empty() {
        text("This block has no knobs.").size(12).color(theme::c().frame_muted).into()
    } else {
        let cells = block.params.iter().map(move |r| {
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
        Row::with_children(cells).spacing(6).wrap().vertical_spacing(6).into()
    };

    let body = column![header, space::vertical().height(Fill), container(display).center_x(Fill), space::vertical().height(24), container(knobs).center_x(Fill), space::vertical().height(Fill)]
        .padding(Padding { top: PAD, right: PAD, bottom, left: PAD });

    container(body)
        .width(Fill)
        .height(Fill)
        .style(|_: &Theme| container::Style {
            background: Some(theme::c().frame_node.into()),
            border: Border { radius: theme::radius(theme::RADIUS_WINDOW), ..Default::default() },
            ..Default::default()
        })
        .into()
}
