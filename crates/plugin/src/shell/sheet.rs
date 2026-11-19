//! Building blocks shared by the paper sheets (chat, controls): frame, header, small buttons,
//! dividers, markdown style.

use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding,
    mouse,
    widget::{Text, button, container, markdown, mouse_area, row, space, text},
};

use super::{icons, theme};

/// The paper card (size-sheet wide, full height).
pub fn frame<'a, M: Clone + 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    frame_width(content, theme::SIZE_SHEET)
}

/// The paper card at another width (the library).
pub fn frame_width<'a, M: Clone + 'a>(content: impl Into<Element<'a, M>>, width: f32) -> Element<'a, M> {
    // `Idle` marks the sheet as occupied, so the stack doesn't pass clicks on its blank space
    // through to the panel below (a click on the panel closes the sheets).
    mouse_area(container(content).width(width).height(Fill).clip(true).style(theme::paper_sheet))
        .interaction(mouse::Interaction::Idle)
        .into()
}

/// "Title · subtitle", close button.
pub fn header<'a, M: Clone + 'a>(title: &'a str, subtitle: &'a str, close: M) -> Element<'a, M> {
    row![
        text(title).size(13.5).font(theme::font::PAPER_SEMIBOLD).color(theme::c().paper_ink),
        text(format!("· {subtitle}")).size(12).font(theme::font::PAPER).color(theme::c().paper_muted),
        space::horizontal(),
        button(container(icons::icon(icons::CLOSE, 11.0, 2.5, theme::c().paper_ink)).center(Fill))
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
    container(space::horizontal().height(1)).style(theme::rule(theme::c().paper_divider)).into()
}

pub fn section_title<'a, M: 'a>(s: &'a str) -> Element<'a, M> {
    text(s).size(11).font(theme::font::PAPER_SEMIBOLD).color(theme::c().paper_muted).into()
}

pub fn markdown_settings() -> markdown::Settings {
    markdown::Settings::with_text_size(
        12.5,
        markdown::Style {
            font: theme::font::PAPER,
            inline_code_font: theme::font::PAPER_MONO,
            code_block_font: theme::font::PAPER_MONO,
            inline_code_color: theme::c().paper_ink,
            inline_code_highlight: markdown::Highlight {
                background: theme::c().paper_divider.into(),
                border: Border { radius: theme::radius(4.0), ..Default::default() },
            },
            inline_code_padding: Padding { left: 2.0, right: 2.0, ..Padding::ZERO },
            link_color: theme::c().signal_ink,
        },
    )
}
