//! The chat sheet: a paper card floating over the right side of the patch window.
//! Chats are global (not per patch); for now there is one, "Main".
//! Listen lives here: a capture's analysis is posted into the chat and attached to the next
//! message. The conversation itself is `assistant::Assistant`; this holds what the view needs of it.

use std::sync::Arc;

use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding, Right, Theme,
    keyboard::{Key, key::Named},
    widget::{self, button, column, container, markdown, row, scrollable, space, text, text_editor, text_editor::Binding},
};

use super::{icons, sheet, theme};
use crate::assistant::{Assistant, Item};
use crate::listen::{self, CaptureResult, Status};

pub struct Chat {
    draft: text_editor::Content,
    /// The assistant's log, parsed for display (same indices).
    log: Vec<Entry>,
    /// Goes with the next message.
    attached: Option<Arc<CaptureResult>>,
    /// What the running turn is doing.
    activity: Option<String>,
    /// The log shows its end: new entries scroll into view. Scrolling up to read turns it off.
    at_end: bool,
    /// Frames left that scroll to the end (the first may still see the log without the new entry).
    follow: u8,
}

enum Entry {
    User { text: String, capture: Option<Arc<CaptureResult>> },
    Reply(markdown::Content),
    Change { label: String, undone: bool },
    Error(String),
    NewChat,
}

#[derive(Debug, Clone)]
pub enum Message {
    Edit(text_editor::Action),
    Send,
    Cancel,
    /// Undo the change at this index of the log.
    Undo(usize),
    Close,
    Link,
    Listen,
    StopListen,
    CancelListen,
    CycleLength,
    Detach,
    Scrolled(scrollable::Viewport),
}

/// What the shell has to act on.
pub enum Event {
    Send(String),
    Cancel,
    Undo(usize),
    Detach,
    Close,
    Listen,
    StopListen,
    CancelListen,
    CycleLength,
}

impl Chat {
    pub fn new() -> Self {
        Self { draft: text_editor::Content::new(), log: Vec::new(), attached: None, activity: None, at_end: true, follow: 0 }
    }

    /// Once per frame: what's new in the conversation.
    pub fn sync(&mut self, a: &Assistant) {
        if self.log.len() < a.log.len() && self.at_end {
            self.follow = 2;
        }
        for item in &a.log[self.log.len()..] {
            self.log.push(match item {
                Item::User { text, capture } => Entry::User { text: text.clone(), capture: capture.clone() },
                Item::Reply(text) => Entry::Reply(markdown::Content::parse(text)),
                Item::Capture(c) => Entry::Reply(markdown::Content::parse(&c.summary_markdown())),
                Item::Change(c) => Entry::Change {
                    label: format!("{} {}", if c.applied.is_new { "Created" } else { "Changed" }, c.applied.folder),
                    undone: c.undone,
                },
                Item::Error(text) => Entry::Error(text.clone()),
                Item::NewChat => Entry::NewChat,
            });
        }
        for (e, item) in self.log.iter_mut().zip(&a.log) {
            if let (Entry::Change { undone, .. }, Item::Change(c)) = (e, item) {
                *undone = c.undone;
            }
        }
        self.attached = a.attached.clone();
        self.activity = a.activity();
    }

    /// Scroll to the newest entry (the sheet opens).
    pub fn to_end(&mut self) {
        self.at_end = true;
        self.follow = 2;
    }

    /// Once per frame: whether to scroll the log to its end now.
    pub fn take_follow(&mut self) -> bool {
        let follow = self.follow > 0;
        self.follow = self.follow.saturating_sub(1);
        follow
    }

    pub fn log_id() -> widget::Id {
        widget::Id::new("chat-log")
    }

    /// The message went out.
    pub fn sent(&mut self) {
        self.draft = text_editor::Content::new();
    }

    pub fn update(&mut self, message: Message) -> Option<Event> {
        match message {
            Message::Edit(action) => self.draft.perform(action),
            Message::Send => {
                let text = self.draft.text().trim().to_string();
                if !text.is_empty() && self.activity.is_none() {
                    return Some(Event::Send(text));
                }
            }
            Message::Cancel => return Some(Event::Cancel),
            Message::Undo(i) => return Some(Event::Undo(i)),
            Message::Close => return Some(Event::Close),
            Message::Listen => return Some(Event::Listen),
            Message::StopListen => return Some(Event::StopListen),
            Message::CancelListen => return Some(Event::CancelListen),
            Message::CycleLength => return Some(Event::CycleLength),
            Message::Detach => return Some(Event::Detach),
            Message::Scrolled(v) => {
                let y = v.relative_offset().y;
                // NaN: everything fits.
                self.at_end = y.is_nan() || y > 0.98;
            }
            Message::Link => {}
        }
        None
    }

    pub fn view<'a>(&'a self, status: Status, seconds: f32) -> Element<'a, Message> {
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
                column(self.log.iter().enumerate().map(|(i, e)| entry(i, e)))
                    .spacing(14)
                    .padding(Padding { top: 14.0, bottom: 14.0, left: 16.0, right: 16.0 }),
            )
            .id(Self::log_id())
            .on_scroll(Message::Scrolled)
            .direction(theme::thin_scrollbar())
            .style(theme::paper_scrollbar)
            .into()
        };

        let listen_row: Element<'a, Message> = match status {
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
                let what = match status {
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
                if let Status::Recording { seconds, .. } = status {
                    r = r.push(sheet::small(sheet::label("Stop"), (seconds >= listen::MIN_SECONDS).then_some(Message::StopListen)));
                }
                if status != Status::Analyzing {
                    r = r.push(sheet::small(sheet::label("Cancel"), Some(Message::CancelListen)));
                }
                r.height(26).into()
            }
        };

        let mut footer = column![listen_row].spacing(7).padding(Padding { top: 10.0, right: 12.0, bottom: 12.0, left: 12.0 });
        if let Some(activity) = &self.activity {
            footer = footer.push(
                row![
                    text(activity.as_str()).size(11.5).font(theme::font::PAPER).color(theme::c().paper_ink),
                    space::horizontal(),
                    sheet::small(sheet::label("Cancel"), Some(Message::Cancel)),
                ]
                .align_y(Center),
            );
        }
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
                    .on_press_maybe(self.activity.is_none().then_some(Message::Send))
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

fn entry(index: usize, e: &Entry) -> Element<'_, Message> {
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
        Entry::Change { label, undone } => row![
            text(if *undone { format!("{label} · undone") } else { label.clone() })
                .size(11.5)
                .font(theme::font::PAPER_SEMIBOLD)
                .color(theme::c().paper_muted)
                .width(Fill),
        ]
        .push(if *undone { None } else { Some(sheet::small(sheet::label("Undo"), Some(Message::Undo(index)))) })
        .align_y(Center)
        .into(),
        Entry::NewChat => row![
            container(sheet::divider()).width(Fill),
            text("New chat").size(11).font(theme::font::PAPER_SEMIBOLD).color(theme::c().paper_muted),
            container(sheet::divider()).width(Fill),
        ]
        .spacing(8)
        .align_y(Center)
        .into(),
        Entry::Error(body) => text(body.as_str()).size(12).font(theme::font::PAPER).color(theme::c().led_clip).line_height(1.45).into(),
    }
}
