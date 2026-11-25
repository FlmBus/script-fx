//! The Matrix (notch: Matrix): every modulation route of the patch as a table, macro slots and
//! modulator assignments alike, each in its source's colour. A row: on/off, source,
//! destination, now (the source's live level as a bar), amount (its sign is the direction),
//! curve (a small graph of the response, as in Serum: drag up/down to bend it, double-click =
//! linear), polarity, reset, remove. Below the rows: add a route (pick a source, then a
//! destination).
//!
//! Edits go through the session like the meta knob's (`Session::set_route`, `add_route`,
//! `remove_route`), so they apply live and are saved with the project.

use std::fmt;
use std::path::PathBuf;

use nice_plug_iced::iced::{
    Background, Border, Center, Color, Element, Fill, Padding, Point, Rectangle, Renderer, Theme,
    mouse,
    widget::{button, canvas, column, container, pick_list, row, scrollable, slider, space, stack, text},
};
use patch::{ModDestination, ModSettings, ModulatorId, Patch, Route, RouteSource, Values};

use super::{routing, session::Session, theme};
use crate::{FxParams, Shared};

/// The add row's state: the source picked, waiting for a destination.
#[derive(Default)]
pub struct Matrix {
    source: Option<RouteSource>,
}

/// One row's route, with what it connects: a message from an older frame (the routes changed
/// since, or the patch was reloaded) can't edit a different route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    dir: PathBuf,
    route: Route,
    source: RouteSource,
    destination: ModDestination,
}

#[derive(Debug, Clone)]
pub enum Edit {
    Amount(f32),
    Curve(f32),
    TogglePolarity,
    ToggleBypass,
    Reset,
    Remove,
}

#[derive(Debug, Clone)]
pub enum Message {
    Edit(Slot, Edit),
    PickSource(Choice<RouteSource>),
    PickDestination(Choice<ModDestination>),
}

/// A pick list entry: what it shows and what it picks.
#[derive(Debug, Clone)]
pub struct Choice<T> {
    label: String,
    value: T,
}

impl<T> fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

impl<T: PartialEq> PartialEq for Choice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

struct SlotRow<'a> {
    slot: Slot,
    source: &'a str,
    /// "M1 · heat", "modulator".
    source_detail: String,
    colour: Color,
    destination: &'a str,
    /// The param or macro id.
    destination_detail: String,
    settings: ModSettings,
    /// The source's level now (0..1).
    level: f32,
}

/// The source's live level: a macro's value, a modulator's output.
fn level(values: &Values, source: RouteSource) -> f32 {
    match source {
        RouteSource::Macro(slot) => values.macros[slot],
        RouteSource::Modulator(id) => values.modulators[id.index()],
    }
}

fn rows<'a>(patch: &'a Patch, values: &Values) -> Vec<SlotRow<'a>> {
    patch
        .routes()
        .into_iter()
        .filter_map(|route| {
            let source = patch.route_source(route)?;
            let destination = patch.route_destination(route)?;
            let (destination_detail, source_detail) = (
                match &destination {
                    ModDestination::Param(id) => id.clone(),
                    ModDestination::Macro(id) => format!("macro · {id}"),
                },
                match source {
                    RouteSource::Macro(slot) => format!("M{} · {}", slot + 1, patch.macros[slot].id),
                    RouteSource::Modulator(_) => "modulator".to_string(),
                },
            );
            Some(SlotRow {
                source: routing::name(patch, source),
                source_detail,
                colour: routing::colour(source),
                destination: match &destination {
                    ModDestination::Param(id) => &patch.params[patch.param_index(id)?].name,
                    ModDestination::Macro(id) => &patch.macros[patch.macro_index(id)?].name,
                },
                destination_detail,
                settings: patch.route_settings(route)?,
                level: level(values, source),
                slot: Slot { dir: patch.dir.clone(), route, source, destination },
            })
        })
        .collect()
}

/// The slot still is what the row showed.
fn valid(patch: &Patch, slot: &Slot) -> bool {
    patch.dir == slot.dir
        && patch.route_source(slot.route) == Some(slot.source)
        && patch.route_destination(slot.route).as_ref() == Some(&slot.destination)
}

/// New settings, computed from the *current* ones, so a message for one field cannot undo a
/// newer edit to another field. `None`: stale slot, non-finite slider value, or Remove.
fn edited(patch: &Patch, slot: &Slot, edit: Edit) -> Option<ModSettings> {
    if !valid(patch, slot) {
        return None;
    }
    let mut s = patch.route_settings(slot.route)?;
    match edit {
        Edit::Amount(v) if v.is_finite() => s.amount = v.clamp(-1.0, 1.0),
        Edit::Curve(v) if v.is_finite() => s.curve = if v.abs() < 0.03 { 0.0 } else { v.clamp(-1.0, 1.0) },
        Edit::Amount(_) | Edit::Curve(_) | Edit::Remove => return None,
        Edit::TogglePolarity => s.bipolar = !s.bipolar,
        Edit::ToggleBypass => s.bypass = !s.bypass,
        Edit::Reset => s = patch.route_reset(slot.route)?,
    }
    Some(s)
}

impl Matrix {
    pub fn update(&mut self, session: &mut Session, params: &FxParams, shared: &Shared, message: Message) {
        let Some(patch) = session.current.as_ref().map(|c| &c.patch) else { return };
        match message {
            Message::Edit(slot, Edit::Remove) => {
                if valid(patch, &slot) {
                    session.remove_route(params, slot.route);
                }
            }
            Message::Edit(slot, edit) => {
                if let Some(settings) = edited(patch, &slot, edit) {
                    session.set_route(params, shared, slot.route, settings);
                }
            }
            Message::PickSource(c) => self.source = Some(c.value),
            Message::PickDestination(c) => {
                if let Some(source) = self.source.take() {
                    session.add_route(params, source, c.value);
                }
            }
        }
    }

    pub fn view<'a>(&'a self, patch: &'a Patch, values: &Values) -> Element<'a, Message> {
        let rows = rows(patch, values);
        let count = rows.len();
        let header = column![
            text("Matrix").size(16).font(theme::font::SANS_SEMIBOLD).color(theme::c().frame_ink),
            text(format!("{count} {}", if count == 1 { "route" } else { "routes" })).size(11).color(theme::c().frame_muted),
        ]
        .spacing(3);

        let columns = row![
            heading("On", ON),
            heading("Source", SOURCE),
            heading("Destination", TARGET),
            heading("Now", NOW),
            heading("Amount", AMOUNT),
            heading("Curve", CURVE),
            heading("Mode", POLARITY),
            heading("", RESET + GAP + REMOVE),
        ]
        .spacing(GAP)
        .align_y(Center);

        let mut list = column![].spacing(3);
        if rows.is_empty() {
            list = list.push(container(text("No modulation yet.").size(12).color(theme::c().frame_muted)).padding([16, 0]));
        }
        for item in rows {
            list = list.push(slot_row(item));
        }
        list = list.push(space::vertical().height(10)).push(self.add_row(patch)).push(space::vertical().height(6)).push(
            text("Or drag a macro's or modulator's grip onto a knob. Reset restores the patch's settings.")
                .size(11)
                .color(theme::c().frame_muted),
        );

        let body = column![
            container(header).height(NOTCH_CLEARANCE),
            columns,
            space::vertical().height(5),
            container(space::horizontal().width(Fill).height(1)).style(theme::rule(theme::c().frame_hairline_strong)),
            space::vertical().height(5),
            scrollable(list).height(Fill).direction(theme::thin_scrollbar()).style(theme::paper_scrollbar),
        ]
        .padding(Padding { top: PAD, right: PAD, bottom: BOTTOM_CLEARANCE, left: PAD });

        container(body)
            .width(Fill)
            .height(Fill)
            .style(|_: &Theme| container::Style {
                background: Some(theme::c().frame_raised.into()),
                border: Border { radius: theme::radius(theme::RADIUS_WINDOW), ..Default::default() },
                ..Default::default()
            })
            .into()
    }

    /// "Add route: [source ▾] → [destination ▾]". Picking the destination adds the route.
    fn add_row<'a>(&'a self, patch: &'a Patch) -> Element<'a, Message> {
        let sources: Vec<Choice<RouteSource>> = (0..patch.macros.len())
            .map(RouteSource::Macro)
            .chain(ModulatorId::ALL.into_iter().map(RouteSource::Modulator))
            .map(|s| Choice { label: routing::name(patch, s).to_string(), value: s })
            .collect();
        let picked = self.source.map(|s| Choice { label: routing::name(patch, s).to_string(), value: s });
        // Params for every source; macros only for modulators (macros don't drive macros).
        let destinations: Vec<Choice<ModDestination>> = match self.source {
            None => Vec::new(),
            Some(source) => {
                // "tape · Amount": param names repeat across blocks.
                let params = patch.params.iter().map(|p| {
                    let block = p.id.split_once('.').map_or(p.id.as_str(), |(b, _)| b);
                    Choice { label: format!("{block} · {}", p.name), value: ModDestination::Param(p.id.clone()) }
                });
                let macros = patch.macros.iter().map(|m| Choice { label: format!("Macro · {}", m.name), value: ModDestination::Macro(m.id.clone()) });
                let all: Vec<_> = if matches!(source, RouteSource::Modulator(_)) { params.chain(macros).collect() } else { params.collect() };
                all.into_iter().filter(|c| patch.find_route(source, &c.value).is_none()).collect()
            }
        };
        row![
            text("Add route").size(11.5).font(theme::font::SANS_MEDIUM).color(theme::c().frame_ink_2),
            pick_list(sources, picked, Message::PickSource)
                .placeholder("Source")
                .width(150)
                .text_size(11.5)
                .padding([4, 8])
                .style(theme::frame_pick_list)
                .menu_style(theme::frame_menu),
            text("→").size(12).color(theme::c().frame_muted),
            pick_list(destinations, None::<Choice<ModDestination>>, Message::PickDestination)
                .placeholder("Destination")
                .width(220)
                .text_size(11.5)
                .padding([4, 8])
                .style(theme::frame_pick_list)
                .menu_style(theme::frame_menu),
        ]
        .spacing(8)
        .align_y(Center)
        .into()
    }
}

// Leave room for the notch and the collapsed drawer, which hang over the view.
const PAD: f32 = 18.0;
const NOTCH_CLEARANCE: f32 = 56.0;
/// A margin below the list (the dock is outside the stage).
const BOTTOM_CLEARANCE: f32 = PAD;
const ON: f32 = 36.0;
const SOURCE: f32 = 104.0;
const TARGET: f32 = 132.0;
const NOW: f32 = 64.0;
const AMOUNT: f32 = 120.0;
const CURVE: f32 = 106.0;
const POLARITY: f32 = 38.0;
const RESET: f32 = 44.0;
const REMOVE: f32 = 24.0;
const GAP: f32 = 6.0;

fn heading(label: &'static str, width: f32) -> Element<'static, Message> {
    text(label).size(10.5).font(theme::font::SANS_MEDIUM).color(theme::c().frame_muted).width(width).into()
}

fn slot_row(item: SlotRow<'_>) -> Element<'_, Message> {
    let s = item.settings;
    let colour = item.colour;
    let message = |edit| Message::Edit(item.slot.clone(), edit);
    let on = control(if s.bypass { "Off" } else { "On" }, !s.bypass, ON, message(Edit::ToggleBypass));
    let dot = container(space::horizontal().width(7).height(7)).style(move |_: &Theme| container::Style {
        background: Some(theme::alpha(colour, if s.bypass { 0.35 } else { 1.0 }).into()),
        border: Border { radius: theme::radius(4.0), ..Default::default() },
        ..Default::default()
    });
    let source = container(row![
        dot,
        column![
            text(item.source).size(12).font(theme::font::SANS_MEDIUM).color(theme::c().frame_ink),
            text(item.source_detail).size(10).color(theme::c().frame_muted),
        ]
        .spacing(2),
    ]
    .spacing(6)
    .align_y(Center))
    .width(SOURCE)
    .clip(true);
    let target = container(column![
        text(item.destination).size(12).font(theme::font::SANS_MEDIUM).color(theme::c().frame_ink),
        text(item.destination_detail).size(10).color(theme::c().frame_muted),
    ]
    .spacing(2))
    .width(TARGET)
    .clip(true);
    let now = container(canvas(Level { value: item.level, colour }).width(Fill).height(4)).width(NOW).center_y(25);
    let amount_slot = item.slot.clone();
    let amount = signed_slider(s.amount, format!("{:+.0}%", s.amount * 100.0).replace('-', "−"), AMOUNT, colour, move |v| {
        Message::Edit(amount_slot.clone(), Edit::Amount(v))
    });
    let curve_slot = item.slot.clone();
    let curve_text = if s.curve == 0.0 { "lin".into() } else { format!("{:+.2}", s.curve).replace('-', "−") };
    let curve = row![
        canvas(CurveGraph { curve: s.curve, colour, on_change: move |v| Message::Edit(curve_slot.clone(), Edit::Curve(v)) })
            .width(Fill)
            .height(25),
        text(curve_text).size(10.5).font(theme::font::MONO).color(theme::c().frame_ink_2).width(40),
    ]
    .spacing(6)
    .align_y(Center)
    .width(CURVE);
    let mode = control(if s.bipolar { "Bi" } else { "Uni" }, s.bipolar, POLARITY, message(Edit::TogglePolarity));
    let reset = control("Reset", false, RESET, message(Edit::Reset));
    let remove = control("×", false, REMOVE, message(Edit::Remove));
    container(row![on, source, target, now, amount, curve, mode, reset, remove].spacing(GAP).align_y(Center))
        .padding(Padding { top: 5.0, bottom: 5.0, ..Padding::ZERO })
        .width(Fill)
        .style(|_: &Theme| container::Style {
            border: Border { width: 1.0, color: theme::c().frame_hairline, ..Default::default() },
            ..Default::default()
        })
        .into()
}

fn control<'a>(label: &'static str, active: bool, width: f32, msg: Message) -> Element<'a, Message> {
    button(container(text(label).size(10.5).font(theme::font::SANS_MEDIUM)).center(Fill))
        .on_press(msg)
        .width(width)
        .height(25)
        .padding(0)
        .style(theme::frame_tab(active))
        .into()
}

/// A signed slider whose bar (in the source's colour) starts at zero, not the left end.
fn signed_slider<'a>(value: f32, value_text: String, width: f32, colour: Color, on_change: impl Fn(f32) -> Message + 'a) -> Element<'a, Message> {
    let track = canvas(SignedTrack { value, colour }).width(Fill).height(20);
    let slider = slider(-1.0..=1.0, value, on_change).step(0.005).height(20).style(slider_style);
    row![stack![track, slider].width(Fill).height(20), text(value_text).size(10.5).font(theme::font::MONO).color(theme::c().frame_ink_2).width(44)]
        .spacing(4)
        .align_y(Center)
        .width(width)
        .into()
}

fn slider_style(_: &Theme, status: slider::Status) -> slider::Style {
    let transparent = Background::Color(Color::TRANSPARENT);
    slider::Style {
        rail: slider::Rail { backgrounds: (transparent, transparent), width: 0.0, border: Border::default() },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 4.0 },
            background: Background::Color(if matches!(status, slider::Status::Hovered | slider::Status::Dragged) {
                theme::c().frame_ink
            } else {
                theme::c().frame_ink_2
            }),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        },
    }
}

/// The curve column: what the route does to the source (x = the source, y = what reaches the
/// knob, `patch::tension`), drawn in the source's colour over a faint linear diagonal, as in
/// Serum's matrix. Drag up/down to bend it the way the pointer goes (up = the curve bows up, a
/// fast start), double-click = linear, the wheel steps the same way.
struct CurveGraph<F> {
    curve: f32,
    colour: Color,
    on_change: F,
}

#[derive(Default)]
struct CurveState {
    /// A drag: where it started (y) and the curve then.
    drag: Option<(f32, f32)>,
    last_press: Option<std::time::Instant>,
    hover: bool,
}

/// Pixels of vertical drag for the whole −1…+1 range.
const CURVE_DRAG: f32 = 120.0;

impl<F: Fn(f32) -> Message> canvas::Program<Message> for CurveGraph<F> {
    type State = CurveState;

    fn update(&self, state: &mut CurveState, event: &canvas::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        use canvas::Event::Mouse;
        match event {
            Mouse(mouse::Event::CursorMoved { position }) => {
                state.hover = cursor.is_over(bounds);
                let (y0, c0) = state.drag?;
                let c = (c0 + (position.y - y0) * 2.0 / CURVE_DRAG).clamp(-1.0, 1.0);
                Some(canvas::Action::publish((self.on_change)(c)).and_capture())
            }
            Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_over(bounds)?;
                let double = state.last_press.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(350));
                state.last_press = Some(std::time::Instant::now());
                if double {
                    state.drag = None;
                    return Some(canvas::Action::publish((self.on_change)(0.0)).and_capture());
                }
                state.drag = Some((p.y, self.curve));
                Some(canvas::Action::capture())
            }
            Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => state.drag.take().map(|_| canvas::Action::capture()),
            Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_over(bounds)?;
                let notches = match *delta {
                    mouse::ScrollDelta::Lines { y, .. } => y,
                    mouse::ScrollDelta::Pixels { y, .. } => y / 40.0,
                };
                Some(canvas::Action::publish((self.on_change)((self.curve - notches * 0.05).clamp(-1.0, 1.0))).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &CurveState, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let c = theme::c();
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let live = state.hover || state.drag.is_some();
        f.fill(&canvas::Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 4.0.into()), theme::alpha(c.frame_ink, if live { 0.08 } else { 0.05 }));
        let pad = 3.0;
        let at = |x: f32, y: f32| Point::new(pad + x * (w - 2.0 * pad), h - pad - y * (h - 2.0 * pad));
        // As the audio thread applies it (`ModSettings::delta`): the whole 0..1 range of the source
        // is bent, bipolar routes then centre it.
        let shape = |x: f32| patch::tension(x, self.curve);
        f.stroke(&canvas::Path::line(at(0.0, 0.0), at(1.0, 1.0)), canvas::Stroke::default().with_color(theme::alpha(c.frame_ink, 0.12)).with_width(1.0));
        const N: usize = 40;
        let line = canvas::Path::new(|b| {
            b.move_to(at(0.0, shape(0.0)));
            for k in 1..=N {
                let x = k as f32 / N as f32;
                b.line_to(at(x, shape(x)));
            }
        });
        f.stroke(&line, canvas::Stroke::default().with_color(self.colour).with_width(1.6));
        vec![f.into_geometry()]
    }

    fn mouse_interaction(&self, state: &CurveState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.drag.is_some() || cursor.is_over(bounds) { mouse::Interaction::ResizingVertically } else { mouse::Interaction::None }
    }
}

/// A source's live level: a thin bar in its colour.
struct Level {
    value: f32,
    colour: Color,
}

impl canvas::Program<Message> for Level {
    type State = ();
    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let size = nice_plug_iced::iced::Size::new;
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), theme::c().frame_control);
        frame.fill_rectangle(Point::ORIGIN, size(bounds.width * self.value.clamp(0.0, 1.0), bounds.height), self.colour);
        vec![frame.into_geometry()]
    }
}

struct SignedTrack {
    value: f32,
    colour: Color,
}

impl canvas::Program<Message> for SignedTrack {
    type State = ();
    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, y) = (bounds.width, bounds.height / 2.0);
        // The iced slider's 8 px handle travels between x=4 and x=w-4.
        let center = w / 2.0;
        let end = center + self.value.clamp(-1.0, 1.0) * (w - 8.0).max(0.0) / 2.0;
        let size = nice_plug_iced::iced::Size::new;
        frame.fill_rectangle(Point::new(0.0, y - 2.0), size(w, 4.0), theme::c().frame_control);
        frame.fill_rectangle(Point::new(center.min(end), y - 2.0), size((center - end).abs(), 4.0), self.colour);
        frame.fill_rectangle(Point::new(center - 0.5, y - 4.0), size(1.0, 8.0), theme::c().frame_muted);
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nice_plug::{
        context::{
            PluginApi,
            gui::{GuiContext, GuiContextInner},
        },
        params::internals::ParamPtr,
        plugin::PluginState,
    };
    use std::sync::{Arc, atomic::Ordering};

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-patches/drum-bus").canonicalize().unwrap()
    }

    fn fixture() -> Patch {
        Patch::load(fixture_dir()).unwrap()
    }

    #[test]
    fn rows_list_both_kinds_and_guard_stale_messages() {
        let mut patch = fixture();
        assert!(rows(&Patch::empty(), &Values::default()).is_empty());
        let macro_routes: usize = patch.modulations.iter().map(Vec::len).sum();
        assert!(macro_routes > 1);
        let first = rows(&patch, &Values::default())[0].slot.clone();
        let lfo = patch.add_route(RouteSource::Modulator(ModulatorId::Lfo1), first.destination.clone()).unwrap();
        let displayed = rows(&patch, &Values::default());
        assert_eq!(displayed.len(), macro_routes + patch.assignments.len());
        let lfo_row = displayed.iter().find(|r| r.slot.route == lfo).unwrap();
        assert_eq!(lfo_row.source, "LFO 1");
        assert_eq!(lfo_row.colour, routing::colour(RouteSource::Modulator(ModulatorId::Lfo1)));

        let settings = edited(&patch, &lfo_row.slot, Edit::Amount(-0.25)).unwrap();
        assert_eq!(settings.amount, -0.25);
        assert!(settings.bipolar, "kept the LFO's other settings");

        let mut stale = first.clone();
        stale.dir.push("other-patch");
        assert!(edited(&patch, &stale, Edit::Reset).is_none());
        stale = first.clone();
        stale.source = RouteSource::Modulator(ModulatorId::Step);
        assert!(edited(&patch, &stale, Edit::Reset).is_none());
        stale = first.clone();
        stale.destination = ModDestination::Param("gone".into());
        assert!(edited(&patch, &stale, Edit::Reset).is_none());
        assert!(edited(&patch, &first, Edit::Amount(f32::NAN)).is_none());
        assert!(edited(&patch, &first, Edit::Curve(f32::INFINITY)).is_none());
        // Removing a route makes the rows after it stale.
        let lfo_slot = lfo_row.slot.clone();
        patch.remove_route(first.route);
        assert!(!valid(&patch, &first) || first.route != lfo_slot.route);
        // Construct both the empty and populated views without a renderer.
        let m = Matrix::default();
        let empty = Patch::empty();
        let _ = m.view(&empty, &Values::default());
        let _ = m.view(&patch, &Values::default());
    }

    #[test]
    fn edits_preserve_other_fields_and_reset_to_patch_defaults() {
        let mut patch = fixture();
        let slot = rows(&patch, &Values::default())[0].slot.clone();
        let Route::Macro { param, k } = slot.route else { panic!("macro routes come first") };
        let initial = patch.modulations[param][k].patch;
        for edit in [Edit::Amount(-2.0), Edit::Curve(0.55), Edit::TogglePolarity, Edit::ToggleBypass] {
            let s = edited(&patch, &slot, edit).unwrap();
            patch.modulations[param][k].settings = s;
        }
        let s = patch.modulations[param][k].settings;
        assert_eq!(s.amount, -1.0);
        assert_eq!(s.curve, 0.55);
        assert_ne!(s.bipolar, initial.bipolar);
        assert_ne!(s.bypass, initial.bypass);
        let snapped = edited(&patch, &slot, Edit::Curve(0.02)).unwrap();
        assert_eq!(snapped.curve, 0.0);
        assert_eq!(snapped.amount, -1.0);
        assert_eq!(edited(&patch, &slot, Edit::Reset), Some(initial));
    }

    // Session::new scans the patch library. Run the integration test in its own process with a
    // read-only fixture library, rather than installing factory patches in the user's data dir or
    // changing the environment of other tests.
    #[test]
    fn matrix_session_updates_live_graph_and_survives_reload() {
        const CHILD: &str = "SCRIPTFX_MATRIX_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "shell::matrix::tests::matrix_session_updates_live_graph_and_survives_reload", "--nocapture"])
                .env(CHILD, "1")
                .env("SCRIPTFX_PATCHES", fixture_dir().parent().unwrap())
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(output.status.success() && stdout.contains("1 passed"), "{stdout}\n{stderr}");
            return;
        }

        let mut plugin = crate::ScriptFx::default();
        let mut session = Session::new();
        let mut matrix = Matrix::default();
        let ctx = GuiContext::new(Arc::new(TestHost));
        let dir = fixture_dir();
        fn patch(s: &Session) -> &Patch {
            &s.current.as_ref().unwrap().patch
        }
        session.load(&ctx, &plugin.params, &plugin.shared, dir.to_str().unwrap(), false);
        let slot = rows(patch(&session), &Values::default())[0].slot.clone();
        let Route::Macro { param, k } = slot.route else { panic!() };
        let default = patch(&session).modulations[param][k].patch;
        let flat = patch(&session).mod_index(param, k);
        let key = patch(&session).mod_key(param, k);
        let other = rows(patch(&session), &Values::default())[1].slot.clone();
        let other_settings = patch(&session).route_settings(other.route).unwrap();
        let graph = plugin.graph_rx.pop().unwrap();
        for edit in [Edit::Amount(-0.4), Edit::Curve(0.5), Edit::TogglePolarity, Edit::ToggleBypass] {
            matrix.update(&mut session, &plugin.params, &plugin.shared, Message::Edit(slot.clone(), edit));
        }
        let current = patch(&session).modulations[param][k].settings;
        assert_eq!(current.amount, -0.4);
        assert_eq!(current.curve, 0.5);
        assert!(current.bipolar && current.bypass);
        assert_eq!(patch(&session).route_settings(other.route), Some(other_settings));
        assert_eq!(plugin.params.mod_settings.read().unwrap().get(&key), Some(&current));
        assert_eq!(graph.mod_settings()[flat].load(Ordering::Relaxed), current.pack());

        // Add an LFO route through the add row: the graph is rebuilt once at the next tick.
        let dest = ModDestination::Param(patch(&session).params[param].id.clone());
        matrix.update(&mut session, &plugin.params, &plugin.shared, Message::PickSource(Choice { label: String::new(), value: RouteSource::Modulator(ModulatorId::Lfo2) }));
        matrix.update(&mut session, &plugin.params, &plugin.shared, Message::PickDestination(Choice { label: String::new(), value: dest.clone() }));
        assert!(plugin.graph_rx.pop().is_err(), "not rebuilt before the tick");
        session.tick(&ctx, &plugin.params, &plugin.shared);
        let rebuilt = plugin.graph_rx.pop().unwrap();
        assert_eq!(rebuilt.assignment_settings().len(), patch(&session).assignments.len());
        let lfo = patch(&session).find_route(RouteSource::Modulator(ModulatorId::Lfo2), &dest).unwrap();
        assert!(plugin.params.mod_edits.read().unwrap().assignments.as_ref().is_some_and(|a| a.len() == patch(&session).assignments.len()));

        // Reload (hot reload / reopening the project): edits and settings come back.
        session.load(&ctx, &plugin.params, &plugin.shared, dir.to_str().unwrap(), false);
        assert_eq!(patch(&session).modulations[param][k].settings, current);
        assert_eq!(patch(&session).find_route(RouteSource::Modulator(ModulatorId::Lfo2), &dest), Some(lfo));
        let restored_graph = plugin.graph_rx.pop().unwrap();
        assert_eq!(restored_graph.mod_settings()[flat].load(Ordering::Relaxed), current.pack());
        matrix.update(&mut session, &plugin.params, &plugin.shared, Message::Edit(slot.clone(), Edit::Reset));
        assert_eq!(patch(&session).modulations[param][k].settings, default);
        assert_eq!(plugin.params.mod_settings.read().unwrap().get(&key), Some(&default));
        assert_eq!(restored_graph.mod_settings()[flat].load(Ordering::Relaxed), default.pack());

        // Remove the patch's own route: gone after a reload too.
        matrix.update(&mut session, &plugin.params, &plugin.shared, Message::Edit(slot.clone(), Edit::Remove));
        assert!(patch(&session).route_source(slot.route) != Some(slot.source) || patch(&session).route_destination(slot.route) != Some(slot.destination.clone()));
        session.load(&ctx, &plugin.params, &plugin.shared, dir.to_str().unwrap(), false);
        assert!(patch(&session).find_route(slot.source, &slot.destination).is_none());
    }

    struct TestHost;
    impl GuiContextInner for TestHost {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Standalone
        }
        unsafe fn raw_begin_set_parameter(&self, _: ParamPtr) {}
        unsafe fn raw_set_parameter_normalized(&self, _: ParamPtr, _: f32) {}
        unsafe fn raw_end_set_parameter(&self, _: ParamPtr) {}
        fn get_state(&self) -> PluginState {
            panic!("not used")
        }
        fn set_state(&self, _: PluginState) {
            panic!("not used")
        }
        fn request_restart(&self) {}
    }
}
