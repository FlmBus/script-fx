//! The drawer: a tab of frame material rising from the patch window's bottom edge, joined to it
//! by concave flares (the notch's mirror image). Collapsed, it hangs into the ControlPanel
//! by its handle and tab row, like the notch at the top. Open, it rises further over the panel.
//!
//! Open, it shows (see `docs/research/chain-editor.md`):
//! - the tabs: the fixed modulator set (Follow, LFO 1–3, Step), each with a grip in its colour;
//!   the highlight slides between them. **Drag a grip onto a knob** (a block page's knob, or a
//!   macro in the rail) to modulate it;
//! - the open modulator: its controls and what it does right now:
//!   - Follow: a scope of its output (an envelope is slow enough to sample once per frame);
//!   - LFO: the shape (sine | tri | square) over one cycle of the wave, a dot at the phase the
//!     audio thread is at; rate, phase. (Not a scope: sampling a fast LFO once per UI frame
//!     aliases into noise.)
//!   - Step: the steps as bars (drag across them to draw), the playing one lit; rate.
//!
//! LFO and Step rates have two modes (`Hz | sync`, the pills on the Rate knob): free in Hz, or a
//! note length at the host tempo; in sync the knob steps through [`NOTES`].
//!
//! Changes apply live and are saved with the project (`Session::set_modulator`).
//! Handle: open/close. A tab: open the drawer on that modulator; the active tab again closes it.
//! The macros live in the left rail (`macro_rail`).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use patch::{ModulatorId, ModulatorLfoShape, ModulatorSettings, ModulatorSpec};

use super::{icons, knob, tabs, theme};
use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, Padding, Point, Rectangle, Renderer, Shadow, Size, Theme, Vector,
    animation::{Animation, Easing},
    color, mouse,
    widget::{Row, button, canvas, column, container, mouse_area, row, space, stack, text},
};

/// The fixed modulator set in tab order (= `ModulatorId::ALL`), each with its colour (fixed,
/// never a patch colour).
pub const MODULATORS: [(&str, Color); 5] = [
    ("Follow", color!(0x60a5fa)),
    ("LFO 1", color!(0x2dd4bf)),
    ("LFO 2", color!(0xf472b6)),
    ("LFO 3", color!(0xa78bfa)),
    ("Step", color!(0xa3e635)),
];

/// Frames of output history the scopes show (≈ 2 s at the shell's frame rate).
const HISTORY: usize = 120;

pub struct Drawer {
    open: bool,
    /// Open/close animation (the height between the strip and the open drawer).
    slide: Animation<bool>,
    /// Index into [`MODULATORS`] of the active tab.
    selected: usize,
    /// Follow's recent output (0..1, one value per frame), newest last; the scope.
    history: VecDeque<f32>,
    /// Each modulator's cycle position this frame (`Values::modulator_phases`).
    phases: [f32; 5],
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The handle: open or close.
    Toggle,
    /// A tab: open on that modulator (or close, if it's the active tab of the open drawer).
    Select(usize),
    /// A tab's grip was pressed: drag that modulator.
    Grab(usize),
    /// Input on one of the open modulator's knobs (index into its [`fields`]).
    Knob(usize, knob::KnobEvent),
    Shape(ModulatorLfoShape),
    /// Step `i` set to a level (0..1).
    StepValue(usize, f32),
}

impl From<(usize, knob::KnobEvent)> for Message {
    fn from((i, e): (usize, knob::KnobEvent)) -> Self {
        Message::Knob(i, e)
    }
}

/// What the shell does for the drawer.
pub enum Event {
    /// Start dragging this modulator (drag & drop onto a knob).
    Grab(ModulatorId),
    /// New settings for a modulator.
    Set(ModulatorSpec),
    /// A knob drag starts / ends (the shell hides the cursor meanwhile).
    Dragging(bool),
}

/// The open drawer's body under the tab row: the modulator's title and controls.
const BODY_HEIGHT: f32 = 120.0;

impl Drawer {
    pub fn new() -> Self {
        Self {
            open: false,
            slide: Animation::new(false).duration(Duration::from_millis(260)).easing(Easing::EaseOutCubic),
            selected: 1,
            history: VecDeque::with_capacity(HISTORY),
            phases: [0.0; 5],
        }
    }

    /// `mods`: the patch's modulators (`ModulatorId::ALL` order), what knob input changes.
    pub fn update(&mut self, message: Message, now: Instant, mods: &[ModulatorSpec]) -> Option<Event> {
        match message {
            Message::Toggle => self.set_open(!self.open, now),
            Message::Select(i) if self.open && i == self.selected => self.set_open(false, now),
            Message::Select(i) => {
                self.selected = i;
                self.set_open(true, now);
            }
            Message::Grab(i) => return Some(Event::Grab(ModulatorId::ALL[i])),
            Message::Knob(f, e) => {
                let mut spec = mods.get(self.selected)?.clone();
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
                let mut spec = mods.get(self.selected)?.clone();
                if let ModulatorSettings::Lfo { shape: s, .. } = &mut spec.settings {
                    *s = shape;
                }
                return Some(Event::Set(spec));
            }
            Message::StepValue(i, v) => {
                let mut spec = mods.get(self.selected)?.clone();
                if let ModulatorSettings::Step { values, .. } = &mut spec.settings {
                    *values.get_mut(i)? = v.clamp(0.0, 1.0);
                }
                return Some(Event::Set(spec));
            }
        }
        None
    }

    /// Once per frame: the modulators' outputs and cycle positions.
    pub fn tick(&mut self, live: &[f32; 5], phases: &[f32; 5]) {
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(live[ModulatorId::Follow.index()]);
        self.phases = *phases;
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

    /// The drawer's height now (it animates between the strip and open).
    pub fn height(&self, now: Instant) -> f32 {
        let t: f32 = self.slide.interpolate(0.0, 1.0, now);
        theme::SIZE_DRAWER_BAR + t * (theme::SIZE_DRAWER_OPEN - theme::SIZE_DRAWER_BAR)
    }

    /// The drawer at its current height, `drawer_width` wide plus the flares.
    pub fn view<'a>(&'a self, now: Instant, mods: &'a [ModulatorSpec]) -> Element<'a, Message> {
        let height = self.height(now);
        let width = theme::drawer_width();
        let r = theme::RADIUS_WINDOW;

        // The body sits under the tabs at its full height; the drawer's height reveals it.
        let body = container(self.modulator(mods)).height(BODY_HEIGHT);
        let content = container(
            column![handle(), self.tabs(), space::vertical().height(8), body]
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

        // `Idle` marks the drawer as occupied: clicks on it don't fall through to the page.
        mouse_area(drawer).interaction(mouse::Interaction::Idle).into()
    }

    /// The modulator tabs, full width, in a recessed track. The highlight slides between them
    /// while the drawer is open; closed, no tab is selected (like the notch's switch). The grip
    /// is a drag handle of its own (pressing it doesn't select the tab).
    fn tabs(&self) -> Element<'_, Message> {
        let tab = |(i, (name, colour)): (usize, &(&'static str, Color)), on: bool| -> Element<'_, Message> {
            let font = if on { theme::font::SANS_SEMIBOLD } else { theme::font::SANS_MEDIUM };
            let grip = mouse_area(container(icons::icon(icons::GRIP, 12.0, 0.0, *colour)).padding([4, 3]))
                .on_press(Message::Grab(i))
                .interaction(mouse::Interaction::Grab);
            button(container(row![grip, text(*name).size(11).font(font)].spacing(2).align_y(Center)).center(Fill))
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

    // ---- the open modulator ------------------------------------------------------------------

    /// The open modulator: title, then its display (scope or steps) left and its knobs right.
    fn modulator<'a>(&'a self, mods: &'a [ModulatorSpec]) -> Element<'a, Message> {
        let i = self.selected;
        let (name, colour) = MODULATORS[i];
        let Some(spec) = mods.get(i) else { return column![].into() };
        let defaults = ModulatorSpec::default_for(spec.id);
        let kind = match spec.settings {
            ModulatorSettings::Follow { .. } => "Envelope follower · drag the grip onto a knob",
            ModulatorSettings::Lfo { .. } => "LFO · drag the grip onto a knob",
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
            ModulatorSettings::Lfo { shape, .. } => {
                let cycle = canvas(Cycle { shape: dsp::lfo_shape(*shape), phase: self.phases[i], colour }).width(Fill).height(Fill);
                column![shapes(*shape), cycle].spacing(6).into()
            }
            ModulatorSettings::Step { values, .. } => {
                let current = self.phases[i] as usize;
                canvas(StepBars { values, colour, current }).width(Fill).height(Fill).into()
            }
        };

        let title = row![
            text(format!("{name}")).size(11).font(theme::font::SANS_SEMIBOLD).color(colour),
            text(kind).size(11).color(theme::c().frame_muted),
        ]
        .spacing(8);
        column![title, row![container(display).width(Fill).height(knob::PARAM_SIZE.height), knobs].spacing(12)]
            .spacing(6)
            .width(Fill)
            .into()
    }
}

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
