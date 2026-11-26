//! The top bar across the whole window, like a window title bar: the "ScriptFX" wordmark in
//! the centre, the library opener on the left (its sheet opens on the left), the other sheet
//! openers on the right (patch controls, chat, settings). Everything the
//! shell can open lives here, so the notch only carries the patch file and its snapshots.
//! Stateless: the shell owns everything it shows.

use super::{icons, theme};
use nice_plug_iced::iced::{
    Center, Element, Fill, Padding, Theme,
    alignment::Horizontal,
    widget::{button, container, row, text},
};

#[derive(Debug, Clone)]
pub enum Message {
    ToggleLibrary,
    ToggleControls,
    ToggleChat,
    ToggleSettings,
}

pub struct Props {
    pub library_open: bool,
    /// The patch's controls exist only with a patch loaded.
    pub controls_available: bool,
    pub controls_open: bool,
    pub chat_open: bool,
    pub settings_open: bool,
    /// Passive mode: no patch, so nothing to pick or control. Chat and settings stay reachable
    /// (settings is where passive mode is switched off again).
    pub passive: bool,
}

pub fn view<'a>(p: Props) -> Element<'a, Message> {
    let wordmark = text("ScriptFX").size(12.5).font(theme::font::SANS_SEMIBOLD).color(theme::c().frame_ink);

    let openers = row![
        opener(icons::SLIDERS, Some("Controls"), (p.controls_available && !p.passive).then_some(Message::ToggleControls), p.controls_open),
        opener(icons::CHAT, Some("Chat"), Some(Message::ToggleChat), p.chat_open),
        opener(icons::GEAR, None, Some(Message::ToggleSettings), p.settings_open),
    ]
    .spacing(theme::SPACE_HAIR)
    .align_y(Center);

    // Equal-width sides keep the wordmark centred on the window, whatever sits on the right.
    let left = row![opener(icons::LIBRARY, Some("Patches"), (!p.passive).then_some(Message::ToggleLibrary), p.library_open)]
    .spacing(theme::SPACE_HAIR)
    .align_y(Center);
    let bar = row![
        container(left).width(Fill),
        wordmark,
        container(openers).width(Fill).align_x(Horizontal::Right),
    ]
    .align_y(Center)
    .height(theme::SIZE_TOP_BAR)
    .padding(Padding { left: theme::SPACE_GUTTER, right: theme::SPACE_GUTTER, ..Padding::ZERO });

    container(bar).width(Fill).into()
}

/// A sheet opener: icon (+ label), filled while its sheet is open; `None` = disabled.
fn opener<'a>(icon: &'static str, label: Option<&'static str>, msg: Option<Message>, open: bool) -> Element<'a, Message> {
    let c = theme::c();
    let ink = if open {
        c.on_frame_invert
    } else if msg.is_none() {
        theme::alpha(c.frame_ink_2, 0.35)
    } else {
        c.frame_ink_2
    };
    let mut content = row![icons::icon(icon, 12.0, 2.2, ink)].spacing(6).align_y(Center);
    if let Some(label) = label {
        let font = if open { theme::font::SANS_SEMIBOLD } else { theme::font::SANS_MEDIUM };
        content = content.push(text(label).size(12).font(font).color(ink));
    }
    // Labelled openers hug their content; the icon-only one is a fixed square.
    let b = match label {
        Some(_) => button(container(content).center_y(Fill)).padding(Padding { left: 10.0, right: 11.0, ..Padding::ZERO }),
        None => button(container(content).center(Fill)).width(28).padding(0),
    };
    let b = b.on_press_maybe(msg).height(26);
    b.style(move |t: &Theme, s| if open { theme::frame_primary(t, s) } else { theme::frame_ghost(false)(t, s) }).into()
}
