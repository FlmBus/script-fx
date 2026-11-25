//! The snapshot menu, a paper card under the notch (its selector opens it): Init and the saved
//! snapshots (click = recall), then Save as new…, Rename…, Revert, Export…, Delete. Naming and
//! deleting happen inside the card (a name field; a confirmation). A press elsewhere or Escape
//! closes it. The shell does the work (`Session::recall`, `save_snapshot`, …); this holds the
//! card's state.

use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding, Shadow, Theme, Vector,
    mouse,
    widget::{self, button, column, container, mouse_area, row, scrollable, space, text, text_input},
};

use super::theme;
use crate::snapshots::INIT;

pub const WIDTH: f32 = 248.0;

pub struct Menu {
    pub mode: Mode,
    /// The last action failed (shown at the card's bottom until the next action).
    pub error: Option<String>,
}

pub enum Mode {
    List,
    Name { purpose: Purpose, text: String },
    ConfirmDelete(String),
}

#[derive(Clone)]
pub enum Purpose {
    SaveAs,
    Rename(String),
}

#[derive(Debug, Clone)]
pub enum Message {
    Recall(String),
    SaveAs,
    Rename,
    Delete,
    ConfirmDelete,
    Revert,
    Export,
    Edit(String),
    Submit,
    /// Back to the list (from naming or the delete confirmation).
    Back,
}

pub fn input_id() -> widget::Id {
    widget::Id::new("snapshot-name")
}

impl Menu {
    pub fn list() -> Self {
        Self { mode: Mode::List, error: None }
    }

    /// Straight into naming (Save on Init).
    pub fn naming(purpose: Purpose, text: String) -> Self {
        Self { mode: Mode::Name { purpose, text }, error: None }
    }

    /// `cycle`: Init and the saved ones; `current`: the selected one.
    pub fn view<'a>(&'a self, cycle: Vec<String>, current: &str, modified: bool) -> Element<'a, Message> {
        let c = theme::c();
        let body: Element<'a, Message> = match &self.mode {
            Mode::List => {
                let rows_n = cycle.len();
                let rows = column(cycle.into_iter().map(|name| { let on = name == current; snapshot_row(name, on) })).spacing(1);
                let is_init = current == INIT;
                let actions = column![
                    action("Save as new…", Some(Message::SaveAs)),
                    action("Rename…", (!is_init).then_some(Message::Rename)),
                    action(if is_init { "Revert to Init" } else { "Revert to saved" }, modified.then_some(Message::Revert)),
                    action("Export…", Some(Message::Export)),
                    action("Delete…", (!is_init).then_some(Message::Delete)),
                ]
                .spacing(1);
                column![
                    title("Snapshots"),
                    scrollable(rows).height(iced_height(rows_n)).direction(theme::thin_scrollbar()).style(theme::paper_scrollbar),
                    container(space::horizontal().height(1)).width(Fill).style(theme::rule(c.paper_line)),
                    actions,
                ]
                .spacing(6)
                .into()
            }
            Mode::Name { purpose, text: typed } => {
                let heading = match purpose {
                    Purpose::SaveAs => "Save as a new snapshot".to_string(),
                    Purpose::Rename(from) => format!("Rename “{from}”"),
                };
                let invalid = self.error.is_some();
                column![
                    title_owned(heading),
                    text_input("Name", typed)
                        .id(input_id())
                        .on_input(Message::Edit)
                        .on_submit(Message::Submit)
                        .size(12.5)
                        .font(theme::font::PAPER)
                        .padding(Padding { top: 6.0, bottom: 6.0, left: 9.0, right: 9.0 })
                        .style(move |_: &Theme, status| {
                            let focused = matches!(status, text_input::Status::Focused { .. });
                            text_input::Style {
                                background: c.paper_raised.into(),
                                border: Border {
                                    radius: theme::radius(theme::RADIUS_BUTTON),
                                    width: 1.0,
                                    color: if invalid { c.led_clip } else if focused { c.paper_muted } else { c.paper_line },
                                },
                                icon: c.paper_muted,
                                placeholder: c.paper_muted,
                                value: c.paper_ink,
                                selection: theme::alpha(c.signal, 0.3),
                            }
                        }),
                    row![
                        text("enter to save · esc to go back").size(11).font(theme::font::PAPER).color(c.paper_muted),
                        space::horizontal(),
                        small("Save", Message::Submit),
                    ]
                    .align_y(Center),
                ]
                .spacing(8)
                .into()
            }
            Mode::ConfirmDelete(name) => column![
                title_owned(format!("Delete “{name}”?")),
                text("The file is removed; the sound you hear stays.").size(11).font(theme::font::PAPER).color(c.paper_muted),
                row![space::horizontal(), small("Cancel", Message::Back), small("Delete", Message::ConfirmDelete)].spacing(4),
            ]
            .spacing(8)
            .into(),
        };
        let mut content = column![body].spacing(8);
        if let Some(e) = &self.error {
            content = content.push(text(e.as_str()).size(11).font(theme::font::PAPER).color(c.led_clip));
        }
        let card = container(content).width(WIDTH).padding(10).style(|_: &Theme| container::Style {
            background: Some(theme::c().paper.into()),
            border: Border { radius: theme::radius(theme::RADIUS_FIELD), width: 1.0, color: theme::c().paper_edge },
            shadow: Shadow { color: theme::shadow(0.45), offset: Vector::new(0.0, 10.0), blur_radius: 28.0 },
            ..Default::default()
        });
        // Occupies the pointer like a sheet: a press on the card isn't a press on the page.
        mouse_area(card).interaction(mouse::Interaction::Idle).into()
    }
}

/// The list shows up to eight rows, then scrolls.
fn iced_height(rows: usize) -> f32 {
    rows.min(8) as f32 * (ROW + 1.0)
}

const ROW: f32 = 26.0;

fn title<'a>(t: &'static str) -> Element<'a, Message> {
    text(t).size(11).font(theme::font::PAPER_SEMIBOLD).color(theme::c().paper_muted).into()
}

fn title_owned<'a>(t: String) -> Element<'a, Message> {
    text(t).size(12.5).font(theme::font::PAPER_SEMIBOLD).color(theme::c().paper_ink).into()
}

fn snapshot_row<'a>(name: String, on: bool) -> Element<'a, Message> {
    let c = theme::c();
    let dot = container(space::horizontal().width(5).height(5)).style(move |_: &Theme| container::Style {
        background: on.then(|| c.paper_ink.into()),
        border: Border { radius: theme::radius(3.0), ..Default::default() },
        ..Default::default()
    });
    button(container(row![dot, text(name.clone()).size(12).font(theme::font::PAPER)].spacing(8).align_y(Center)).center_y(Fill))
        .on_press(Message::Recall(name))
        .width(Fill)
        .height(ROW)
        .padding(Padding { left: 8.0, right: 8.0, ..Padding::ZERO })
        .style(move |_: &Theme, status| menu_row(on, true, status))
        .into()
}

fn action<'a>(label: &'static str, msg: Option<Message>) -> Element<'a, Message> {
    let enabled = msg.is_some();
    button(container(text(label).size(12).font(theme::font::PAPER)).center_y(Fill))
        .on_press_maybe(msg)
        .width(Fill)
        .height(ROW)
        .padding(Padding { left: 21.0, right: 8.0, ..Padding::ZERO })
        .style(move |_: &Theme, status| menu_row(false, enabled, status))
        .into()
}

fn small<'a>(label: &'static str, msg: Message) -> Element<'a, Message> {
    button(text(label).size(11.5).font(theme::font::PAPER)).on_press(msg).padding([3, 10]).style(theme::paper_small).into()
}

fn menu_row(on: bool, enabled: bool, status: button::Status) -> button::Style {
    let c = theme::c();
    let hover = enabled && matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: if on {
            Some(theme::alpha(c.paper_ink, 0.08).into())
        } else if hover {
            Some(theme::alpha(c.paper_ink, 0.05).into())
        } else {
            None
        },
        text_color: if enabled { c.paper_ink } else { theme::alpha(c.paper_muted, 0.7) },
        border: Border { radius: theme::radius(theme::RADIUS_BUTTON - 2.0), ..Default::default() },
        ..Default::default()
    }
}
