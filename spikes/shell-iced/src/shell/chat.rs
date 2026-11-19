//! The chat sheet: a paper card floating over the right side of the patch window.
//! No LLM in the spike: Send appends the message and a canned reply.

use super::{icons, mock, theme};
use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding, Right, Theme,
    keyboard::{Key, key::Named},
    widget::{
        button, column, container, markdown, row, scrollable, space, text,
        text_editor, text_editor::Binding,
    },
};

pub struct Chat {
    draft: text_editor::Content,
    log: Vec<Entry>,
}

enum Entry {
    User { text: String, capture: Option<&'static str> },
    Assistant { reply: markdown::Content, changes: &'static [(&'static str, &'static str)] },
}

#[derive(Debug, Clone)]
pub enum Message {
    Edit(text_editor::Action),
    Send,
    Close,
    Link(markdown::Uri),
    /// Not wired up in the spike.
    Listen,
    CaptureLength,
    History,
    Undo,
    SaveSnapshot,
}

pub enum Event {
    Close,
}

impl Chat {
    pub fn new() -> Self {
        Self {
            draft: text_editor::Content::new(),
            log: vec![
                Entry::User {
                    text: "Make it warmer, less fizzy on the cymbals.".into(),
                    capture: Some("Capture 1 · 10.0 s · −14.2 LUFS"),
                },
                Entry::Assistant {
                    reply: markdown::Content::parse(
                        "The capture shows a lot of energy above 6 kHz after the chain. \
                         I moved some drive from **Diode** to **Tape**, which rounds off the top end.",
                    ),
                    changes: &[("Tape", "50 % → 62 %"), ("Diode", "24 % → 12 %")],
                },
            ],
        }
    }

    pub fn update(&mut self, message: Message) -> Option<Event> {
        match message {
            Message::Edit(action) => self.draft.perform(action),
            Message::Send => {
                let text = self.draft.text().trim().to_string();
                if !text.is_empty() {
                    self.log.push(Entry::User { text, capture: None });
                    self.log.push(Entry::Assistant {
                        reply: markdown::Content::parse(
                            "No LLM in the spike yet. This reply is **markdown**: lists,\n\n\
                             - `inline code` for block ids\n- **bold** parameter names\n\n\
                             render with the paper styles.",
                        ),
                        changes: &[],
                    });
                    self.draft = text_editor::Content::new();
                }
            }
            Message::Close => return Some(Event::Close),
            Message::Link(_) | Message::Listen | Message::CaptureLength | Message::History | Message::Undo | Message::SaveSnapshot => {}
        }
        None
    }

    pub fn view<'a>(&'a self, patch: &'static mock::Patch) -> Element<'a, Message> {
        let header = row![
            text("Chat").size(13.5).font(theme::font::PAPER_SEMIBOLD).color(theme::PAPER_INK),
            text(format!("· {}", patch.name)).size(12).font(theme::font::PAPER).color(theme::PAPER_MUTED),
            space::horizontal(),
            small(text("History").size(12).font(theme::font::PAPER), Message::History),
            button(container(icons::icon(icons::CLOSE, 11.0, 2.5, theme::PAPER_INK)).center(Fill))
                .on_press(Message::Close)
                .width(28)
                .height(26)
                .padding(0)
                .style(theme::paper_bare),
        ]
        .spacing(6)
        .align_y(Center)
        .height(48)
        .padding(Padding { left: 16.0, right: 8.0, ..Padding::ZERO });

        let messages = column(self.log.iter().map(|e| entry(e)))
            .spacing(14)
            .padding(Padding { top: 14.0, bottom: 14.0, left: 16.0, right: 16.0 });

        let footer = column![
            row![
                small(
                    row![icons::icon(icons::HEADPHONES, 11.0, 2.2, theme::SIGNAL_INK), text("Listen").size(12).font(theme::font::PAPER)].spacing(6).align_y(Center),
                    Message::Listen
                ),
                small(text("10 s ▾").size(12).font(theme::font::PAPER), Message::CaptureLength),
                text("captures attach to the next message").size(11).font(theme::font::PAPER).color(theme::PAPER_MUTED),
            ]
            .spacing(5)
            .align_y(Center),
            row![
                text_editor(&self.draft)
                    .placeholder("Ask for changes to this patch…")
                    .on_action(Message::Edit)
                    .key_binding(|key| {
                        // Enter sends, Shift+Enter inserts a newline.
                        if key.key == Key::Named(Named::Enter) && !key.modifiers.shift() {
                            Some(Binding::Custom(Message::Send))
                        } else {
                            Binding::from_key_press(key)
                        }
                    })
                    .height(52)
                    .padding(Padding { top: 8.0, bottom: 8.0, left: 11.0, right: 11.0 })
                    .font(theme::font::PAPER)
                    .size(12.5)
                    .style(theme::paper_editor),
                button(container(text("Send").size(12.5).font(theme::font::PAPER_SEMIBOLD)).center_y(Fill))
                    .on_press(Message::Send)
                    .height(52)
                    .padding(Padding { left: 16.0, right: 16.0, ..Padding::ZERO })
                    .style(theme::signal),
            ]
            .spacing(6),
        ]
        .spacing(7)
        .padding(Padding { top: 10.0, right: 12.0, bottom: 12.0, left: 12.0 });

        container(column![
            header,
            divider(),
            scrollable(messages).height(Fill).anchor_bottom(),
            divider(),
            footer,
        ])
        .width(theme::SIZE_SHEET)
        .height(Fill)
        .clip(true)
        .style(theme::paper_sheet)
        .into()
    }
}

fn entry(e: &Entry) -> Element<'_, Message> {
    match e {
        Entry::User { text: body, capture } => {
            let bubble = container(text(body.as_str()).size(12.5).font(theme::font::PAPER).color(theme::ON_PAPER_INK).line_height(1.45))
                .padding(Padding { top: 8.0, bottom: 8.0, left: 12.0, right: 12.0 })
                .max_width(270)
                .style(|_: &Theme| container::Style {
                    background: Some(theme::PAPER_INK.into()),
                    border: Border { radius: theme::radius(14.0).bottom_right(4.0), ..Default::default() },
                    ..Default::default()
                });
            let mut col = column![bubble].spacing(5).align_x(Right);
            if let Some(c) = capture {
                col = col.push(
                    container(row![icons::icon(icons::HEADPHONES, 11.0, 2.2, theme::SIGNAL_INK), text(*c).size(11).font(theme::font::PAPER).color(nice_plug_iced::iced::color!(0x47443e))].spacing(6).align_y(Center))
                        .padding(Padding { top: 3.0, bottom: 3.0, left: 10.0, right: 10.0 })
                        .style(|t: &Theme| container::Style { border: Border { radius: theme::radius(theme::RADIUS_PILL), ..theme::paper_raised(t).border }, ..theme::paper_raised(t) }),
                );
            }
            container(col).width(Fill).align_x(Right).into()
        }
        Entry::Assistant { reply, changes } => {
            let style = markdown::Style {
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
            };
            let md = container(markdown::view(reply.items(), markdown::Settings::with_text_size(12.5, style)).map(Message::Link))
                .style(|_: &Theme| container::Style { text_color: Some(theme::PAPER_INK_2), ..Default::default() });

            let mut col = column![md].spacing(10);
            if !changes.is_empty() {
                let rows = column(changes.iter().map(|(name, change)| {
                    row![
                        text(*name).size(11.5).font(theme::font::PAPER_MONO),
                        space::horizontal(),
                        text(*change).size(11.5).font(theme::font::PAPER_MONO)
                    ]
                    .into()
                }))
                .spacing(5);
                col = col.push(
                    container(
                        column![rows, row![small(text("Undo").size(12).font(theme::font::PAPER), Message::Undo), small(text("Save as snapshot").size(12).font(theme::font::PAPER), Message::SaveSnapshot)].spacing(5)]
                            .spacing(9),
                    )
                    .padding(Padding { top: 10.0, bottom: 10.0, left: 12.0, right: 12.0 })
                    .width(Fill)
                    .style(theme::paper_raised),
                );
            }
            col.into()
        }
    }
}

fn small<'a>(content: impl Into<Element<'a, Message>>, msg: Message) -> Element<'a, Message> {
    button(container(content).center_y(Fill))
        .on_press(msg)
        .height(26)
        .padding(Padding { left: 10.0, right: 10.0, ..Padding::ZERO })
        .style(theme::paper_small)
        .into()
}

fn divider<'a>() -> Element<'a, Message> {
    container(space::horizontal().height(1)).style(theme::rule(theme::PAPER_DIVIDER)).into()
}
