//! The mod dock: always there below the stage (it never covers it), the patch's sources in one
//! strip. Design: `docs/research/mockups/shell-layout/` (boards 1–3 and 8, "Component · mod
//! dock").
//!
//! - **Macros**, eight at a time (pager 1–8 · 9–16): a cell per macro, its title bar in macro
//!   yellow is the drag handle (drag it onto a knob), the knob below turns the macro.
//! - **Modulators** (Follow, LFO 1–3, Step): a tile each, a title bar in its colour (the drag
//!   handle), a live mini display (one LFO cycle with a dot at the audio thread's phase, Step's
//!   bars with the playing one lit, Follow's recent output) and its rate or level.
//!
//! **Focus:** a click on a source (a tile, or a title bar released without dragging) focuses it:
//! it is outlined, the others fade, the hint says how many routes it has. A click on the focused
//! modulator again opens its **editor**: a panel rising from the dock over the stage's lower part
//! ([`Dock::editor_view`]; the stage isn't scaled or moved), with the modulator's display and
//! knobs. It stays open while the stage is used (turn a knob, watch it move), steps aside while a
//! source is dragged (so anything under it takes the drop), and closes with Done, Escape or a
//! click on its tile. Escape then clears the focus.
//!
//! LFO and Step rates have two modes (`Hz | sync`, the pills on the Rate knob): free in Hz, or a
//! note length at the host tempo; in sync the knob steps through [`NOTES`]. Changes apply live and
//! are saved with the project (`Session::set_modulator`).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use patch::{ModulatorId, ModulatorLfoShape, ModulatorSettings, ModulatorSpec, Patch, RouteSource, Values};

use super::{knob, routing, theme};
use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, Padding, Pixels, Point, Rectangle, Renderer, Shadow, Size, Theme, Vector, alignment,
    animation::{Animation, Easing},
    color, mouse,
    widget::{Row, button, canvas, column, container, mouse_area, row, space, text, text::Alignment},
};

/// The fixed modulator set in tile order (= `ModulatorId::ALL`), each with its colour (fixed,
/// never a patch colour).
pub const MODULATORS: [(&str, Color); 5] = [
    ("Follow", color!(0x60a5fa)),
    ("LFO 1", color!(0x2dd4bf)),
    ("LFO 2", color!(0xf472b6)),
    ("LFO 3", color!(0xa78bfa)),
    ("Step", color!(0xa3e635)),
];

/// Frames of output history Follow's displays show (≈ 2 s at the shell's frame rate).
const HISTORY: usize = 120;
/// Macros per page.
const PAGE: usize = 8;
/// A modulator tile (the macro cell is `knob::MACRO_SIZE`).
const TILE: Size = Size::new(63.0, 74.0);
const GAP: f32 = 4.0;
/// Padding inside the dock, around the strip and the editor.
const PAD: Padding = Padding { top: 6.0, right: 2.0, bottom: 6.0, left: 2.0 };
/// The editor's display height.
const DISPLAY_H: f32 = 132.0;
/// The editor panel's height (title, display, padding).
pub const EDITOR_H: f32 = 196.0;

pub struct Dock {
    /// The macro page (0 = 1–8, 1 = 9–16).
    page: usize,
    /// The focused source.
    focus: Option<RouteSource>,
    /// The modulator whose editor is open (index into [`MODULATORS`]); `shown` keeps the last one
    /// drawn while the dock shrinks back.
    editing: Option<usize>,
    shown: usize,
    /// The editor's open/close animation (the dock's height between the strip and the editor).
    grow: Animation<bool>,
    /// Follow's recent output (0..1, one value per frame), newest last.
    history: VecDeque<f32>,
    /// Each modulator's output and cycle position this frame (`Values::modulators`, `modulator_phases`).
    live: [f32; 5],
    phases: [f32; 5],
}

#[derive(Debug, Clone)]
pub enum Message {
    Page(usize),
    /// Input on a macro cell (slot).
    Macro(usize, knob::KnobEvent),
    /// Input on a modulator tile.
    Tile(usize, TileEvent),
    /// Input on one of the open editor's knobs (index into its [`fields`]).
    Knob(usize, knob::KnobEvent),
    Shape(ModulatorLfoShape),
    /// Step `i` set to a level (0..1).
    StepValue(usize, f32),
    /// The editor's Done.
    Done,
}

#[derive(Debug, Clone, Copy)]
pub enum TileEvent {
    /// The title bar was pressed (at this window point): drag the modulator.
    Grab(Point),
    /// A click on the tile: focus it (again: its editor).
    Select,
}

impl From<(usize, knob::KnobEvent)> for Message {
    fn from((i, e): (usize, knob::KnobEvent)) -> Self {
        Message::Knob(i, e)
    }
}

/// Macro cells report as [`Message::Macro`].
struct MacroMsg(Message);

impl From<(usize, knob::KnobEvent)> for MacroMsg {
    fn from((slot, e): (usize, knob::KnobEvent)) -> Self {
        MacroMsg(Message::Macro(slot, e))
    }
}

/// What the shell does for the dock.
pub enum Event {
    /// Input on a macro's knob or title bar.
    Macro(usize, knob::KnobEvent),
    /// Start dragging this source from this window point (drag & drop onto a knob).
    Grab(RouteSource, Point),
    /// New settings for a modulator.
    Set(ModulatorSpec),
    /// A knob drag starts / ends (the shell hides the cursor meanwhile).
    Dragging(bool),
}

impl Dock {
    pub fn new() -> Self {
        Self {
            page: 0,
            focus: None,
            editing: None,
            shown: 1,
            grow: Animation::new(false).duration(Duration::from_millis(260)).easing(Easing::EaseInOut),
            history: VecDeque::with_capacity(HISTORY),
            live: [0.0; 5],
            phases: [0.0; 5],
        }
    }

    /// `mods`: the patch's modulators (`ModulatorId::ALL` order), what knob input changes.
    pub fn update(&mut self, message: Message, now: Instant, mods: &[ModulatorSpec]) -> Option<Event> {
        match message {
            Message::Page(p) => self.page = p,
            Message::Macro(slot, knob::KnobEvent::Grab(at)) => return Some(Event::Grab(RouteSource::Macro(slot), at)),
            Message::Macro(slot, e) => return Some(Event::Macro(slot, e)),
            Message::Tile(i, TileEvent::Grab(at)) => return Some(Event::Grab(RouteSource::Modulator(ModulatorId::ALL[i]), at)),
            Message::Tile(i, TileEvent::Select) => self.select(RouteSource::Modulator(ModulatorId::ALL[i]), now),
            Message::Done => self.edit(None, now),
            Message::Knob(f, e) => {
                let mut spec = mods.get(self.shown)?.clone();
                let field = fields(&spec.settings).get(f)?;
                let synced = field.syncable && sync(&spec.settings).is_some();
                match e {
                    knob::KnobEvent::Begin => return Some(Event::Dragging(true)),
                    knob::KnobEvent::End => return Some(Event::Dragging(false)),
                    // Switching to sync starts at a quarter note; the Hz value is kept for later.
                    knob::KnobEvent::Mode(on) if on != synced => set_sync(&mut spec.settings, on.then_some(1.0)),
                    knob::KnobEvent::Set(v) | knob::KnobEvent::Step(v) if synced => set_sync(&mut spec.settings, Some(NOTES[note_at(v)].1)),
                    knob::KnobEvent::Set(v) | knob::KnobEvent::Step(v) => (field.set)(&mut spec.settings, field.plain(v)),
                    _ => return None,
                }
                return Some(Event::Set(spec));
            }
            Message::Shape(shape) => {
                let mut spec = mods.get(self.shown)?.clone();
                if let ModulatorSettings::Lfo { shape: s, .. } = &mut spec.settings {
                    *s = shape;
                }
                return Some(Event::Set(spec));
            }
            Message::StepValue(i, v) => {
                let mut spec = mods.get(self.shown)?.clone();
                if let ModulatorSettings::Step { values, .. } = &mut spec.settings {
                    *values.get_mut(i)? = v.clamp(0.0, 1.0);
                }
                return Some(Event::Set(spec));
            }
        }
        None
    }

    /// A click on a source: focus it; on the focused modulator, open (or close) its editor. With
    /// an editor open, a click on another modulator switches the editor to it.
    pub fn select(&mut self, source: RouteSource, now: Instant) {
        let modulator = match source {
            RouteSource::Modulator(id) => Some(id.index()),
            RouteSource::Macro(_) => None,
        };
        if self.focus == Some(source) {
            if let Some(i) = modulator {
                self.edit(if self.editing == Some(i) { None } else { Some(i) }, now);
            }
            return;
        }
        self.focus = Some(source);
        if self.editing.is_some() {
            self.edit(modulator, now);
        }
    }

    /// Escape: closes the editor, then clears the focus. `false` = there was nothing to close.
    pub fn escape(&mut self, now: Instant) -> bool {
        if self.editing.is_some() {
            self.edit(None, now);
        } else if self.focus.take().is_none() {
            return false;
        }
        true
    }

    fn edit(&mut self, modulator: Option<usize>, now: Instant) {
        if let Some(i) = modulator {
            self.shown = i;
            self.focus = Some(RouteSource::Modulator(ModulatorId::ALL[i]));
        }
        if self.editing.is_some() != modulator.is_some() {
            self.grow.go_mut(modulator.is_some(), now);
        }
        self.editing = modulator;
    }

    /// The focused source (the knobs show its routes).
    #[allow(dead_code)]
    pub fn focus(&self) -> Option<RouteSource> {
        self.focus
    }

    /// Once per frame: the modulators' outputs and cycle positions.
    pub fn tick(&mut self, live: &[f32; 5], phases: &[f32; 5]) {
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(live[ModulatorId::Follow.index()]);
        self.live = *live;
        self.phases = *phases;
    }

    /// 0 = the strip, 1 = the editor open (eased).
    pub fn grown(&self, now: Instant) -> f32 {
        self.grow.interpolate(0.0, 1.0, now)
    }

    /// The editor panel's height over the stage now (it rises and sinks).
    pub fn editor_height(&self, now: Instant) -> f32 {
        self.grown(now) * EDITOR_H
    }

    /// The strip: macros and modulators.
    pub fn view<'a>(&'a self, patch: &'a Patch, values: &'a Values, drag: Option<&routing::SourceDrag>) -> Element<'a, Message> {
        // `Idle` marks the dock as occupied: clicks on it don't fall through.
        let dock = container(self.strip(patch, values, drag)).width(Fill).height(theme::SIZE_DOCK).padding(PAD).clip(true);
        mouse_area(dock).interaction(mouse::Interaction::Idle).into()
    }

    /// The open modulator's editor: a frame-coloured panel along the stage's bottom edge, its
    /// height animating as it rises; `None` while closed.
    pub fn editor_view<'a>(&'a self, now: Instant, mods: &'a [ModulatorSpec]) -> Option<Element<'a, Message>> {
        let h = self.editor_height(now);
        if h <= 0.5 {
            return None;
        }
        let colour = MODULATORS[self.shown].1;
        let panel = container(self.editor(mods))
            .width(Fill)
            .height(EDITOR_H)
            .padding(Padding { top: 10.0, right: 12.0, bottom: 12.0, left: 12.0 });
        let panel = container(panel).width(Fill).height(h).clip(true).style(move |_: &Theme| container::Style {
            background: Some(theme::c().frame.into()),
            border: Border { radius: theme::radius(0.0).top(theme::RADIUS_WINDOW), width: 1.0, color: theme::alpha(colour, 0.35) },
            shadow: Shadow { color: theme::shadow(0.45), offset: Vector::new(0.0, -10.0), blur_radius: 24.0 },
            ..Default::default()
        });
        // `Idle`: clicks on the panel don't fall through to the stage.
        Some(mouse_area(panel).interaction(mouse::Interaction::Idle).into())
    }

    // ---- the strip -----------------------------------------------------------------------------

    fn strip<'a>(&'a self, patch: &'a Patch, values: &'a Values, drag: Option<&routing::SourceDrag>) -> Element<'a, Message> {
        let focused = self.focus;
        let pages = patch.macros.len().div_ceil(PAGE).max(1);
        let page = self.page.min(pages - 1);
        let macro_drop = drag.and_then(routing::SourceDrag::macro_drop);
        let cells = (page * PAGE..(page + 1) * PAGE).map(|slot| -> Element<'a, Message> {
            let Some(m) = patch.macros.get(slot) else { return empty_cell(slot) };
            let v = values.macros[slot];
            let source = RouteSource::Macro(slot);
            let cell = knob::macro_cell::<MacroMsg>(knob::Macro {
                slot,
                label: &m.name,
                value_text: patch.macro_text(slot, v),
                value: v,
                effective: routing::macro_effective(patch, values, slot),
                default: patch.macro_default(slot),
                drop: macro_drop,
                picked: focused == Some(source),
                dim: focused.is_some_and(|f| f != source),
                mods: routing::macro_metas(patch, slot).1,
            });
            cell.map(|MacroMsg(m)| m)
        });
        let macros = column![
            header(
                "MACROS",
                row![pager_label("1–8", page == 0, Some(Message::Page(0))), text("·").size(9.5).color(theme::c().frame_muted), pager_label("9–16", page == 1, (pages > 1).then_some(Message::Page(1)))]
                    .spacing(4)
                    .align_y(Center)
                    .into(),
            ),
            Row::with_children(cells).spacing(GAP),
        ]
        .spacing(4)
        .width(PAGE as f32 * knob::MACRO_SIZE.width + (PAGE - 1) as f32 * GAP);

        let tiles = MODULATORS.iter().enumerate().map(|(i, &(name, colour))| -> Element<'a, Message> {
            let id = ModulatorId::ALL[i];
            let source = RouteSource::Modulator(id);
            let routes = route_count(patch, source);
            let Some(spec) = patch.modulators.get(i) else { return space::horizontal().width(TILE.width).into() };
            let mini = match &spec.settings {
                ModulatorSettings::Follow { .. } => Mini::Follow(&self.history),
                ModulatorSettings::Lfo { shape, .. } => Mini::Lfo { shape: dsp::lfo_shape(*shape), phase: self.phases[i] },
                ModulatorSettings::Step { values, .. } => Mini::Step { values, current: self.phases[i] as usize },
            };
            let value = if routes == 0 { "no routes".to_string() } else { tile_value(&spec.settings, self.live[i]) };
            canvas(Tile {
                index: i,
                name,
                colour,
                mini,
                value,
                picked: focused == Some(source),
                dim: focused.is_some_and(|f| f != source),
                unused: routes == 0,
            })
            .width(TILE.width)
            .height(TILE.height)
            .into()
        });
        let (hint, hint_ink) = self.hint(patch);
        let modulators = column![
            header("MODULATORS", text(hint).size(9.5).font(theme::font::SANS_MEDIUM).color(hint_ink).into()),
            Row::with_children(tiles).spacing(GAP),
        ]
        .spacing(4)
        .width(Fill);

        let rule = container(space::vertical().width(1).height(Fill)).style(|_: &Theme| container::Style {
            background: Some(theme::alpha(theme::c().frame_ink, 0.07).into()),
            ..Default::default()
        });
        row![macros, rule, modulators].spacing(10).height(12.0 + 4.0 + knob::MACRO_SIZE.height).into()
    }

    /// What the dock says about the focus, and in which colour.
    fn hint(&self, patch: &Patch) -> (String, Color) {
        let muted = theme::c().frame_muted;
        let Some(source) = self.focus else { return ("click to focus · drag a title bar onto a knob".into(), muted) };
        let name = routing::name(patch, source);
        let colour = knob::source_ink(routing::colour(source));
        if let Some(i) = self.editing {
            return (format!("editing {} · Done or Esc closes", MODULATORS[i].0), colour);
        }
        let n = route_count(patch, source);
        let routes = match n {
            0 => "no routes yet".to_string(),
            1 => "1 route".to_string(),
            n => format!("{n} routes"),
        };
        let again = if matches!(source, RouteSource::Modulator(_)) { " · click again to edit" } else { "" };
        (format!("{name} · {routes}{again} · Esc clears"), colour)
    }

    // ---- the editor ----------------------------------------------------------------------------

    /// The open modulator: title (and LFO shapes) with Done, then its display left and its knobs right.
    fn editor<'a>(&'a self, mods: &'a [ModulatorSpec]) -> Element<'a, Message> {
        let i = self.shown;
        let (name, colour) = MODULATORS[i];
        let Some(spec) = mods.get(i) else { return column![].into() };
        let defaults = ModulatorSpec::default_for(spec.id);
        let kind = match spec.settings {
            ModulatorSettings::Follow { .. } => "Envelope follower",
            ModulatorSettings::Lfo { .. } => "",
            ModulatorSettings::Step { .. } => "Step sequencer · drag across the steps to draw",
        };

        let knobs = fields(&spec.settings).iter().enumerate().map(|(f, field)| {
            let synced = sync(&spec.settings).filter(|_| field.syncable);
            let (value, value_text, default) = match synced {
                Some(beats) => {
                    let k = note_index(beats);
                    (note_norm(k), NOTES[k].0.to_string(), note_norm(note_index(1.0)))
                }
                None => {
                    let plain = (field.get)(&spec.settings);
                    (field.norm(plain), field.text(plain), field.norm((field.get)(&defaults.settings)))
                }
            };
            knob::param_cell(knob::Param {
                index: f,
                label: field.label,
                value_text,
                value,
                effective: value,
                default,
                mods: Vec::new(),
                drop: None,
                modes: field.syncable.then_some(knob::Modes { labels: ["Hz", "sync"], second: synced.is_some() }),
                colours: Some(knob::Colours { accent: colour, ink: theme::c().frame_ink }),
            })
        });
        let knobs = Row::with_children(knobs).spacing(6);

        let display: Element<'a, Message> = match &spec.settings {
            ModulatorSettings::Follow { .. } => canvas(Scope { history: &self.history, colour }).width(Fill).height(Fill).into(),
            ModulatorSettings::Lfo { shape, .. } => canvas(Cycle { shape: dsp::lfo_shape(*shape), phase: self.phases[i], colour }).width(Fill).height(Fill).into(),
            ModulatorSettings::Step { values, .. } => {
                let current = self.phases[i] as usize;
                canvas(StepBars { values, colour, current }).width(Fill).height(Fill).into()
            }
        };

        let mut title = row![text(name).size(11.5).font(theme::font::SANS_SEMIBOLD).color(colour)].spacing(8).align_y(Center);
        if let ModulatorSettings::Lfo { shape, .. } = spec.settings {
            title = title.push(shapes(shape));
        }
        title = title.push(text(kind).size(10.5).color(theme::c().frame_muted)).push(space::horizontal()).push(
            button(text("Done").size(10.5).font(theme::font::SANS_MEDIUM).color(theme::c().frame_ink_3))
                .on_press(Message::Done)
                .padding(Padding { top: 3.0, bottom: 3.0, left: 9.0, right: 9.0 })
                .style(theme::frame_ghost(false)),
        );
        column![title.height(24), row![container(display).width(Fill).height(DISPLAY_H), container(knobs).padding(Padding { top: 8.0, ..Padding::ZERO })].spacing(12)]
            .spacing(8)
            .width(Fill)
            .into()
    }
}

/// How many routes come from `source`.
fn route_count(patch: &Patch, source: RouteSource) -> usize {
    patch.routes().into_iter().filter(|r| patch.route_source(*r) == Some(source)).count()
}

/// A tile's value line: the rate (a note length when synced), or Follow's level now.
fn tile_value(s: &ModulatorSettings, live: f32) -> String {
    match s {
        ModulatorSettings::Follow { .. } => format!("{:.0}%", live * 100.0),
        ModulatorSettings::Lfo { rate_hz, sync, .. } | ModulatorSettings::Step { rate_hz, sync, .. } => match sync {
            Some(beats) => NOTES[note_index(*beats)].0.to_string(),
            None => fields(s)[0].text(*rate_hz),
        },
    }
}

/// A section's header line: its name at the left, something at the right.
fn header<'a>(name: &'a str, right: Element<'a, Message>) -> Element<'a, Message> {
    row![text(name).size(9.5).font(theme::font::SANS_SEMIBOLD).color(theme::c().frame_muted), space::horizontal(), right]
        .align_y(Center)
        .height(12)
        .into()
}

/// "1–8" / "9–16": bright when it's the page shown; `None` = no such page.
fn pager_label<'a>(label: &'a str, on: bool, msg: Option<Message>) -> Element<'a, Message> {
    let c = theme::c();
    let ink = if on { c.frame_ink_3 } else if msg.is_some() { c.frame_muted } else { theme::alpha(c.frame_muted, 0.4) };
    button(text(label).size(9.5).font(theme::font::SANS_MEDIUM).color(ink))
        .on_press_maybe(msg.filter(|_| !on))
        .padding(0)
        .style(|_: &Theme, _| button::Style::default())
        .into()
}

/// A macro slot the patch doesn't use: a faint placeholder with its slot name.
fn empty_cell<'a>(slot: usize) -> Element<'a, Message> {
    canvas(EmptyCell { slot }).width(knob::MACRO_SIZE.width).height(knob::MACRO_SIZE.height).into()
}

struct EmptyCell {
    slot: usize,
}

impl canvas::Program<Message> for EmptyCell {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let c = theme::c();
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let fade = 0.4;
        f.fill(&canvas::Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 8.0.into()), theme::alpha(c.frame_ink, 0.02 * fade * 2.0));
        knob::title_bar(&mut f, bounds.width, theme::alpha(c.frame_ink, 0.04), theme::alpha(c.frame_muted, 0.0), "", 5.0);
        let label = |content: String, y: f32, font| canvas::Text {
            content,
            position: Point::new(bounds.width / 2.0, y),
            color: theme::alpha(c.frame_muted, fade),
            size: Pixels(8.5),
            font,
            align_x: Alignment::Center,
            align_y: alignment::Vertical::Center,
            ..canvas::Text::default()
        };
        f.fill_text(label("empty".into(), knob::BAR / 2.0, theme::font::SANS_SEMIBOLD));
        f.fill_text(label(format!("M{}", self.slot + 1), bounds.height - 9.0, theme::font::MONO));
        vec![f.into_geometry()]
    }
}

// ---- modulator tiles ---------------------------------------------------------------------------

/// What a tile's mini display shows.
enum Mini<'a> {
    /// One cycle of the wave (`dsp::lfo_shape` morph position) and the phase.
    Lfo { shape: f32, phase: f32 },
    Step { values: &'a [f32], current: usize },
    Follow(&'a VecDeque<f32>),
}

/// A modulator in the strip: title bar (drag handle), mini display, value.
struct Tile<'a> {
    index: usize,
    name: &'static str,
    colour: Color,
    mini: Mini<'a>,
    value: String,
    picked: bool,
    dim: bool,
    /// No routes yet: drawn faded, a neutral bar.
    unused: bool,
}

#[derive(Default)]
struct TileState {
    hover: bool,
    bar_hover: bool,
}

/// The mini display's rectangle in a tile.
const MINI: Rectangle = Rectangle { x: (TILE.width - 50.0) / 2.0, y: knob::BAR + 5.0, width: 50.0, height: 26.0 };

impl canvas::Program<Message> for Tile<'_> {
    type State = TileState;

    fn update(&self, state: &mut TileState, event: &canvas::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        match event {
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                state.hover = cursor.is_over(bounds);
                state.bar_hover = cursor.position_over(bounds).is_some_and(|p| p.y - bounds.y <= knob::BAR);
                None
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_over(bounds)?;
                let e = if p.y - bounds.y <= knob::BAR { TileEvent::Grab(p) } else { TileEvent::Select };
                Some(canvas::Action::publish(Message::Tile(self.index, e)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &TileState, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let c = theme::c();
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let fade_by = if self.dim { knob::DIM } else { 1.0 } * if self.unused { 0.6 } else { 1.0 };
        let fade = |col: Color| theme::alpha(col, col.a * fade_by);
        let (w, h) = (bounds.width, bounds.height);

        let wash = if self.picked { theme::alpha(self.colour, 0.14) } else { theme::alpha(c.frame_ink, if state.hover { 0.05 } else { 0.03 }) };
        f.fill(&canvas::Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 8.0.into()), fade(wash));
        let bar = if self.unused && !self.picked {
            theme::alpha(c.frame_ink, if state.bar_hover { 0.09 } else { 0.05 })
        } else {
            theme::alpha(self.colour, if self.picked { 0.32 } else if state.bar_hover { 0.26 } else { 0.16 })
        };
        let ink = if self.unused && !self.picked { c.frame_ink_2 } else { knob::source_ink(self.colour) };
        knob::title_bar(&mut f, w, fade(bar), fade(ink), self.name, 6.0);

        // The mini display.
        let m = MINI;
        f.fill(&canvas::Path::rounded_rectangle(m.position(), m.size(), 4.0.into()), fade(theme::alpha(c.frame_ink, 0.04)));
        let stroke = canvas::Stroke::default().with_color(fade(self.colour)).with_width(1.5);
        let pad = 3.0;
        let (x0, x1, y0, y1) = (m.x + pad, m.x + m.width - pad, m.y + pad, m.y + m.height - pad);
        let y_of = |v: f32| y1 - v.clamp(0.0, 1.0) * (y1 - y0);
        match &self.mini {
            Mini::Lfo { shape, phase } => {
                let at = |p: f32| Point::new(x0 + p * (x1 - x0), y_of(0.5 + 0.5 * dsp::modulators::morph(*shape, p)));
                const N: usize = 48;
                let wave = canvas::Path::new(|b| {
                    b.move_to(at(0.0));
                    for k in 1..=N {
                        b.line_to(at(k as f32 / N as f32));
                    }
                });
                f.stroke(&wave, stroke);
                f.fill(&canvas::Path::circle(at(phase.clamp(0.0, 1.0)), 2.0), fade(Color::WHITE));
            }
            Mini::Step { values, current } => {
                let n = values.len().max(1);
                let slot = (x1 - x0) / n as f32;
                let mid = y_of(0.5);
                for (k, &v) in values.iter().enumerate() {
                    let (top, bottom) = (y_of(v).min(mid), y_of(v).max(mid));
                    let lit = k == *current;
                    let colour = theme::alpha(self.colour, if lit { 1.0 } else { 0.55 });
                    f.fill_rectangle(Point::new(x0 + k as f32 * slot + 0.5, top), Size::new((slot - 1.0).max(0.5), (bottom - top).max(1.0)), fade(colour));
                }
            }
            Mini::Follow(history) => {
                let n = history.len();
                if n >= 2 {
                    let at = |k: usize, v: f32| Point::new(x1 - (n - 1 - k) as f32 * (x1 - x0) / (HISTORY - 1) as f32, y_of(v));
                    let line = canvas::Path::new(|p| {
                        for (k, &v) in history.iter().enumerate() {
                            if k == 0 { p.move_to(at(k, v)) } else { p.line_to(at(k, v)) }
                        }
                    });
                    f.stroke(&line, stroke);
                    f.fill(&canvas::Path::circle(at(n - 1, *history.back().unwrap_or(&0.0)), 2.0), fade(Color::WHITE));
                }
            }
        }

        f.fill_text(canvas::Text {
            content: knob::fit(&self.value, w - 8.0, 9.0),
            position: Point::new(w / 2.0, m.y + m.height + (h - m.y - m.height) / 2.0),
            color: fade(c.frame_ink_2),
            size: Pixels(9.0),
            font: theme::font::MONO,
            align_x: Alignment::Center,
            align_y: alignment::Vertical::Center,
            ..canvas::Text::default()
        });
        if self.picked {
            let outline = canvas::Path::rounded_rectangle(Point::new(0.75, 0.75), Size::new(w - 1.5, h - 1.5), 7.5.into());
            f.stroke(&outline, canvas::Stroke::default().with_color(self.colour).with_width(1.5));
        }
        vec![f.into_geometry()]
    }

    fn mouse_interaction(&self, _: &TileState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        match cursor.position_over(bounds) {
            Some(p) if p.y - bounds.y <= knob::BAR => mouse::Interaction::Grab,
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::None,
        }
    }
}

// ---- the editor's parts ------------------------------------------------------------------------

/// One continuous setting of a modulator: how its knob maps to it and shows it.
struct Field {
    label: &'static str,
    lo: f32,
    hi: f32,
    /// Logarithmic knob travel (times, rates).
    log: bool,
    /// A rate that can follow the host tempo instead (`Hz | sync`).
    syncable: bool,
    unit: Unit,
    get: fn(&ModulatorSettings) -> f32,
    set: fn(&mut ModulatorSettings, f32),
}

#[derive(Clone, Copy)]
enum Unit {
    Ms,
    Hz,
    /// 0..1 shown as degrees.
    Degrees,
}

impl Field {
    fn norm(&self, v: f32) -> f32 {
        let v = v.clamp(self.lo, self.hi);
        if self.log { (v / self.lo).ln() / (self.hi / self.lo).ln() } else { (v - self.lo) / (self.hi - self.lo) }
    }

    fn plain(&self, n: f32) -> f32 {
        let n = n.clamp(0.0, 1.0);
        if self.log { self.lo * (self.hi / self.lo).powf(n) } else { self.lo + n * (self.hi - self.lo) }
    }

    fn text(&self, v: f32) -> String {
        match self.unit {
            Unit::Ms if v < 10.0 => format!("{v:.1} ms"),
            Unit::Ms if v < 1000.0 => format!("{v:.0} ms"),
            Unit::Ms => format!("{:.2} s", v / 1000.0),
            Unit::Hz if v < 1.0 => format!("{v:.2} Hz"),
            Unit::Hz if v < 10.0 => format!("{v:.1} Hz"),
            Unit::Hz => format!("{v:.0} Hz"),
            Unit::Degrees => format!("{:.0}°", v * 360.0),
        }
    }
}

/// The knobs of a modulator kind (ranges as the patch file allows; times and rates logarithmic).
fn fields(s: &ModulatorSettings) -> &'static [Field] {
    use ModulatorSettings::*;
    const FOLLOW: &[Field] = &[
        Field {
            label: "Attack",
            lo: 0.1,
            hi: 1000.0,
            log: true,
            syncable: false,
            unit: Unit::Ms,
            get: |s| if let Follow { attack_ms, .. } = s { *attack_ms } else { 0.0 },
            set: |s, v| if let Follow { attack_ms, .. } = s { *attack_ms = v },
        },
        Field {
            label: "Release",
            lo: 1.0,
            hi: 5000.0,
            log: true,
            syncable: false,
            unit: Unit::Ms,
            get: |s| if let Follow { release_ms, .. } = s { *release_ms } else { 0.0 },
            set: |s, v| if let Follow { release_ms, .. } = s { *release_ms = v },
        },
    ];
    const LFO: &[Field] = &[
        Field {
            label: "Rate",
            lo: 0.01,
            hi: 50.0,
            log: true,
            syncable: true,
            unit: Unit::Hz,
            get: |s| if let Lfo { rate_hz, .. } = s { *rate_hz } else { 0.0 },
            set: |s, v| if let Lfo { rate_hz, .. } = s { *rate_hz = v },
        },
        Field {
            label: "Phase",
            lo: 0.0,
            hi: 1.0,
            log: false,
            syncable: false,
            unit: Unit::Degrees,
            get: |s| if let Lfo { phase, .. } = s { *phase } else { 0.0 },
            set: |s, v| if let Lfo { phase, .. } = s { *phase = v },
        },
    ];
    const STEP: &[Field] = &[Field {
        label: "Rate",
        lo: 0.01,
        hi: 50.0,
        log: true,
        syncable: true,
        unit: Unit::Hz,
        get: |s| if let Step { rate_hz, .. } = s { *rate_hz } else { 0.0 },
        set: |s, v| if let Step { rate_hz, .. } = s { *rate_hz = v },
    }];
    match s {
        Follow { .. } => FOLLOW,
        Lfo { .. } => LFO,
        Step { .. } => STEP,
    }
}

/// The note lengths a synced rate steps through, slowest first (the knob's left end), in beats.
pub const NOTES: [(&str, f32); 17] = [
    ("4 bars", 16.0),
    ("2 bars", 8.0),
    ("1 bar", 4.0),
    ("1/2.", 3.0),
    ("1/2", 2.0),
    ("1/4.", 1.5),
    ("1/2T", 4.0 / 3.0),
    ("1/4", 1.0),
    ("1/8.", 0.75),
    ("1/4T", 2.0 / 3.0),
    ("1/8", 0.5),
    ("1/16.", 0.375),
    ("1/8T", 1.0 / 3.0),
    ("1/16", 0.25),
    ("1/16T", 1.0 / 6.0),
    ("1/32", 0.125),
    ("1/64", 0.0625),
];

/// The note nearest `beats` (a patch file may say "3/16").
fn note_index(beats: f32) -> usize {
    (0..NOTES.len()).min_by(|&a, &b| (NOTES[a].1 / beats).ln().abs().total_cmp(&(NOTES[b].1 / beats).ln().abs())).unwrap_or(0)
}

fn note_norm(k: usize) -> f32 {
    k as f32 / (NOTES.len() - 1) as f32
}

fn note_at(n: f32) -> usize {
    (n.clamp(0.0, 1.0) * (NOTES.len() - 1) as f32).round() as usize
}

/// The modulator's synced rate (beats), if it follows the host tempo.
fn sync(s: &ModulatorSettings) -> Option<f32> {
    match s {
        ModulatorSettings::Lfo { sync, .. } | ModulatorSettings::Step { sync, .. } => *sync,
        ModulatorSettings::Follow { .. } => None,
    }
}

fn set_sync(s: &mut ModulatorSettings, beats: Option<f32>) {
    if let ModulatorSettings::Lfo { sync, .. } | ModulatorSettings::Step { sync, .. } = s {
        *sync = beats;
    }
}

/// Sine | Tri | Square.
fn shapes<'a>(current: ModulatorLfoShape) -> Element<'a, Message> {
    use ModulatorLfoShape::*;
    let pill = |shape: ModulatorLfoShape, label: &'static str| -> Element<'a, Message> {
        let on = shape == current;
        button(container(text(label).size(10.5).font(theme::font::SANS_MEDIUM)).center(Fill))
            .on_press_maybe((!on).then_some(Message::Shape(shape)))
            .width(Fill)
            .height(22)
            .padding(0)
            .style(theme::frame_tab(on))
            .into()
    };
    container(row![pill(Sine, "Sine"), pill(Triangle, "Tri"), pill(Square, "Square")].spacing(1))
        .width(168)
        .padding(2)
        .style(theme::frame_track)
        .into()
}

/// One cycle of the LFO's wave (what the audio thread plays, `dsp::modulators::morph`) and a dot
/// riding it at the current phase.
struct Cycle {
    /// Morph position (`dsp::lfo_shape`).
    shape: f32,
    phase: f32,
    colour: Color,
}

impl canvas::Program<Message> for Cycle {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        f.fill(&canvas::Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 6.0.into()), theme::c().frame_control);
        f.stroke(&canvas::Path::line(Point::new(0.0, h / 2.0), Point::new(w, h / 2.0)), canvas::Stroke::default().with_color(theme::alpha(theme::c().frame_ink, 0.08)).with_width(1.0));
        let pad = 5.0;
        let at = |p: f32| {
            let v = dsp::modulators::morph(self.shape, p);
            Point::new(pad + p * (w - 2.0 * pad), h / 2.0 - v * (h / 2.0 - pad))
        };
        const N: usize = 128;
        let wave = canvas::Path::new(|b| {
            b.move_to(at(0.0));
            for k in 1..=N {
                b.line_to(at(k as f32 / N as f32));
            }
        });
        f.stroke(&wave, canvas::Stroke::default().with_color(self.colour).with_width(1.6));
        let dot = at(self.phase.clamp(0.0, 1.0));
        f.stroke(&canvas::Path::line(Point::new(dot.x, pad), Point::new(dot.x, h - pad)), canvas::Stroke::default().with_color(theme::alpha(self.colour, 0.25)).with_width(1.0));
        f.fill(&canvas::Path::circle(dot, 5.0), theme::alpha(self.colour, 0.25));
        f.fill(&canvas::Path::circle(dot, 3.0), Color::WHITE);
        vec![f.into_geometry()]
    }
}

/// Follow's output over the last ~2 s, newest at the right.
struct Scope<'a> {
    history: &'a VecDeque<f32>,
    colour: Color,
}

impl canvas::Program<Message> for Scope<'_> {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        f.fill(&canvas::Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 6.0.into()), theme::c().frame_control);
        f.stroke(&canvas::Path::line(Point::new(0.0, h / 2.0), Point::new(w, h / 2.0)), canvas::Stroke::default().with_color(theme::alpha(theme::c().frame_ink, 0.08)).with_width(1.0));
        let n = self.history.len();
        if n >= 2 {
            let pad = 4.0;
            let at = |i: usize, v: f32| Point::new(w - (n - 1 - i) as f32 * w / (HISTORY - 1) as f32, pad + (1.0 - v.clamp(0.0, 1.0)) * (h - 2.0 * pad));
            let line = canvas::Path::new(|p| {
                for (i, &v) in self.history.iter().enumerate() {
                    if i == 0 { p.move_to(at(i, v)) } else { p.line_to(at(i, v)) }
                }
            });
            f.stroke(&line, canvas::Stroke::default().with_color(self.colour).with_width(1.6));
            let last = at(n - 1, *self.history.back().unwrap());
            f.fill(&canvas::Path::circle(last, 2.5), self.colour);
        }
        vec![f.into_geometry()]
    }
}

/// The step levels as bars around the middle line (0.5 = no change); drag across them to draw.
/// The step playing is lit.
struct StepBars<'a> {
    values: &'a [f32],
    colour: Color,
    current: usize,
}

impl canvas::Program<Message> for StepBars<'_> {
    /// Drawing (the left button went down on the bars).
    type State = bool;

    fn update(&self, drawing: &mut bool, event: &canvas::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        let n = self.values.len().max(1);
        let at = |p: Point| {
            let i = (((p.x - bounds.x) / bounds.width * n as f32).floor().max(0.0) as usize).min(n - 1);
            let v = 1.0 - ((p.y - bounds.y) / bounds.height).clamp(0.0, 1.0);
            canvas::Action::publish(Message::StepValue(i, v)).and_capture()
        };
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_over(bounds)?;
                *drawing = true;
                Some(at(p))
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) if *drawing => Some(at(*position)),
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if *drawing => {
                *drawing = false;
                None
            }
            _ => None,
        }
    }

    fn draw(&self, _: &bool, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let n = self.values.len().max(1);
        let slot = w / n as f32;
        let y = |v: f32| (1.0 - v.clamp(0.0, 1.0)) * h;
        for (i, &v) in self.values.iter().enumerate() {
            let x = i as f32 * slot + 1.0;
            let bw = slot - 2.0;
            let playing = i == self.current;
            let slot_bg = if playing { theme::alpha(self.colour, 0.14) } else { theme::c().frame_control };
            f.fill(&canvas::Path::rounded_rectangle(Point::new(x, 0.0), Size::new(bw, h), 3.0.into()), slot_bg);
            let (top, bottom) = (y(v).min(h / 2.0), y(v).max(h / 2.0));
            f.fill_rectangle(Point::new(x, top), Size::new(bw, (bottom - top).max(1.5)), theme::alpha(self.colour, if playing { 1.0 } else { 0.6 }));
        }
        f.stroke(&canvas::Path::line(Point::new(0.0, h / 2.0), Point::new(w, h / 2.0)), canvas::Stroke::default().with_color(theme::alpha(theme::c().frame_ink, 0.2)).with_width(1.0));
        vec![f.into_geometry()]
    }

    fn mouse_interaction(&self, _: &bool, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if cursor.is_over(bounds) { mouse::Interaction::Crosshair } else { mouse::Interaction::None }
    }
}

