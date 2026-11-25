//! The window in **passive mode**: this instance only listens. No patch is loaded (the signal
//! passes through, the Lua runtime and the cached layers are freed), every control is disabled
//! and this card fills the window. Switched here and in the settings sheet; the state lives in
//! the project (`FxParams::passive`).
//!
//! Stateless: the shell owns `passive`.

use super::theme;
use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Gradient, Padding, Shadow, Theme, Vector,
    gradient::Linear,
    widget::{button, column, container, row, space, text},
};

#[derive(Debug, Clone)]
pub enum Message {
    /// Leave passive mode (the shell clears `FxParams::passive`).
    Leave,
}

pub struct Props<'a> {
    /// The track this instance sits on, as the host names it. `None` = the host didn't say.
    pub track: Option<&'a str>,
    /// Room taken by open sheets, so the card centres in what's left (like the empty card).
    pub left: f32,
    pub right: f32,
}

pub fn view<'a>(p: Props<'a>) -> Element<'a, Message> {
    let c = theme::c();

    let title = match p.track {
        Some(name) => format!("Listening to {name}"),
        None => "Passive listener".to_owned(),
    };

    let head = column![
        row![dot(), text(title).size(16).font(theme::font::SANS_SEMIBOLD).color(c.frame_ink)].spacing(8).align_y(Center),
        text("This instance doesn't process anything: the signal passes through and no patch is loaded.")
            .size(12)
            .color(c.frame_muted),
    ]
    .spacing(6);

    let why = text("Another ScriptFX can record this channel when you press Listen, to compare its track against this one.")
        .size(12)
        .color(c.frame_subtle);

    let leave = row![
        space::horizontal(),
        button(text("Leave passive mode").size(12.5).font(theme::font::SANS_SEMIBOLD))
            .on_press(Message::Leave)
            .height(32)
            .padding(Padding { top: 8.0, bottom: 8.0, left: 15.0, right: 15.0 })
            .style(|t: &Theme, s| {
                let mut st = theme::frame_primary(t, s);
                st.border.radius = theme::radius(10.0);
                st
            }),
    ]
    .align_y(Center);

    let card = container(column![head, why, leave].spacing(14).padding(Padding { top: 16.0, right: 18.0, bottom: 14.0, left: 18.0 }))
        .width(440)
        .style(|_: &Theme| container::Style {
            background: Some(theme::c().frame_raised.into()),
            border: Border { radius: theme::radius(18.0), width: 1.0, color: theme::c().frame_hairline_strong },
            shadow: Shadow { color: theme::shadow(0.5), offset: Vector::new(0.0, 24.0), blur_radius: 60.0 },
            ..Default::default()
        });

    container(container(card).center(Fill).padding(Padding { left: p.left, right: p.right, ..Padding::ZERO }))
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

/// The "listening" LED, in the shell's ok colour.
fn dot<'a, M: 'a>() -> Element<'a, M> {
    container(space::horizontal().width(8).height(8))
        .style(|_: &Theme| container::Style {
            background: Some(theme::c().led_ok.into()),
            border: Border { radius: theme::radius(4.0), ..Default::default() },
            ..Default::default()
        })
        .into()
}
