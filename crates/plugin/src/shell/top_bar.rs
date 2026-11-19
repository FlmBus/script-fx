//! The top bar across the whole window: the plugin's name on the left; on the right the UI
//! scale (per user) and instance-wide modes. Today that is passive mode (this instance only listens and shares its audio with
//! the other ScriptFX instances, e.g. on the master). Passive mode isn't built yet, so its
//! switch is shown but disabled and says so. Rightmost: the chat (global, not per patch).

use super::{icons, theme};
use crate::user_settings::{self, ThemeMode};
use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding, Theme,
    alignment::Horizontal,
    widget::{button, container, row, space, text},
};

#[derive(Debug, Clone)]
pub enum Message {
    TogglePassive,
    ToggleChat,
    ScaleDown,
    ScaleUp,
    SetTheme(ThemeMode),
}

pub struct Props {
    pub passive: bool,
    /// False while passive mode isn't implemented.
    pub passive_available: bool,
    /// 1.0 = 100 %.
    pub ui_scale: f32,
    pub chat_open: bool,
    pub theme: ThemeMode,
}

pub fn view<'a>(p: Props) -> Element<'a, Message> {
    let name = text("ScriptFX").size(12.5).font(theme::font::SANS_SEMIBOLD).color(theme::c().frame_ink);

    let bar = row![name, space::horizontal(), theme_switch(p.theme), divider(), scale(p.ui_scale), divider(), passive(&p), divider(), chat(p.chat_open)]
        .spacing(theme::SPACE_2)
        .align_y(Center)
        .height(theme::SIZE_TOP_BAR)
        .padding(Padding { left: 16.0, right: theme::SPACE_GUTTER, ..Padding::ZERO });

    container(bar).width(Fill).into()
}

// ---- theme switch ------------------------------------------------------------------------------

/// System | Light | Dark as three icon segments; the active one is filled.
fn theme_switch<'a>(mode: ThemeMode) -> Element<'a, Message> {
    let segment = |m: ThemeMode| {
        let on = m == mode;
        let icon = match m {
            ThemeMode::System => icons::AUTO,
            ThemeMode::Light => icons::SUN,
            ThemeMode::Dark => icons::MOON,
        };
        let ink = if on { theme::c().on_frame_invert } else { theme::c().frame_muted };
        button(container(icons::icon(icon, 12.0, 2.0, ink)).center(Fill))
            .on_press(Message::SetTheme(m))
            .width(26)
            .height(22)
            .padding(0)
            .style(move |t: &Theme, s| if on { theme::frame_primary(t, s) } else { theme::frame_ghost(false)(t, s) })
            .into()
    };
    container(row(ThemeMode::ALL.into_iter().map(segment)).spacing(2))
        .padding(2)
        .style(|_: &Theme| container::Style {
            background: Some(theme::c().frame_field.into()),
            border: Border { radius: theme::radius(theme::RADIUS_CONTROL), width: 1.0, color: theme::c().frame_hairline },
            ..Default::default()
        })
        .into()
}

// ---- UI scale stepper ---------------------------------------------------------------------------

fn scale<'a>(ui_scale: f32) -> Element<'a, Message> {
    let step = |icon: &'static str, msg: Message, enabled: bool| {
        let ink = if enabled { theme::c().frame_ink_2 } else { theme::alpha(theme::c().frame_ink_2, 0.35) };
        button(container(icons::icon(icon, 12.0, 2.2, ink)).center(Fill))
            .on_press_maybe(enabled.then_some(msg))
            .width(22)
            .height(22)
            .padding(0)
            .style(theme::frame_ghost(false))
    };
    let value = text(format!("{:.0} %", ui_scale * 100.0)).size(11.5).font(theme::font::SANS_MEDIUM).color(theme::c().frame_muted).width(44).center();
    row![
        step(icons::MINUS, Message::ScaleDown, ui_scale > user_settings::UI_SCALE_MIN),
        value,
        step(icons::PLUS, Message::ScaleUp, ui_scale < user_settings::UI_SCALE_MAX),
    ]
    .align_y(Center)
    .into()
}

fn divider<'a>() -> Element<'a, Message> {
    container(space::vertical().width(1).height(14)).style(theme::rule(theme::c().frame_hairline_strong)).into()
}

// ---- chat --------------------------------------------------------------------------------------

/// Opens/closes the chat sheet; filled while open.
fn chat<'a>(open: bool) -> Element<'a, Message> {
    let ink = if open { theme::c().on_frame_invert } else { theme::c().frame_ink_2 };
    let font = if open { theme::font::SANS_SEMIBOLD } else { theme::font::SANS_MEDIUM };
    button(container(row![icons::icon(icons::CHAT, 12.0, 2.2, ink), text("Chat").size(12).font(font).color(ink)].spacing(6).align_y(Center)).center_y(Fill))
        .on_press(Message::ToggleChat)
        .height(26)
        .padding(Padding { left: 10.0, right: 11.0, ..Padding::ZERO })
        .style(move |t: &Theme, s| if open { theme::frame_primary(t, s) } else { theme::frame_ghost(false)(t, s) })
        .into()
}

// ---- passive mode switch ------------------------------------------------------------------------

fn passive<'a>(p: &Props) -> Element<'a, Message> {
    let on = p.passive && p.passive_available;
    let ink = if p.passive_available { theme::c().frame_ink_2 } else { theme::alpha(theme::c().frame_ink_2, 0.45) };

    let mut content = row![switch(on, p.passive_available), text("Passive listener").size(12).font(theme::font::SANS_MEDIUM).color(ink)]
        .spacing(8)
        .align_y(Center);
    if !p.passive_available {
        content = content.push(badge("soon"));
    }

    button(container(content).center_y(Fill))
        .on_press_maybe(p.passive_available.then_some(Message::TogglePassive))
        .height(26)
        .padding(Padding { left: 8.0, right: 10.0, ..Padding::ZERO })
        .style(theme::frame_ghost(false))
        .into()
}

/// A small pill switch: track with a knob on the left (off) or right (on).
fn switch<'a>(on: bool, enabled: bool) -> Element<'a, Message> {
    let knob_color = if !enabled { theme::alpha(theme::c().frame_ink, 0.35) } else if on { theme::c().on_frame_invert } else { theme::c().frame_ink };
    let track_color = if on { theme::c().led_ok } else { theme::c().frame_control };
    let knob = container(space::horizontal().width(10).height(10)).style(move |_: &Theme| container::Style {
        background: Some(knob_color.into()),
        border: Border { radius: theme::radius(5.0), ..Default::default() },
        ..Default::default()
    });
    let track = container(knob)
        .width(24)
        .height(14)
        .padding(2)
        .align_x(if on { Horizontal::Right } else { Horizontal::Left })
        .center_y(14)
        .style(move |_: &Theme| container::Style {
            background: Some(track_color.into()),
            border: Border { radius: theme::radius(7.0), width: 1.0, color: theme::c().frame_hairline_strong },
            ..Default::default()
        });
    track.into()
}

fn badge<'a>(label: &'a str) -> Element<'a, Message> {
    container(text(label).size(10).font(theme::font::SANS_MEDIUM).color(theme::c().frame_subtle))
        .padding(Padding { top: 1.0, bottom: 1.0, left: 6.0, right: 6.0 })
        .style(|_: &Theme| container::Style {
            border: Border { radius: theme::radius(theme::RADIUS_PILL), width: 1.0, color: theme::c().frame_hairline_strong },
            ..Default::default()
        })
        .into()
}
