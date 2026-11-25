//! The control notch: a tab of frame material hanging from the window's top edge, joined to
//! it by concave flares. One row: reload (+ sync LED) | snapshot selector, save, load | bypass.
//!
//! The view switch (Surface · Chain · Matrix) floats in the stage's top-right corner as three
//! icons ([`view_switch`]); the sheet openers (controls, chat, settings) live in the top bar.
//! Stateless: the shell owns everything it shows. Snapshots: ‹ › step through Init and the saved
//! ones, the selector opens the snapshot menu (`snapshot_menu`), Save stores the sound (a name is
//! asked for on Init), Load imports a snapshot file.

use super::{icons, tabs, theme};
use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding, Point, Rectangle, Renderer, Shadow, Theme, Vector,
    mouse,
    widget::{button, canvas, container, row, space, stack, text},
};

#[derive(Debug, Clone)]
pub enum Message {
    Reload,
    View(View),
    /// The bypass button: the plugin's (host) bypass on or off.
    Bypass(bool),
    /// ‹ ›: the previous / next snapshot.
    Step(bool),
    /// The selector: open or close the snapshot menu.
    Menu,
    Save,
    /// Load: import a snapshot file.
    Import,
}

/// What the patch window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The patch's ControlPanel, or without one its macros (`surface`).
    Surface,
    /// The audio tree (blocks, containers with slots).
    Chain,
    /// The patch's existing macro-to-parameter modulation slots.
    Matrix,
}

impl View {
    const ALL: [View; 3] = [View::Surface, View::Chain, View::Matrix];

    fn label(self) -> &'static str {
        match self {
            View::Surface => "Surface",
            View::Chain => "Chain",
            View::Matrix => "Matrix",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            View::Surface => icons::VIEW_SURFACE,
            View::Chain => icons::VIEW_CHAIN,
            View::Matrix => icons::VIEW_MATRIX,
        }
    }
}

/// The patch file's state, shown by the LED on Reload.
#[derive(Clone, Copy, PartialEq)]
pub enum Sync {
    /// Loaded and up to date (hot reload watches the files).
    Ok,
    /// The last (re)load failed; the previous version keeps running.
    Error,
}

pub struct Props {
    pub snapshot: String,
    /// The sound differs from the snapshot (a dot after its name).
    pub modified: bool,
    /// There is more than Init to step through.
    pub can_step: bool,
    pub menu_open: bool,
    pub sync: Sync,
    /// The plugin is bypassed (the host's bypass parameter).
    pub bypassed: bool,
}

pub fn view<'a>(p: Props) -> Element<'a, Message> {
    let ink = theme::c().frame_ink_2;
    let off = theme::alpha(ink, 0.35);

    // Reload, with the sync LED next to its icon.
    let led = container(space::horizontal().width(5).height(5)).style(move |_: &Theme| container::Style {
        background: Some(if p.sync == Sync::Ok { theme::c().led_ok } else { theme::c().led_clip }.into()),
        border: Border { radius: theme::radius(3.0), ..Default::default() },
        ..Default::default()
    });
    let reload = square(row![icons::icon(icons::RELOAD, 13.0, 2.2, ink), led].spacing(4).align_y(Center), Some(Message::Reload), false);

    // Snapshots: ‹ selector ›, save, load.
    let arrow = |forward: bool| {
        let ink = if p.can_step { ink } else { off };
        button(container(icons::icon_rotated(icons::CHEVRON_RIGHT, 8.0, 3.0, ink, if forward { 0.0 } else { 180.0 })).center(Fill))
            .on_press_maybe(p.can_step.then_some(Message::Step(forward)))
            .width(ARROW_WIDTH)
            .height(theme::SIZE_CONTROL)
            .padding(0)
            .style(theme::frame_ghost(false))
    };
    let mut name = row![text(p.snapshot.clone()).size(12).font(theme::font::SANS_MEDIUM).color(theme::c().frame_ink).wrapping(text::Wrapping::None)]
        .spacing(6)
        .align_y(Center)
        .clip(true);
    if p.modified {
        name = name.push(container(space::horizontal().width(5).height(5)).style(|_: &Theme| container::Style {
            background: Some(theme::c().frame_ink_2.into()),
            border: Border { radius: theme::radius(3.0), ..Default::default() },
            ..Default::default()
        }));
    }
    let selector = button(
        container(
            row![
                name,
                space::horizontal(),
                icons::icon_rotated(icons::CHEVRON_DOWN, 10.0, 2.5, theme::c().frame_muted, if p.menu_open { 180.0 } else { 0.0 }),
            ]
            .spacing(6)
            .align_y(Center),
        )
        .center_y(Fill),
    )
    .on_press(Message::Menu)
    .width(SELECTOR_WIDTH)
    .height(theme::SIZE_CONTROL)
    .padding(Padding { left: 12.0, right: 10.0, ..Padding::ZERO })
    .style(theme::frame_ghost(true));
    let selector = tooltip(selector, if p.modified { "Changed since saved" } else { "Snapshots" });
    let save = tooltip(square(icons::icon(icons::SAVE, 13.0, 2.0, ink), Some(Message::Save), false), "Save snapshot");
    let load = tooltip(square(icons::icon(icons::FOLDER, 13.0, 2.0, ink), Some(Message::Import), false), "Load snapshot file");

    // Bypass: lit while the effect runs; bypassed, the button turns amber.
    let bypass = {
        let (ink, msg) = if p.bypassed { (theme::c().led_reduce, Message::Bypass(false)) } else { (ink, Message::Bypass(true)) };
        let b = button(container(icons::icon(icons::POWER, 13.0, 2.2, ink)).center(Fill))
            .on_press(msg)
            .width(theme::SIZE_CONTROL)
            .height(theme::SIZE_CONTROL)
            .padding(0);
        let b = if p.bypassed {
            b.style(|t: &Theme, s| button::Style {
                background: Some(theme::alpha(theme::c().led_reduce, 0.16).into()),
                border: Border { radius: theme::radius(theme::RADIUS_BUTTON), width: 1.0, color: theme::alpha(theme::c().led_reduce, 0.5) },
                ..theme::frame_ghost(false)(t, s)
            })
        } else {
            b.style(theme::frame_ghost(false))
        };
        tooltip(b, if p.bypassed { "Bypassed: click to turn the effect on" } else { "Bypass" })
    };

    let controls = row![
        reload,
        divider(),
        row![arrow(false), selector, arrow(true), save, load].spacing(theme::SPACE_HAIR).align_y(Center),
        divider(),
        bypass,
    ]
    .spacing(theme::SPACE_2)
    .align_y(Center);

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

const SELECTOR_WIDTH: f32 = 156.0;
const ARROW_WIDTH: f32 = 22.0;

/// The notch's size, flares included (where the shell's blur leaves it sharp).
pub fn size() -> (f32, f32) {
    (TOOLS_WIDTH + 2.0 * 8.0 + 2.0 * theme::RADIUS_WINDOW, 4.0 + theme::SIZE_CONTROL + 6.0)
}

/// The width of the row (reload | ‹ selector ›, save, load | bypass).
const TOOLS_WIDTH: f32 = theme::SIZE_CONTROL
    + 4.0 * theme::SPACE_2
    + 2.0
    + 2.0 * ARROW_WIDTH
    + SELECTOR_WIDTH
    + 4.0 * theme::SPACE_HAIR
    + 3.0 * theme::SIZE_CONTROL;

/// Surface · Chain · Matrix as three icons in a small frame-coloured group (floating in the
/// stage's top-right corner); the highlight slides between them, the name shows on hover.
pub fn view_switch<'a>(current: View) -> Element<'a, Message> {
    let segment = |v: View, on: bool| -> Element<'a, Message> {
        let ink = if on { theme::c().on_frame_invert } else { theme::c().frame_ink_2 };
        let b = button(container(icons::icon(v.icon(), 14.0, 2.0, ink)).center(Fill))
            .on_press_maybe((v != current).then_some(Message::View(v)))
            .width(30)
            .height(26)
            .padding(0)
            .style(tabs::style(on, theme::frame_switch(on)));
        // The labels only on the layer under the highlight's (one tooltip per button).
        if on { b.into() } else { tooltip(b, v.label()) }
    };
    let off = row(View::ALL.into_iter().map(|v| segment(v, false))).spacing(2);
    let on = row(View::ALL.into_iter().map(|v| segment(v, true))).spacing(2);
    let selected = View::ALL.iter().position(|v| *v == current);
    let pill = tabs::Pill {
        background: theme::c().frame_invert,
        radius: 7.0,
        shadow: Shadow::default(),
    };
    container(tabs::tabs(off, on, selected, pill))
        .padding(3)
        .style(|_: &Theme| container::Style {
            background: Some(theme::c().frame.into()),
            border: Border { radius: theme::radius(10.0), ..Default::default() },
            shadow: Shadow { color: theme::shadow(0.3), offset: Vector::new(0.0, 4.0), blur_radius: 14.0 },
            ..Default::default()
        })
        .into()
}

/// A label under the control on hover.
fn tooltip<'a>(content: impl Into<Element<'a, Message>>, label: &'a str) -> Element<'a, Message> {
    let tip = container(text(label).size(11).font(theme::font::SANS_MEDIUM).color(theme::c().frame_ink))
        .padding(Padding { top: 3.0, bottom: 3.0, left: 7.0, right: 7.0 })
        .style(|_: &Theme| container::Style {
            background: Some(theme::c().frame_raised.into()),
            border: Border { radius: theme::radius(6.0), width: 1.0, color: theme::c().frame_hairline_strong },
            ..Default::default()
        });
    nice_plug_iced::iced::widget::tooltip(content, tip, nice_plug_iced::iced::widget::tooltip::Position::Bottom).gap(6).into()
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
