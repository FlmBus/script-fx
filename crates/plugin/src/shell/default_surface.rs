//! The **default surface**: what the viewport shows for a patch that declares no surface of its
//! own — its macros, large, in rows of four. A patch surface is mostly a pretty face for the
//! macros, so this is the plain one. (With one of its own, `stage` draws it.)
//!
//! The cells are plain knob cells (`knob::Param`, no macro yellow: here the macros are simply the
//! patch's controls), scaled up; input as everywhere (drag, Shift, wheel, double-click,
//! right-click to type).
//!
//! Styled by the patch's theme, like its library card: `colors.bg` the ground, `colors.accent`
//! the lit ticks and pointers, `colors.ink` the labels (else a light tint of the accent), the
//! title is the patch's logo or its name in `fonts.title`.
//!

use patch::{Patch, Values};

use super::brand::{self, Brand};
use super::{knob, routing, session, theme};
use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, Padding, Theme,
    widget::{Column, Row, column, container, text},
};

#[derive(Debug, Clone)]
pub enum Message {
    /// Input on a macro knob (slot).
    Macro(usize, knob::KnobEvent),
}

impl From<(usize, knob::KnobEvent)> for Message {
    fn from((slot, e): (usize, knob::KnobEvent)) -> Self {
        Message::Macro(slot, e)
    }
}

const PER_ROW: usize = 4;
const GAP: f32 = 12.0;
/// Room above for the notch, a margin below (the dock is outside the stage).
const NOTCH_CLEARANCE: f32 = 56.0;
const BOTTOM_CLEARANCE: f32 = 24.0;

/// `brand`: the patch's title font / logo, as the library loaded them.
/// `drop`: a modulator is being dragged (its colour): the macro knobs take it.
pub fn view<'a>(patch: &'a Patch, brand: Option<&'a Brand>, values: &'a Values, drop: Option<Color>) -> Element<'a, Message> {
    let colour = |name: &str| session::hex(patch.theme.color(name));
    let bg = colour("bg").unwrap_or(theme::c().frame_raised);
    let accent = colour("accent").unwrap_or(theme::c().frame_ink);
    let ink = colour("ink").unwrap_or(theme::mix(accent, Color::WHITE, 0.8));
    let colours = knob::Colours { accent, ink };

    let title: Element<'a, Message> = match brand {
        Some(b) => brand::title(b, &patch.name, accent, 28.0, 360.0),
        None => text(&patch.name).size(18).font(theme::font::SANS_SEMIBOLD).color(accent).into(),
    };

    let body: Element<'a, Message> = if patch.macros.is_empty() {
        text("This patch has no macros. Its knobs are on the blocks (Chain).").size(12.5).color(theme::alpha(ink, 0.6)).into()
    } else {
        // Up to two rows at 1.6×; more (up to 16 macros = four rows) smaller.
        let scale = if patch.macros.len() <= 2 * PER_ROW { 1.6 } else { 1.05 };
        let mut rows = Column::new().spacing(GAP).align_x(Center);
        let slots: Vec<usize> = (0..patch.macros.len()).collect();
        for chunk in slots.chunks(PER_ROW) {
            let cells = chunk.iter().map(|&slot| {
                let v = values.macros[slot];
                knob::param_cell_scaled(
                    knob::Param {
                        index: slot,
                        label: &patch.macros[slot].name,
                        value_text: patch.macro_text(slot, v),
                        value: v,
                        effective: routing::macro_effective(patch, values, slot),
                        default: patch.macro_default(slot),
                        mods: routing::macro_metas(patch, slot).1,
                        drop,
                        modes: None,
                        colours: Some(colours),
                    },
                    scale,
                )
            });
            rows = rows.push(Row::with_children(cells).spacing(GAP));
        }
        rows.into()
    };

    let content = column![title, body].spacing(24).align_x(Center);
    container(content)
        .center(Fill)
        .padding(Padding { top: NOTCH_CLEARANCE, bottom: BOTTOM_CLEARANCE, ..Padding::ZERO })
        .style(move |_: &Theme| container::Style {
            background: Some(bg.into()),
            border: Border { radius: theme::radius(theme::RADIUS_WINDOW), ..Default::default() },
            ..Default::default()
        })
        .into()
}
