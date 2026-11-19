//! Typing a value: right-click a knob, a small paper card opens next to it with the current
//! value selected. Enter applies (typed in the control's own unit, e.g. "6 dB", or percent for
//! a macro driving several params), Escape or a click elsewhere cancels. The shell parses and
//! applies; this section only holds the text.

use nice_plug_iced::iced::{
    Border, Element, Padding, Point, Shadow, Theme, Vector,
    mouse,
    widget::{self, column, container, mouse_area, row, space, text, text_input},
};

use super::{stage::Target, theme};

/// The card's size (logical px), for placing it inside the stage.
pub const SIZE: (f32, f32) = (200.0, 86.0);

pub struct Entry {
    pub target: Target,
    /// Where the right-click happened, in panel coordinates.
    pub at: Point,
    pub label: String,
    pub text: String,
    /// The last Enter didn't parse.
    pub invalid: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Edit(String),
    Submit,
}

pub fn input_id() -> widget::Id {
    widget::Id::new("value-entry")
}

impl Entry {
    pub fn view(&self) -> Element<'_, Message> {
        let c = theme::c();
        let hint = if self.invalid { "type a number, e.g. 6 or 6 dB" } else { "enter to apply · esc to cancel" };
        let card = container(
            column![
                row![text(&self.label).size(12.5).font(theme::font::PAPER_SEMIBOLD).color(c.paper_ink), space::horizontal()],
                text_input("", &self.text)
                    .id(input_id())
                    .on_input(Message::Edit)
                    .on_submit(Message::Submit)
                    .size(12.5)
                    .font(theme::font::PAPER_MONO)
                    .padding(Padding { top: 6.0, bottom: 6.0, left: 9.0, right: 9.0 })
                    .style(move |_: &Theme, status| {
                        let focused = matches!(status, text_input::Status::Focused { .. });
                        text_input::Style {
                            background: c.paper_raised.into(),
                            border: Border {
                                radius: theme::radius(theme::RADIUS_BUTTON),
                                width: 1.0,
                                color: if self.invalid { c.led_clip } else if focused { c.paper_muted } else { c.paper_line },
                            },
                            icon: c.paper_muted,
                            placeholder: c.paper_muted,
                            value: c.paper_ink,
                            selection: theme::alpha(c.signal, 0.3),
                        }
                    }),
                text(hint).size(11).font(theme::font::PAPER).color(if self.invalid { c.led_clip } else { c.paper_muted }),
            ]
            .spacing(6),
        )
        .width(SIZE.0)
        .padding(10)
        .style(|_: &Theme| container::Style {
            background: Some(theme::c().paper.into()),
            border: Border { radius: theme::radius(theme::RADIUS_FIELD), width: 1.0, color: theme::c().paper_edge },
            shadow: Shadow { color: theme::shadow(0.45), offset: Vector::new(0.0, 10.0), blur_radius: 28.0 },
            ..Default::default()
        });
        // Occupies the pointer like a sheet: a click on the card isn't a click on the panel.
        mouse_area(card).interaction(mouse::Interaction::Idle).into()
    }
}
