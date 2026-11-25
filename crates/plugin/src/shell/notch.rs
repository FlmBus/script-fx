//! The control notch: a tab of frame material hanging from the window's top edge, joined to
//! it by concave flares. Two rows:
//! - the surface switcher, full width: Surface | Chain | Matrix (what the viewport shows);
//! - reload (+ sync LED) | snapshot selector, save, load.
//!
//! The sheet openers (controls, chat, settings) live in the top bar. Stateless: the shell owns
//! everything it shows. Snapshots are not built yet (the selector shows "Init", Save and Load
//! are off).

use super::viewport::{self, SurfaceId};
use super::{icons, tabs, theme};
use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding, Point, Rectangle, Renderer, Shadow, Theme, Vector,
    mouse,
    widget::{button, canvas, column, container, row, space, stack, text},
};

#[derive(Debug, Clone)]
pub enum Message {
    Reload,
    /// Switch the viewport to another surface.
    Surface(SurfaceId),
}

/// The patch file's state, shown by the LED on Reload.
#[derive(Clone, Copy, PartialEq)]
pub enum Sync {
    /// Loaded and up to date (hot reload watches the files).
    Ok,
    /// The last (re)load failed; the previous version keeps running.
    Error,
}

pub struct Props<'a> {
    /// The surfaces to offer, in switcher order, and which one shows.
    pub surfaces: Vec<viewport::Entry<'a>>,
    pub current: SurfaceId,
    pub snapshot: &'a str,
    pub sync: Sync,
}

pub fn view<'a>(p: Props<'a>) -> Element<'a, Message> {
    let ink = theme::c().frame_ink_2;
    let off = theme::alpha(ink, 0.35);

    // Reload, with the sync LED next to its icon.
    let led = container(space::horizontal().width(5).height(5)).style(move |_: &Theme| container::Style {
        background: Some(if p.sync == Sync::Ok { theme::c().led_ok } else { theme::c().led_clip }.into()),
        border: Border { radius: theme::radius(3.0), ..Default::default() },
        ..Default::default()
    });
    let reload = square(row![icons::icon(icons::RELOAD, 13.0, 2.2, ink), led].spacing(4).align_y(Center), Some(Message::Reload), false);

    // Snapshots: selector, save, load (disabled until snapshots exist).
    let selector = button(
        container(
            row![
                text(p.snapshot).size(12).font(theme::font::SANS_MEDIUM).color(theme::c().frame_ink),
                space::horizontal(),
                icons::icon(icons::CHEVRON_DOWN, 10.0, 2.5, theme::c().frame_muted),
            ]
            .align_y(Center),
        )
        .center_y(Fill),
    )
    .width(SELECTOR_WIDTH)
    .height(theme::SIZE_CONTROL)
    .padding(Padding { left: 12.0, right: 10.0, ..Padding::ZERO })
    .style(theme::frame_ghost(true));
    let save = square(icons::icon(icons::SAVE, 13.0, 2.0, off), None, false);
    let load = square(icons::icon(icons::FOLDER, 13.0, 2.0, off), None, false);

    let tools = row![
        reload,
        divider(),
        row![selector, save, load].spacing(theme::SPACE_HAIR),
    ]
    .spacing(theme::SPACE_2)
    .align_y(Center);

    // The view switch spans the width of the row below it.
    let controls = column![surface_switch(&p.surfaces, p.current), tools].spacing(6);

    // Three layers, so the notch has no seams at fractional UI scales (separate flare and body
    // widgets each antialiased the shared edge, and the patch showed through as a thin line):
    // the controls set the size; under them one canvas draws the whole silhouette (flares and
    // body as a single path); under that, a body-sized quad casts the shadow.
    let r = theme::RADIUS_WINDOW;
    let content = row![
        space::horizontal().width(r),
        container(controls).padding(Padding { top: 4.0, right: 8.0, bottom: 6.0, left: 8.0 }),
        space::horizontal().width(r),
    ];
    let shadow = row![
        space::horizontal().width(r),
        container(space::horizontal().width(Fill).height(Fill)).width(Fill).height(Fill).style(|_: &Theme| container::Style {
            background: Some(theme::c().frame.into()),
            border: Border { radius: theme::radius(0.0).bottom(theme::RADIUS_NOTCH), ..Default::default() },
            // shadow-notch: 0 10px 24px -6px rgba(0,0,0,0.45)
            shadow: Shadow { color: theme::shadow(0.45), offset: Vector::new(0.0, 10.0), blur_radius: 24.0 },
            ..Default::default()
        }),
        space::horizontal().width(r),
    ];

    stack![content]
        .push_under(canvas(Silhouette).width(Fill).height(Fill))
        .push_under(shadow)
        .into()
}

const SELECTOR_WIDTH: f32 = 168.0;

/// The notch's size, flares included (where the shell's blur leaves it sharp).
pub fn size() -> (f32, f32) {
    (TOOLS_WIDTH + 2.0 * 8.0 + 2.0 * theme::RADIUS_WINDOW, 4.0 + (24.0 + 4.0) + 6.0 + theme::SIZE_CONTROL + 6.0)
}

/// The width of the tools row (reload | selector, save, load); the surface switcher matches it.
const TOOLS_WIDTH: f32 =
    theme::SIZE_CONTROL + 2.0 * theme::SPACE_2 + 1.0 + SELECTOR_WIDTH + 2.0 * theme::SPACE_HAIR + 2.0 * theme::SIZE_CONTROL;

/// The surface switcher (Surface | Chain | Matrix, plus any the patch adds) in a recessed track;
/// the highlight slides between them.
fn surface_switch<'a>(entries: &[viewport::Entry<'a>], current: SurfaceId) -> Element<'a, Message> {
    let segment = |e: viewport::Entry<'a>, on: bool| -> Element<'a, Message> {
        button(container(text(e.label).size(12).font(if on { theme::font::SANS_SEMIBOLD } else { theme::font::SANS })).center(Fill))
            .on_press_maybe((e.id != current).then_some(Message::Surface(e.id)))
            .width(Fill)
            .height(24)
            .padding(0)
            .style(tabs::style(on, theme::frame_switch(on)))
            .into()
    };
    let off = row(entries.iter().map(|e| segment(*e, false))).spacing(2);
    let on = row(entries.iter().map(|e| segment(*e, true))).spacing(2);
    let selected = entries.iter().position(|e| e.id == current);
    let pill = tabs::Pill {
        background: theme::c().frame_invert,
        radius: theme::RADIUS_BUTTON,
        shadow: Shadow::default(),
    };
    container(tabs::tabs(off, on, selected, pill))
        .width(TOOLS_WIDTH)
        .padding(2)
        .style(theme::frame_track)
        .into()
}

/// A square icon button; `None` = disabled. `active`: highlighted (its sheet is open).
fn square<'a>(content: impl Into<Element<'a, Message>>, msg: Option<Message>, active: bool) -> Element<'a, Message> {
    let b = button(container(content).center(Fill))
        .on_press_maybe(msg)
        .width(theme::SIZE_CONTROL)
        .height(theme::SIZE_CONTROL)
        .padding(0);
    if active { b.style(theme::frame_primary) } else { b.style(theme::frame_ghost(false)) }.into()
}

/// A short vertical hairline between control groups.
fn divider<'a>() -> Element<'a, Message> {
    container(space::vertical().width(1).height(16)).style(theme::rule(theme::c().frame_hairline_strong)).into()
}

// ---- silhouette ----------------------------------------------------------------------------------

/// The notch's outline in frame colour: concave flares into the window's top edge on both sides
/// (radius-window), the body with rounded bottom corners (radius-notch).
struct Silhouette;

impl canvas::Program<Message> for Silhouette {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let (w, h) = (bounds.width, bounds.height);
        let r = theme::RADIUS_WINDOW;
        let rb = theme::RADIUS_NOTCH.min((w - 2.0 * r) / 2.0).min(h);
        // Quarter circles as cubic Béziers (k = 0.5523): unambiguous direction, unlike arcs.
        let k = 0.5523;
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let path = canvas::Path::new(|p| {
            let pt = Point::new;
            p.move_to(pt(0.0, 0.0));
            p.line_to(pt(w, 0.0));
            // right flare: from the top edge curving down into the body's right side
            p.bezier_curve_to(pt(w - k * r, 0.0), pt(w - r, r - k * r), pt(w - r, r));
            p.line_to(pt(w - r, h - rb));
            p.bezier_curve_to(pt(w - r, h - rb + k * rb), pt(w - r - rb + k * rb, h), pt(w - r - rb, h));
            p.line_to(pt(r + rb, h));
            p.bezier_curve_to(pt(r + rb - k * rb, h), pt(r, h - rb + k * rb), pt(r, h - rb));
            p.line_to(pt(r, r));
            // left flare: back up to the top edge
            p.bezier_curve_to(pt(r, r - k * r), pt(k * r, 0.0), pt(0.0, 0.0));
            p.close();
        });
        frame.fill(&path, theme::c().frame);
        vec![frame.into_geometry()]
    }
}
