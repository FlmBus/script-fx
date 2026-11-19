//! Building blocks shared by the paper sheets (chat, controls): frame, header, small buttons,
//! dividers, markdown style.

use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding,
    widget::{Text, button, container, markdown, row, space, text},
};

use super::{icons, theme};

/// The paper card (size-sheet wide, full height).
pub fn frame<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content).width(theme::SIZE_SHEET).height(Fill).clip(true).style(theme::paper_sheet).into()
}

/// "Title · subtitle", close button.
pub fn header<'a, M: Clone + 'a>(title: &'a str, subtitle: &'a str, close: M) -> Element<'a, M> {
    row![
        text(title).size(13.5).font(theme::font::PAPER_SEMIBOLD).color(theme::PAPER_INK),
        text(format!("· {subtitle}")).size(12).font(theme::font::PAPER).color(theme::PAPER_MUTED),
        space::horizontal(),
        button(container(icons::icon(icons::CLOSE, 11.0, 2.5, theme::PAPER_INK)).center(Fill))
            .on_press(close)
            .width(28)
            .height(26)
            .padding(0)
            .style(theme::paper_bare),
    ]
    .spacing(6)
    .align_y(Center)
    .height(48)
    .padding(Padding { left: 16.0, right: 8.0, ..Padding::ZERO })
    .into()
}

/// Text for paper buttons.
pub fn label<'a>(s: impl text::IntoFragment<'a>) -> Text<'a> {
    text(s).size(12).font(theme::font::PAPER)
}

/// Paper small button (26 px). `None` = disabled.
pub fn small<'a, M: Clone + 'a>(content: impl Into<Element<'a, M>>, msg: Option<M>) -> Element<'a, M> {
    button(container(content).center_y(Fill))
        .on_press_maybe(msg)
        .height(26)
        .padding(Padding { left: 10.0, right: 10.0, ..Padding::ZERO })
        .style(theme::paper_small)
        .into()
}

pub fn divider<'a, M: 'a>() -> Element<'a, M> {
    container(space::horizontal().height(1)).style(theme::rule(theme::PAPER_DIVIDER)).into()
}

pub fn section_title<'a, M: 'a>(s: &'a str) -> Element<'a, M> {
    text(s).size(11).font(theme::font::PAPER_SEMIBOLD).color(theme::PAPER_MUTED).into()
}

pub fn markdown_settings() -> markdown::Settings {
    markdown::Settings::with_text_size(
        12.5,
        markdown::Style {
            font: theme::font::PAPER,
            inline_code_font: theme::font::PAPER_MONO,
            code_block_font: theme::font::PAPER_MONO,
            inline_code_color: theme::PAPER_INK,
            inline_code_highlight: markdown::Highlight {
                background: theme::PAPER_DIVIDER.into(),
                border: Border { radius: theme::radius(4.0), ..Default::default() },
            },
            inline_code_padding: Padding { left: 2.0, right: 2.0, ..Padding::ZERO },
            link_color: theme::SIGNAL_INK,
        },
    )
}
