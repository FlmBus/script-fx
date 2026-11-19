//! The chat sheet: a paper card floating over the right side of the patch window.
//! Chats are global (not per patch); for now there is one, "Main".
//! Listen lives here: a capture's analysis is posted into the chat and attached to the next
//! message. No LLM is connected yet; sending says so instead of pretending.

use std::sync::Arc;

use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding, Right, Theme,
    keyboard::{Key, key::Named},
    widget::{button, column, container, markdown, row, scrollable, space, text, text_editor, text_editor::Binding},
};

use super::{icons, sheet, theme};
use crate::listen::{CaptureResult, Status};

pub struct Chat {
    draft: text_editor::Content,
    log: Vec<Entry>,
    /// Goes with the next message.
    attached: Option<Arc<CaptureResult>>,
}

enum Entry {
    User { text: String, capture: Option<Arc<CaptureResult>> },
    Reply(markdown::Content),
}

#[derive(Debug, Clone)]
pub enum Message {
    Edit(text_editor::Action),
    Send,
    Close,
    Link,
    Listen,
    CancelListen,
    CycleLength,
    Detach,
}

/// What the shell has to act on.
pub enum Event {
    Close,
    Listen,
    CancelListen,
    CycleLength,
}

impl Chat {
    pub fn new() -> Self {
        Self { draft: text_editor::Content::new(), log: Vec::new(), attached: None }
    }

    /// A capture finished: post its analysis and attach it to the next message.
    pub fn capture_done(&mut self, result: Arc<CaptureResult>) {
        self.log.push(Entry::Reply(markdown::Content::parse(&result.summary_markdown())));
        self.attached = Some(result);
    }

    pub fn update(&mut self, message: Message) -> Option<Event> {
        match message {
            Message::Edit(action) => self.draft.perform(action),
            Message::Send => {
                let text = self.draft.text().trim().to_string();
                if !text.is_empty() {
                    self.log.push(Entry::User { text, capture: self.attached.take() });
                    self.log.push(Entry::Reply(markdown::Content::parse(
                        "No LLM is connected yet, so nothing changed. Once it is, your message and any \
                         attached capture go to it, and it answers here.",
                    )));
                    self.draft = text_editor::Content::new();
                }
            }
            Message::Close => return Some(Event::Close),
            Message::Listen => return Some(Event::Listen),
            Message::CancelListen => return Some(Event::CancelListen),
            Message::CycleLength => return Some(Event::CycleLength),
            Message::Detach => self.attached = None,
            Message::Link => {}
        }
        None
    }

    pub fn view<'a>(&'a self, listen: Status, seconds: f32) -> Element<'a, Message> {
        let header = sheet::header("Chat", "Main", Message::Close);

        let body: Element<'a, Message> = if self.log.is_empty() {
            container(
                text("Ask for a new patch or changes to the loaded one. Listen captures what your track sounds like before and after the chain; the capture goes with your next message.")
                    .size(12)
                    .font(theme::font::PAPER)
                    .color(theme::c().paper_muted)
                    .line_height(1.5),
            )
            .padding(16)
            .into()
        } else {
            scrollable(
                column(self.log.iter().map(entry))
                    .spacing(14)
                    .padding(Padding { top: 14.0, bottom: 14.0, left: 16.0, right: 16.0 }),
            )
            .anchor_bottom()
            .direction(theme::thin_scrollbar())
            .style(theme::paper_scrollbar)
            .into()
        };

        let listen_row: Element<'a, Message> = match listen {
            Status::Idle => row![
                sheet::small(
                    row![icons::icon(icons::HEADPHONES, 11.0, 2.2, theme::c().signal_ink), sheet::label("Listen")].spacing(6).align_y(Center),
                    Some(Message::Listen)
                ),
                sheet::small(sheet::label(format!("{:.0} s ▾", seconds)), Some(Message::CycleLength)),
                text("captures attach to the next message").size(11).font(theme::font::PAPER).color(theme::c().paper_muted),
            ]
            .spacing(5)
            .align_y(Center)
            .into(),
            Status::Armed | Status::Recording { .. } | Status::Analyzing => {
                let what = match listen {
                    Status::Armed => "Play your track. Listening starts with the first signal.".to_string(),
                    Status::Recording { seconds, of } => format!("Listening… {seconds:.1} / {of:.0} s"),
                    _ => "Analyzing…".to_string(),
                };
                let mut r = row![
                    icons::icon(icons::HEADPHONES, 11.0, 2.2, theme::c().signal_ink),
                    text(what).size(11.5).font(theme::font::PAPER).color(theme::c().paper_ink),
                    space::horizontal(),
                ]
                .spacing(6)
                .align_y(Center);
                if listen != Status::Analyzing {
                    r = r.push(sheet::small(sheet::label("Cancel"), Some(Message::CancelListen)));
                }
                r.height(26).into()
            }
        };

        let mut footer = column![listen_row].spacing(7).padding(Padding { top: 10.0, right: 12.0, bottom: 12.0, left: 12.0 });
        if let Some(c) = &self.attached {
            footer = footer.push(
                row![
                    capture_chip(c),
                    button(container(icons::icon(icons::CLOSE, 9.0, 2.5, theme::c().paper_muted)).center(Fill))
                        .on_press(Message::Detach)
                        .width(20)
                        .height(20)
                        .padding(0)
                        .style(theme::paper_bare),
                ]
                .spacing(4)
                .align_y(Center),
            );
        }
        footer = footer.push(
            row![
                text_editor(&self.draft)
                    .placeholder("Ask for a patch or a change…")
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
        );

        sheet::frame(column![header, sheet::divider(), container(body).height(Fill), sheet::divider(), footer])
    }
}

fn capture_chip<'a>(c: &CaptureResult) -> Element<'a, Message> {
    container(
        row![
            icons::icon(icons::HEADPHONES, 11.0, 2.2, theme::c().signal_ink),
            text(format!("Capture · {}", c.label())).size(11).font(theme::font::PAPER).color(theme::c().paper_ink_3)
        ]
        .spacing(6)
        .align_y(Center),
    )
    .padding(Padding { top: 3.0, bottom: 3.0, left: 10.0, right: 10.0 })
    .style(|t: &Theme| container::Style {
        border: Border { radius: theme::radius(theme::RADIUS_PILL), ..theme::paper_raised(t).border },
        ..theme::paper_raised(t)
    })
    .into()
}

fn entry(e: &Entry) -> Element<'_, Message> {
    match e {
        Entry::User { text: body, capture } => {
            let bubble = container(text(body.as_str()).size(12.5).font(theme::font::PAPER).color(theme::c().on_paper_ink).line_height(1.45))
                .padding(Padding { top: 8.0, bottom: 8.0, left: 12.0, right: 12.0 })
                .max_width(270)
                .style(|_: &Theme| container::Style {
                    background: Some(theme::c().paper_ink.into()),
                    border: Border { radius: theme::radius(14.0).bottom_right(4.0), ..Default::default() },
                    ..Default::default()
                });
            let mut col = column![bubble].spacing(5).align_x(Right);
            if let Some(c) = capture {
                col = col.push(capture_chip(c));
            }
            container(col).width(Fill).align_x(Right).into()
        }
        Entry::Reply(md) => container(markdown::view(md.items(), sheet::markdown_settings()).map(|_| Message::Link))
            .style(|_: &Theme| container::Style { text_color: Some(theme::c().paper_ink_2), ..Default::default() })
            .into(),
    }
}
