//! The window when no patch is loaded: build a patch from a description, or run as a passive
//! listener. No notch, no signal line: there is nothing to control.

use super::theme;
use nice_plug_iced::iced::{
    Border, Element, Fill, Gradient, Padding, Shadow, Theme, Vector,
    gradient::Linear,
    widget::{button, checkbox, column, container, row, space, text, text_input},
};

#[derive(Default)]
pub struct Empty {
    describe: String,
    passive: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Describe(String),
    Build,
    ListenFirst,
    Passive(bool),
}

pub enum Event {
    Build,
}

impl Empty {
    pub fn update(&mut self, message: Message) -> Option<Event> {
        match message {
            Message::Describe(s) => self.describe = s,
            Message::Build => return Some(Event::Build),
            Message::ListenFirst => {}
            Message::Passive(on) => self.passive = on,
        }
        None
    }

    pub fn view(&self) -> Element<'_, Message> {
        let intro = column![
            text("No patch loaded").size(16).font(theme::font::SANS_SEMIBOLD).color(theme::FRAME_INK),
            text("The signal passes through untouched.").size(12).color(theme::FRAME_MUTED),
        ]
        .spacing(5)
        .padding(Padding { top: 16.0, right: 18.0, bottom: 10.0, left: 18.0 });

        let build = column![
            text_input("Describe a new patch, e.g. warm up a thin DI bass", &self.describe)
                .on_input(Message::Describe)
                .on_submit(Message::Build)
                .size(12.5)
                .padding(Padding { top: 10.0, bottom: 10.0, left: 12.0, right: 12.0 })
                .style(theme::frame_input),
            row![
                space::horizontal(),
                button(text("Listen first").size(12.5))
                    .on_press(Message::ListenFirst)
                    .height(32)
                    .padding(Padding { top: 8.0, bottom: 8.0, left: 13.0, right: 13.0 })
                    .style(rounded(theme::frame_ghost(true))),
                button(text("Build patch").size(12.5).font(theme::font::SANS_SEMIBOLD))
                    .on_press(Message::Build)
                    .height(32)
                    .padding(Padding { top: 8.0, bottom: 8.0, left: 15.0, right: 15.0 })
                    .style(rounded(theme::frame_primary)),
            ]
            .spacing(6),
        ]
        .spacing(8)
        .padding(Padding { top: 0.0, right: 18.0, bottom: 14.0, left: 18.0 });

        let passive = row![
            checkbox(self.passive)
                .on_toggle(Message::Passive)
                .size(14)
                .style(|_: &Theme, status| checkbox::Style {
                    background: if matches!(status, checkbox::Status::Active { is_checked: true } | checkbox::Status::Hovered { is_checked: true }) {
                        theme::FRAME_INVERT.into()
                    } else {
                        theme::FRAME_FIELD.into()
                    },
                    icon_color: theme::ON_FRAME_INVERT,
                    border: Border { radius: theme::radius(4.0), width: 1.0, color: theme::FRAME_HAIRLINE_STRONG },
                    text_color: None,
                }),
            column![
                text("Passive listener").size(12).font(theme::font::SANS_SEMIBOLD).color(theme::FRAME_INK),
                text("Don't process. Share this channel's audio with the other ScriptFX instances, e.g. on the master.")
                    .size(12)
                    .color(theme::FRAME_MUTED)
                    .line_height(1.4),
            ]
            .spacing(3),
        ]
        .spacing(10)
        .padding(Padding { top: 12.0, right: 18.0, bottom: 14.0, left: 18.0 });

        let card = container(column![intro, build, rule(), passive])
            .width(440)
            .style(|_: &Theme| container::Style {
                background: Some(theme::FRAME_RAISED.into()),
                border: Border { radius: theme::radius(18.0), width: 1.0, color: theme::FRAME_HAIRLINE_STRONG },
                shadow: Shadow { color: theme::alpha(nice_plug_iced::iced::Color::BLACK, 0.5), offset: Vector::new(0.0, 24.0), blur_radius: 60.0 },
                ..Default::default()
            });

        container(card)
            .center(Fill)
            .style(|_: &Theme| container::Style {
                background: Some(
                    Gradient::Linear(Linear::new(std::f32::consts::PI).add_stop(0.0, nice_plug_iced::iced::color!(0x1a1a1d)).add_stop(0.7, nice_plug_iced::iced::color!(0x0f0f11))).into(),
                ),
                border: Border { radius: theme::radius(theme::RADIUS_WINDOW), width: 1.0, color: theme::FRAME_HAIRLINE },
                ..Default::default()
            })
            .into()
    }
}

fn rule<'a>() -> Element<'a, Message> {
    container(space::horizontal().height(1)).style(theme::rule(theme::FRAME_HAIRLINE)).into()
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
