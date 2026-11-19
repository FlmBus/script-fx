//! The patch library in the sidebar: one card per patch, painted with the patch's own art and
//! type, its snapshots listed inside.

use super::{Loaded, icons, mock, theme};
use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, Gradient, Padding, Shadow, Theme, Vector,
    gradient::Linear,
    widget::{button, column, container, row, scrollable, space, text},
};
use std::collections::HashSet;

pub struct Library {
    open: HashSet<usize>,
    loaded: Option<Loaded>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Toggle(usize),
    Pick(Loaded),
    NewPatch,
    Search,
}

/// What the shell has to act on.
pub enum Event {
    Load(Loaded),
    Unload,
}

impl Library {
    pub fn new(loaded: Option<Loaded>) -> Self {
        let mut open: HashSet<usize> = [0, 1].into();
        open.extend(loaded.map(|l| l.patch));
        Self { open, loaded }
    }

    /// The shell loaded something (from here, the notch or the empty state).
    pub fn loaded_changed(&mut self, loaded: Option<Loaded>) {
        self.loaded = loaded;
        if let Some(l) = loaded {
            self.open.insert(l.patch);
        }
    }

    pub fn update(&mut self, message: Message) -> Option<Event> {
        match message {
            Message::Toggle(i) => {
                if !self.open.remove(&i) {
                    self.open.insert(i);
                }
                None
            }
            Message::Pick(loaded) => Some(Event::Load(loaded)),
            Message::NewPatch => Some(Event::Unload),
            Message::Search => None,
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let header = row![
            text("Patches").size(12.5).font(theme::font::SANS_SEMIBOLD).color(theme::FRAME_INK),
            space::horizontal(),
            icon_button(icons::SEARCH, "Search patches", Message::Search),
            icon_button(icons::PLUS, "New patch", Message::NewPatch),
        ]
        .spacing(2)
        .align_y(Center)
        .height(46)
        .padding(Padding { left: 16.0, right: 8.0, ..Padding::ZERO });

        let sections = column(mock::PATCHES.iter().enumerate().map(|(i, p)| self.section(i, p)))
            .spacing(theme::SPACE_2)
            .padding(Padding { left: 12.0, right: 8.0, bottom: 10.0, top: 0.0 });

        column![header, scrollable(sections).height(Fill)].width(theme::SIZE_SIDEBAR).into()
    }

    fn section<'a>(&self, i: usize, p: &'static mock::Patch) -> Element<'a, Message> {
        let is_open = self.open.contains(&i);
        let active = self.loaded.filter(|l| l.patch == i);

        let title = if p.title_upper { p.name.to_uppercase() } else { p.name.to_string() };
        let header = button(container(
            row![
                icons::icon_rotated(icons::CHEVRON_RIGHT, 8.0, 3.5, theme::alpha(p.ink, 0.8), if is_open { 90.0 } else { 0.0 }),
                text(title).size(12).font(p.title_font).color(p.title_color).wrapping(text::Wrapping::None),
            ]
            .spacing(8)
            .align_y(Center),
        ).center_y(Fill))
        .on_press(Message::Toggle(i))
        .width(Fill)
        .height(34)
        .padding(Padding { left: 11.0, right: 11.0, ..Padding::ZERO })
        .style(bare);

        let mut body = column![header];
        if is_open {
            let snaps = column(p.snapshots.iter().enumerate().map(|(s, name)| {
                let on = active.is_some_and(|l| l.snapshot == s);
                let dot = container(space::horizontal().width(5).height(5)).style(move |_: &Theme| container::Style {
                    background: on.then(|| p.accent.into()),
                    border: Border { radius: theme::radius(3.0), ..Default::default() },
                    shadow: if on { Shadow { color: p.accent, offset: Vector::ZERO, blur_radius: 5.0 } } else { Shadow::default() },
                    ..Default::default()
                });
                button(container(row![dot, text(*name).size(12)].spacing(7).align_y(Center)).center_y(Fill))
                    .on_press(Message::Pick(Loaded { patch: i, snapshot: s }))
                    .width(Fill)
                    .height(26)
                    .padding(Padding { left: 15.0, right: 11.0, ..Padding::ZERO })
                    .style(move |_, status| snapshot_row(p.ink, on, status))
                    .into()
            }))
            .padding(Padding { bottom: 5.0, ..Padding::ZERO });
            body = body.push(snaps);
        }

        let (a, b) = p.art;
        container(body)
            .width(Fill)
            .clip(true)
            .style(move |_: &Theme| container::Style {
                background: Some(Gradient::Linear(Linear::new(2.35).add_stop(0.0, a).add_stop(0.75, b)).into()),
                border: Border {
                    radius: theme::radius(theme::RADIUS_SECTION),
                    width: 1.0,
                    // Loaded: a ring in the patch's own accent; idle: ring-idle.
                    color: if active.is_some() { theme::alpha(p.accent, 0.8) } else { theme::FRAME_HAIRLINE },
                },
                shadow: if active.is_some() {
                    Shadow { color: theme::alpha(p.accent, 0.3), offset: Vector::new(0.0, 4.0), blur_radius: 12.0 }
                } else {
                    Shadow::default()
                },
                ..Default::default()
            })
            .into()
    }
}

fn icon_button<'a>(icon: &str, _label: &str, msg: Message) -> Element<'a, Message> {
    // TODO(a11y): iced has no aria-label equivalent yet; keep the label for later.
    button(container(icons::icon(icon, 12.0, 2.2, theme::FRAME_MUTED)).center(Fill))
        .on_press(msg)
        .width(28)
        .height(28)
        .padding(0)
        .style(|t, s| {
            let mut st = theme::frame_ghost(false)(t, s);
            st.border.radius = theme::radius(8.0);
            st
        })
        .into()
}

fn bare(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: matches!(status, button::Status::Hovered).then(|| theme::alpha(Color::WHITE, 0.05).into()),
        ..Default::default()
    }
}

fn snapshot_row(ink: Color, on: bool, status: button::Status) -> button::Style {
    let bg = if on {
        Some(theme::alpha(Color::WHITE, 0.14).into())
    } else if matches!(status, button::Status::Hovered) {
        Some(theme::alpha(Color::WHITE, 0.07).into())
    } else {
        None
    };
    button::Style {
        background: bg,
        text_color: if on { Color::WHITE } else { theme::alpha(ink, 0.75) },
        ..Default::default()
    }
}
