//! The drawer: a tab of frame material rising from the patch window's bottom edge, joined to it
//! by concave flares (the notch's mirror image). Collapsed, it hangs into the ControlPanel
//! by its handle and tab row, like the notch at the top. Open, it rises further over the panel.
//!
//! Open, it shows (see `docs/research/chain-editor.md`):
//! - the tabs: the fixed modulator set (Follow, LFO 1–3, Step), each with a grip in its colour
//!   (the tabs will be dragged onto knobs); the highlight slides between them;
//! - the open modulator: its title; the modulator itself isn't built yet.
//!
//! The patch's macros have moved to the left sidebar (`macro_rail`), so they can be tweaked in
//! the panel view too.
//!
//! Handle: open/close. A tab: open the drawer on that modulator; the active tab again closes it.

use std::time::{Duration, Instant};

use super::{icons, tabs, theme};
use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, Padding, Point, Rectangle, Renderer, Shadow, Theme, Vector,
    animation::{Animation, Easing},
    color, mouse,
    widget::{button, canvas, column, container, mouse_area, row, space, stack, text},
};

/// The fixed modulator set in tab order, each with its colour (fixed, never a patch colour).
pub const MODULATORS: [(&str, Color); 5] = [
    ("Follow", color!(0x60a5fa)),
    ("LFO 1", color!(0x2dd4bf)),
    ("LFO 2", color!(0xf472b6)),
    ("LFO 3", color!(0xa78bfa)),
    ("Step", color!(0xa3e635)),
];

pub struct Drawer {
    open: bool,
    /// Open/close animation (the height between the strip and the open drawer).
    slide: Animation<bool>,
    /// Index into [`MODULATORS`] of the active tab.
    selected: usize,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The handle: open or close.
    Toggle,
    /// A tab: open on that modulator (or close, if it's the active tab of the open drawer).
    Select(usize),
}

/// The open drawer's body under the tab row: the modulator's title and note.
const BODY_HEIGHT: f32 = 120.0;

impl Drawer {
    pub fn new() -> Self {
        Self {
            open: false,
            slide: Animation::new(false).duration(Duration::from_millis(260)).easing(Easing::EaseOutCubic),
            selected: 1,
        }
    }

    pub fn update(&mut self, message: Message, now: Instant) {
        match message {
            Message::Toggle => self.set_open(!self.open, now),
            Message::Select(i) if self.open && i == self.selected => self.set_open(false, now),
            Message::Select(i) => {
                self.selected = i;
                self.set_open(true, now);
            }
        }
    }

    pub fn set_open(&mut self, open: bool, now: Instant) {
        if self.open != open {
            self.open = open;
            self.slide.go_mut(open, now);
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The drawer at its current height, `drawer_width` wide plus the flares.
    pub fn view<'a>(&'a self, now: Instant) -> Element<'a, Message> {
        let t: f32 = self.slide.interpolate(0.0, 1.0, now);
        let height = theme::SIZE_DRAWER_BAR + t * (theme::SIZE_DRAWER_OPEN - theme::SIZE_DRAWER_BAR);
        let width = theme::drawer_width();
        let r = theme::RADIUS_WINDOW;

        // The body sits under the tabs at its full height; the drawer's height reveals it.
        let body = container(self.modulator()).height(BODY_HEIGHT);
        let content = container(
            column![handle(), self.tabs(), space::vertical().height(12), body]
                .padding(Padding { top: 0.0, right: 12.0, bottom: 2.0, left: 12.0 }),
        )
        .width(width)
        .height(height)
        .clip(true);

        // Like the notch: one canvas draws the silhouette (body and flares as one path, no seams
        // at fractional scales), a body-sized quad under it casts the shadow (upwards).
        let shadow = row![
            space::horizontal().width(r),
            container(space::horizontal().width(Fill).height(Fill)).width(Fill).height(Fill).style(|_: &Theme| container::Style {
                background: Some(theme::c().frame.into()),
                border: Border { radius: theme::radius(0.0).top(theme::RADIUS_NOTCH), ..Default::default() },
                shadow: Shadow { color: theme::shadow(0.45), offset: Vector::new(0.0, -10.0), blur_radius: 24.0 },
                ..Default::default()
            }),
            space::horizontal().width(r),
        ];
        // Explicit size: the layers under the content are `Fill` and must not grow the stack.
        let drawer = stack![row![space::horizontal().width(r), content, space::horizontal().width(r)]]
            .push_under(canvas(Silhouette).width(Fill).height(Fill))
            .push_under(shadow)
            .width(width + 2.0 * r)
            .height(height);

        // `Idle` marks the drawer as occupied: clicks on it don't fall through to the panel
        // (a click on the panel closes the drawer).
        mouse_area(drawer).interaction(mouse::Interaction::Idle).into()
    }

    /// The modulator tabs, full width, in a recessed track. The highlight slides between them
    /// while the drawer is open; closed, no tab is selected (like the notch's switch).
    fn tabs(&self) -> Element<'_, Message> {
        let tab = |(i, (name, colour)): (usize, &(&'static str, Color)), on: bool| -> Element<'_, Message> {
            let font = if on { theme::font::SANS_SEMIBOLD } else { theme::font::SANS_MEDIUM };
            button(container(row![icons::icon(icons::GRIP, 12.0, 0.0, *colour), text(*name).size(11).font(font)].spacing(5).align_y(Center)).center(Fill))
                .on_press(Message::Select(i))
                .width(Fill)
                .height(24)
                .padding(0)
                .style(tabs::style(on, theme::frame_tab(on)))
                .into()
        };
        let off = row(MODULATORS.iter().enumerate().map(|m| tab(m, false))).spacing(1);
        let on = row(MODULATORS.iter().enumerate().map(|m| tab(m, true))).spacing(1);
        let selected = self.open.then_some(self.selected);
        let pill = tabs::Pill {
            background: theme::alpha(theme::c().frame_ink, 0.12),
            radius: theme::RADIUS_BUTTON,
            shadow: Shadow { color: theme::shadow(0.4), offset: Vector::new(0.0, 1.0), blur_radius: 3.0 },
        };
        container(tabs::tabs(off, on, selected, pill))
            .width(Fill)
            .padding(2)
            .style(theme::frame_track)
            .into()
    }
}

impl Drawer {
    /// The open modulator: its title; the controls come with the modulation engine.
    fn modulator(&self) -> Element<'_, Message> {
        let (name, colour) = MODULATORS[self.selected];
        let kind = match name {
            "Follow" => "Envelope follower",
            "Step" => "Step sequencer",
            _ => "LFO",
        };
        let note = column![
            icons::icon(icons::GRIP, 14.0, 0.0, theme::alpha(colour, 0.6)),
            text("Not built yet").size(12).font(theme::font::SANS_MEDIUM).color(theme::c().frame_ink_2),
            text("comes with the modulation engine").size(11).color(theme::c().frame_muted),
        ]
        .spacing(6)
        .align_x(Center);
        column![title(format!("{name} · {kind}")), container(note).center(Fill)].spacing(8).width(Fill).into()
    }
}

/// A column title: small, muted, centred.
fn title<'a>(s: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    container(text(s).size(11).font(theme::font::SANS_MEDIUM).color(theme::c().frame_muted)).center_x(Fill).height(16).into()
}

/// The grab handle: a short bar centred at the top, the whole strip clickable.
fn handle<'a>() -> Element<'a, Message> {
    let bar = container(space::horizontal().width(36).height(4)).style(|_: &Theme| container::Style {
        background: Some(theme::alpha(theme::c().frame_ink, 0.22).into()),
        border: Border { radius: theme::radius(2.0), ..Default::default() },
        ..Default::default()
    });
    button(container(bar).center_x(Fill).padding(Padding { top: 4.0, ..Padding::ZERO }))
        .on_press(Message::Toggle)
        .width(Fill)
        .height(12)
        .padding(0)
        .style(|_: &Theme, _| button::Style::default())
        .into()
}

// ---- silhouette ----------------------------------------------------------------------------------

/// The drawer's outline in frame colour: concave flares out of the window's bottom edge on both
/// sides (radius-window), the body with rounded top corners (radius-notch). The notch's path,
/// upside down.
struct Silhouette;

impl canvas::Program<Message> for Silhouette {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let (w, h) = (bounds.width, bounds.height);
        let r = theme::RADIUS_WINDOW;
        let rb = theme::RADIUS_NOTCH.min((w - 2.0 * r) / 2.0).min(h);
        let k = 0.5523; // quarter circles as cubic Béziers
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let pt = |x: f32, y: f32| Point::new(x, h - y); // the notch's coordinates, flipped
        let path = canvas::Path::new(|p| {
            p.move_to(pt(0.0, 0.0));
            p.line_to(pt(w, 0.0));
            p.bezier_curve_to(pt(w - k * r, 0.0), pt(w - r, r - k * r), pt(w - r, r));
            p.line_to(pt(w - r, h - rb));
            p.bezier_curve_to(pt(w - r, h - rb + k * rb), pt(w - r - rb + k * rb, h), pt(w - r - rb, h));
            p.line_to(pt(r + rb, h));
            p.bezier_curve_to(pt(r + rb - k * rb, h), pt(r, h - rb + k * rb), pt(r, h - rb));
            p.line_to(pt(r, r));
            p.bezier_curve_to(pt(r, r - k * r), pt(k * r, 0.0), pt(0.0, 0.0));
            p.close();
        });
        frame.fill(&path, theme::c().frame);
        vec![frame.into_geometry()]
    }
}
