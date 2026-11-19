//! The patch library, a sheet on the left: one card per patch folder, painted in the patch's own
//! colours, its snapshots listed inside. Snapshots don't exist yet, so every patch lists one:
//! "Init" (the patch's defaults).

use std::collections::HashSet;

use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, Gradient, Padding, Shadow, Theme, Vector,
    gradient::Linear,
    widget::{button, column, container, image, row, scrollable, space, svg, text},
};

use super::brand::Logo;
use super::{icons, session::PatchInfo, sheet, theme};

pub struct Library {
    open: HashSet<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Toggle(String),
    /// Load a patch with its defaults.
    Pick(String),
    NewPatch,
    Close,
}

/// What the shell has to act on.
pub enum Event {
    Load(String),
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
            Message::Pick(f) => Some(Event::Load(f)),
            Message::NewPatch => Some(Event::Unload),
            Message::Close => Some(Event::Close),
        }
    }

    pub fn view<'a>(&'a self, patches: &'a [PatchInfo], loaded: Option<&'a str>) -> Element<'a, Message> {
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

        let cards = column(patches.iter().map(|p| self.card(p, loaded == Some(p.folder.as_str()))))
            .spacing(theme::SPACE_2)
            .padding(Padding { left: 12.0, right: 12.0, bottom: 12.0, top: 12.0 });

        sheet::frame_width(
            column![header, sheet::divider(), scrollable(cards).height(Fill).direction(theme::thin_scrollbar()).style(theme::paper_scrollbar)],
            theme::SIZE_LIBRARY,
        )
    }

    fn card<'a>(&self, p: &'a PatchInfo, active: bool) -> Element<'a, Message> {
        let is_open = self.open.contains(&p.folder);
        let ink = p.ink.unwrap_or(theme::mix(p.accent, Color::WHITE, 0.8));
        let (accent, bg) = (p.accent, p.bg);

        // The patch's own logo, else its name in its own font (SVG/BMFont: an image) and colour.
        let title: Element<'a, Message> = match (&p.brand.logo, &p.brand.title) {
            (None, Some((handle, w, h))) => {
                let k = (150.0 / w).min(1.0);
                image(handle.clone()).width(w * k).height(h * k).into()
            }
            _ => match &p.brand.logo {
            Some((logo, aspect)) => {
                let (h, max_w) = (14.0, 150.0);
                let w = (h * aspect).min(max_w);
                match logo {
                    Logo::Svg(handle) => svg(handle.clone()).width(w).height(w / aspect).into(),
                    Logo::Image(handle) => image(handle.clone()).width(w).height(w / aspect).into(),
                }
            }
            None => text(&p.name)
                .size(if p.brand.font.is_some() { 13 } else { 12 })
                .font(p.brand.font.unwrap_or(theme::font::SANS_SEMIBOLD))
                .color(if p.broken { theme::c().frame_muted } else { accent })
                .wrapping(text::Wrapping::None)
                .into(),
            },
        };
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
            let label = if p.broken { "doesn't load (open it to see why)" } else { "Init" };
            let dot = container(space::horizontal().width(5).height(5)).style(move |_: &Theme| container::Style {
                background: active.then(|| accent.into()),
                border: Border { radius: theme::radius(3.0), ..Default::default() },
                shadow: if active { Shadow { color: accent, offset: Vector::ZERO, blur_radius: 5.0 } } else { Shadow::default() },
                ..Default::default()
            });
            let snapshot = button(container(row![dot, text(label).size(12)].spacing(7).align_y(Center)).center_y(Fill))
                .on_press(Message::Pick(p.folder.clone()))
                .width(Fill)
                .height(26)
                .padding(Padding { left: 15.0, right: 11.0, ..Padding::ZERO })
                .style(move |_, status| snapshot_row(ink, active, status));
            body = body.push(container(snapshot).padding(Padding { bottom: 5.0, ..Padding::ZERO }));
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
