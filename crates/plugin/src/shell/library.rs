//! The patch library, a sheet on the left: one card per patch folder, painted in the patch's own
//! colours, its snapshots listed inside: "Init" (the patch's defaults), then the saved ones.

use std::collections::HashSet;

use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, Gradient, Padding, Shadow, Theme, Vector,
    gradient::Linear,
    widget::{button, column, container, row, scrollable, space, text},
};

use super::{brand, icons, session::PatchInfo, sheet, theme};
use crate::snapshots::INIT;

pub struct Library {
    open: HashSet<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Toggle(String),
    /// Load a patch with one of its snapshots (Init: its defaults).
    Pick(String, String),
    NewPatch,
    Close,
}

/// What the shell has to act on.
pub enum Event {
    /// Patch folder, snapshot.
    Load(String, String),
    Unload,
    Close,
}

impl Library {
    pub fn new() -> Self {
        Self { open: HashSet::new() }
    }

    /// Opens the loaded patch's card.
    pub fn reveal(&mut self, folder: &str) {
        self.open.insert(folder.to_string());
    }

    pub fn update(&mut self, message: Message) -> Option<Event> {
        match message {
            Message::Toggle(f) => {
                if !self.open.remove(&f) {
                    self.open.insert(f);
                }
                None
            }
            Message::Pick(f, snapshot) => Some(Event::Load(f, snapshot)),
            Message::NewPatch => Some(Event::Unload),
            Message::Close => Some(Event::Close),
        }
    }

    /// `loaded`: the loaded patch's folder and its selected snapshot.
    pub fn view<'a>(&'a self, patches: &'a [PatchInfo], loaded: Option<(&'a str, String)>) -> Element<'a, Message> {
        let c = theme::c();
        let small = |icon: &'static str, msg: Message| {
            button(container(icons::icon(icon, 11.0, 2.5, c.paper_ink)).center(Fill)).on_press(msg).width(28).height(26).padding(0).style(theme::paper_bare)
        };
        let header = row![
            text("Patches").size(13.5).font(theme::font::PAPER_SEMIBOLD).color(c.paper_ink),
            text(format!("· {}", patches.len())).size(12).font(theme::font::PAPER).color(c.paper_muted),
            space::horizontal(),
            small(icons::PLUS, Message::NewPatch),
            small(icons::CLOSE, Message::Close),
        ]
        .spacing(6)
        .align_y(Center)
        .height(48)
        .padding(Padding { left: 16.0, right: 8.0, ..Padding::ZERO });

        let cards = column(patches.iter().map(|p| self.card(p, loaded.as_ref().filter(|(f, _)| *f == p.folder).map(|(_, s)| s.as_str()))))
            .spacing(theme::SPACE_2)
            .padding(Padding { left: 12.0, right: 12.0, bottom: 12.0, top: 12.0 });

        sheet::frame_width(
            column![header, sheet::divider(), scrollable(cards).height(Fill).direction(theme::thin_scrollbar()).style(theme::paper_scrollbar)],
            theme::SIZE_LIBRARY,
        )
    }

    /// `selected`: the patch is loaded, with this snapshot.
    fn card<'a>(&self, p: &'a PatchInfo, selected: Option<&str>) -> Element<'a, Message> {
        let active = selected.is_some();
        let is_open = self.open.contains(&p.folder);
        let ink = p.ink.unwrap_or(theme::mix(p.accent, Color::WHITE, 0.8));
        let (accent, bg) = (p.accent, p.bg);

        // The patch's own logo, else its name in its own font (SVG/BMFont: an image) and colour.
        let colour = if p.broken { theme::c().frame_muted } else { accent };
        let title = brand::title(&p.brand, &p.name, colour, 14.0, 150.0);
        let header = button(
            container(
                row![
                    icons::icon_rotated(icons::CHEVRON_RIGHT, 8.0, 3.5, theme::alpha(ink, 0.8), if is_open { 90.0 } else { 0.0 }),
                    title,
                ]
                .spacing(8)
                .align_y(Center),
            )
            .center_y(Fill),
        )
        .on_press(Message::Toggle(p.folder.clone()))
        .width(Fill)
        .height(34)
        .padding(Padding { left: 11.0, right: 11.0, ..Padding::ZERO })
        .style(bare);

        let mut body = column![header];
        if is_open {
            let row_of = |name: &'a str, label: &'a str| {
                let on = selected == Some(name);
                let dot = container(space::horizontal().width(5).height(5)).style(move |_: &Theme| container::Style {
                    background: on.then(|| accent.into()),
                    border: Border { radius: theme::radius(3.0), ..Default::default() },
                    shadow: if on { Shadow { color: accent, offset: Vector::ZERO, blur_radius: 5.0 } } else { Shadow::default() },
                    ..Default::default()
                });
                button(container(row![dot, text(label).size(12)].spacing(7).align_y(Center)).center_y(Fill))
                    .on_press(Message::Pick(p.folder.clone(), name.to_string()))
                    .width(Fill)
                    .height(26)
                    .padding(Padding { left: 15.0, right: 11.0, ..Padding::ZERO })
                    .style(move |_, status| snapshot_row(ink, on, status))
            };
            let mut rows = column![row_of(INIT, if p.broken { "doesn't load (open it to see why)" } else { INIT })];
            if !p.broken {
                for name in &p.snapshots {
                    rows = rows.push(row_of(name, name));
                }
            }
            body = body.push(container(rows).padding(Padding { bottom: 5.0, ..Padding::ZERO }));
        }

        container(body)
            .width(Fill)
            .clip(true)
            .style(move |_: &Theme| container::Style {
                background: Some(
                    Gradient::Linear(Linear::new(2.35).add_stop(0.0, theme::mix(bg, accent, 0.28)).add_stop(0.75, bg)).into(),
                ),
                border: Border {
                    radius: theme::radius(theme::RADIUS_SECTION),
                    width: 1.0,
                    // Loaded: a ring in the patch's own accent; idle: ring-idle.
                    color: if active { theme::alpha(accent, 0.8) } else { theme::c().frame_hairline },
                },
                shadow: if active {
                    Shadow { color: theme::alpha(accent, 0.3), offset: Vector::new(0.0, 4.0), blur_radius: 12.0 }
                } else {
                    Shadow::default()
                },
                ..Default::default()
            })
            .into()
    }
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
    button::Style { background: bg, text_color: if on { Color::WHITE } else { theme::alpha(ink, 0.75) }, ..Default::default() }
}
