//! The window when no patch is loaded: build a patch from a description. No notch: there is
//! nothing to control. Building needs the LLM, so those controls are disabled and say why.
//! (Passive mode lives in the top bar.)

use super::theme;
use nice_plug_iced::iced::{
    Border, Element, Fill, Gradient, Padding, Shadow, Theme, Vector,
    gradient::Linear,
    widget::{button, column, container, row, space, text, text_input},
};

#[derive(Default)]
pub struct Empty {
    describe: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Describe(String),
}

pub enum Event {}

impl Empty {
    pub fn update(&mut self, message: Message) -> Option<Event> {
        match message {
            Message::Describe(s) => self.describe = s,
        }
        None
    }

    /// `inset`: room on the right taken by an open sheet; the card centres in what's left.
    pub fn view(&self, inset: f32) -> Element<'_, Message> {
        let intro = column![
            text("No patch loaded").size(16).font(theme::font::SANS_SEMIBOLD).color(theme::c().frame_ink),
            text("The signal passes through untouched. Pick a patch on the left, or describe a new one.").size(12).color(theme::c().frame_muted),
        ]
        .spacing(5)
        .padding(Padding { top: 16.0, right: 18.0, bottom: 10.0, left: 18.0 });

        let build = column![
            text_input("Describe a new patch, e.g. warm up a thin DI bass", &self.describe)
                .on_input(Message::Describe)
                .size(12.5)
                .padding(Padding { top: 10.0, bottom: 10.0, left: 12.0, right: 12.0 })
                .style(theme::frame_input),
            row![
                text("needs the LLM (not connected yet)").size(11).color(theme::c().frame_subtle),
                space::horizontal(),
                button(text("Listen first").size(12.5))
                    .height(32)
                    .padding(Padding { top: 8.0, bottom: 8.0, left: 13.0, right: 13.0 })
                    .style(rounded(theme::frame_ghost(true))),
                button(text("Build patch").size(12.5).font(theme::font::SANS_SEMIBOLD))
                    .height(32)
                    .padding(Padding { top: 8.0, bottom: 8.0, left: 15.0, right: 15.0 })
                    .style(rounded(theme::frame_primary)),
            ]
            .spacing(6)
            .align_y(nice_plug_iced::iced::Center),
        ]
        .spacing(8)
        .padding(Padding { top: 0.0, right: 18.0, bottom: 14.0, left: 18.0 });

        let card = container(column![intro, build])
            .width(440)
            .style(|_: &Theme| container::Style {
                background: Some(theme::c().frame_raised.into()),
                border: Border { radius: theme::radius(18.0), width: 1.0, color: theme::c().frame_hairline_strong },
                shadow: Shadow { color: theme::shadow(0.5), offset: Vector::new(0.0, 24.0), blur_radius: 60.0 },
                ..Default::default()
            });

        container(container(card).center(Fill).padding(Padding { right: inset, ..Padding::ZERO }))
            .center(Fill)
            .style(|_: &Theme| container::Style {
                background: Some(
                    Gradient::Linear(Linear::new(std::f32::consts::PI).add_stop(0.0, theme::c().frame_backdrop.0).add_stop(0.7, theme::c().frame_backdrop.1)).into(),
                ),
                border: Border { radius: theme::radius(theme::RADIUS_WINDOW), width: 1.0, color: theme::c().frame_hairline },
                ..Default::default()
            })
            .into()
    }
}

fn rounded(
    f: impl Fn(&Theme, button::Status) -> button::Style,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |t, s| {
        let mut st = f(t, s);
        st.border.radius = theme::radius(10.0);
        st
    }
}
