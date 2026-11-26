//! The shell's knob cells. One canvas per cell draws everything and handles the input, so a cell
//! is one widget: background, knob, tag, label.
//!
//! - [`Macro`]: the mod dock's macro cell (54 × 74, yellow, no ticks): title bar, knob, slot, value.
//! - [`Param`]: a block knob (64 × 92, tick ring), on the block page; the surface shows macros
//!   with it too, larger and without the macro look. A modulated knob still turns: it edits the
//!   base value. Serum's layers (`docs/research/chain-editor.md`): ticks (lit to the base), the
//!   **range arc** of the focused route (base → base + amount; bipolar: ±amount/2 around the
//!   base, the same size; its end is a handle that drags the amount), the knob at the base, and the **live dot**: a soft white
//!   dot at the current value after all modulation. The top line (18 px) carries one **meta
//!   knob** per route in its source's colour (macro yellow, the modulators' colours; amount
//!   −100…+100 % from the top): press = focus its range arc, drag = amount, double-click = reset,
//!   wheel steps, right-click = the menu. Hovering one shows "Heat +45%" in place of the label.
//!   Up to three fit at full size, up to five smaller; beyond that the last place says "+N"
//!   (the Matrix lists them all). A knob with two modes (`Hz | sync`) shows them as a pill pair
//!   at the right of the top line; pressing one switches ([`KnobEvent::Mode`]).
//!
//! Drag & drop: while a source (a macro, a modulator) is dragged, cells that can take it get
//! `drop` (the source's colour): they outline themselves, and a release over one reports
//! [`KnobEvent::Drop`]. A macro cell's title bar is its grip: pressing it starts dragging the
//! macro ([`KnobEvent::Grab`]) instead of turning the knob.
//!
//! Input, as on a patch surface: drag up/down (200 px = full range), Shift = fine, wheel steps,
//! double-click = default, right-click = type a value. While dragging, the value replaces the
//! label.

use std::f32::consts::PI;
use std::time::{Duration, Instant};

use patch::ModSettings;

use super::pointer_lock::Drag;
use super::theme;
use nice_plug_iced::iced::{
    Color, Element, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector,
    alignment, keyboard, mouse,
    widget::{
        canvas::{self, Action, Event, Frame, Path, Stroke, Text, path::Arc},
        text::Alignment,
    },
};

/// What a knob asks of the shell. Values are normalized 0..1.
#[derive(Debug, Clone)]
pub enum KnobEvent {
    /// A drag starts (host gesture begins).
    Begin,
    Set(f32),
    /// The drag ends (host gesture ends).
    End,
    /// One-shot change (wheel, double-click): begin, set, end in one.
    Step(f32),
    /// Right-click: open the value entry at this window point.
    Entry(Point),
    /// Input on the knob's `k`-th meta knob (param cells only).
    Meta(usize, MetaEvent),
    /// The dragged source was released over this cell.
    Drop,
    /// The macro cell's title bar was pressed (at this window point): start dragging the macro.
    Grab(Point),
    /// A mode pill was pressed: `true` = the second mode (sync).
    Mode(bool),
}

/// What a meta knob asks of the shell.
#[derive(Debug, Clone)]
pub enum MetaEvent {
    /// New amount (−1..+1).
    Amount(f32),
    /// Double-click: back to what the patch defines.
    Reset,
    /// A drag starts / ends (the shell hides the cursor meanwhile).
    Begin,
    End,
    /// Right-click: open the meta menu at this window point.
    Menu(Point),
}

/// Macro cell size (the mod dock's cell: title bar, knob, slot and value).
pub const MACRO_SIZE: Size = Size::new(54.0, 74.0);
/// The cell's title bar: the macro's name and a grip, the drag handle.
const MACRO_BAR: f32 = 16.0;

/// The macro colour (fixed, never a patch colour).
pub const MACRO: Color = Color::from_rgb(0xfa as f32 / 255.0, 0xcc as f32 / 255.0, 0x15 as f32 / 255.0);

const DRAG_RANGE: f32 = 200.0;
const SCROLL_STEP: f32 = 0.02;
const FINE: f32 = 0.2;

/// The knob's sweep: 270°, starting at 7:30 (in canvas angles, y down).
const START: f32 = 0.75 * PI;
const SWEEP: f32 = 1.5 * PI;

/// A macro cell in the mod dock: a title bar in macro yellow (its name and a grip: drag it onto
/// a knob), a 30 px knob that turns the macro, the slot ("M3") and the value.
pub struct Macro<'a> {
    pub slot: usize,
    pub label: &'a str,
    /// The value as text, in the target param's units.
    pub value_text: String,
    /// Normalized.
    pub value: f32,
    /// Normalized value after the modulators on it (= `value` without any).
    pub effective: f32,
    pub default: f32,
    /// A modulator is being dragged and could go here: its colour.
    pub drop: Option<Color>,
    /// The focused source in the dock: outlined.
    pub picked: bool,
    /// Another source is focused: drawn faded.
    pub dim: bool,
    /// The modulators that modulate the macro, one meta knob each (beside the knob).
    pub mods: Vec<Meta<'a>>,
}

#[derive(Default)]
pub struct State {
    drag: Option<Drag>,
    fine: bool,
    hover: bool,
    last_press: Option<Instant>,
    /// A meta knob being dragged: which one, and the drag's start.
    meta_drag: Option<(usize, Drag)>,
    /// The meta knob under the pointer.
    meta_hover: Option<usize>,
    /// The slot whose range arc shows (param cells).
    focus: usize,
    /// The pointer is over the title bar (macro cells).
    bar_hover: bool,
}


/// The input every cell shares: drag, Shift, double-click, right-click, wheel. `value` and
/// `default` normalized; `None` = nothing for the shell.
fn input(state: &mut State, event: &Event, bounds: Rectangle, cursor: mouse::Cursor, value: f32, default: f32) -> Option<KnobEvent> {
    match event {
        Event::Mouse(mouse::Event::CursorMoved { position }) => {
            state.hover = cursor.is_over(bounds);
            let d = state.drag.as_mut()?;
            let fine = if state.fine { FINE } else { 1.0 };
            // Relative to the last event (the pointer stays locked where it grabbed).
            Some(KnobEvent::Set(d.moved(position.y, fine / DRAG_RANGE, 0.0..=1.0)))
        }
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            let p = cursor.position_over(bounds)?;
            let double = state.last_press.is_some_and(|t| t.elapsed() < Duration::from_millis(350));
            state.last_press = Some(Instant::now());
            if double {
                state.drag = None;
                return Some(KnobEvent::Step(default));
            }
            state.drag = Some(Drag::start(value, p.y));
            Some(KnobEvent::Begin)
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            state.drag.take()?;
            Some(KnobEvent::End)
        }
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => Some(KnobEvent::Entry(cursor.position_over(bounds)?)),
        Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
            cursor.position_over(bounds)?;
            if state.drag.is_some() {
                return None;
            }
            let notches = match *delta {
                mouse::ScrollDelta::Lines { x, y } => if y != 0.0 { y } else { x },
                mouse::ScrollDelta::Pixels { x, y } => (if y != 0.0 { y } else { x }) / 40.0,
            };
            let step = notches * SCROLL_STEP * if state.fine { FINE } else { 1.0 };
            Some(KnobEvent::Step((value + step).clamp(0.0, 1.0)))
        }
        Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
            state.fine = m.shift();
            None
        }
        _ => None,
    }
}

impl<'a, M: 'a> canvas::Program<M> for Macro<'a>
where
    M: From<(usize, KnobEvent)>,
{
    type State = State;

    fn update(&self, state: &mut State, event: &Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Action<M>> {
        if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event {
            state.bar_hover = cursor.position_over(bounds).is_some_and(|p| on_bar(bounds, p));
        }
        if let Some(e) = drop_input(self.drop, state, event, bounds, cursor) {
            return e.map(|e| Action::publish(M::from((self.slot, e))).and_capture());
        }
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && let Some(p) = cursor.position_over(bounds).filter(|p| on_bar(bounds, *p))
        {
            return Some(Action::publish(M::from((self.slot, KnobEvent::Grab(p)))).and_capture());
        }
        if let Some(action) = meta_input(&MACRO_DIALS, self.slot, &self.mods, self.value, state, event, bounds, cursor) {
            return Some(action);
        }
        let e = input(state, event, bounds, cursor, self.value, self.default)?;
        Some(Action::publish(M::from((self.slot, e))).and_capture())
    }

    fn draw(&self, state: &State, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let c = theme::c();
        let mut f = Frame::new(renderer, bounds.size());
        f.scale(bounds.width / MACRO_SIZE.width);
        let (w, h) = (MACRO_SIZE.width, MACRO_SIZE.height);
        let fade = |col: Color| if self.dim { theme::alpha(col, col.a * DIM) } else { col };

        // Cell: a faint yellow wash, stronger when picked, a touch lighter on hover or while dragging.
        let wash = if self.picked { 0.10 } else if state.hover || state.drag.is_some() { 0.07 } else { 0.04 };
        f.fill(&Path::rounded_rectangle(Point::ORIGIN, MACRO_SIZE, 8.0.into()), fade(theme::alpha(MACRO, wash)));

        // The title bar: name and grip.
        let bar = if self.picked { 0.32 } else if state.bar_hover { 0.26 } else { 0.16 };
        title_bar(&mut f, w, fade(theme::alpha(MACRO, bar)), fade(source_ink(MACRO)), &fit(self.label, w - 24.0, 8.5), 5.0);

        // Knob (30 px): track, yellow value arc, disc, pointer.
        let centre = Point::new(w / 2.0, MACRO_BAR + 22.0);
        let arc = |r: f32, from: f32, to: f32| {
            Path::new(|p| p.arc(Arc { center: centre, radius: r, start_angle: from.into(), end_angle: to.into() }))
        };
        let stroke = |color: Color, width: f32| Stroke::default().with_color(color).with_width(width).with_line_cap(canvas::LineCap::Round);
        f.stroke(&arc(10.5, START, START + SWEEP), stroke(fade(theme::alpha(c.frame_ink, 0.1)), 2.5));
        let angle = START + SWEEP * self.value;
        if self.value > 0.001 {
            f.stroke(&arc(10.5, START, angle), stroke(fade(MACRO), 2.5));
        }
        f.fill(&Path::circle(centre, 6.8), fade(theme::mix(c.frame, c.frame_ink, 0.1)));
        let dir = Vector::new(angle.cos(), angle.sin());
        f.stroke(&Path::line(centre + dir * 2.0, centre + dir * 6.0), stroke(fade(c.frame_ink), 1.5));

        // A meta knob per modulator on the macro, beside the knob; the focused one's range arc
        // just outside the track.
        let active = state.meta_drag.as_ref().map(|(k, _)| *k).or(state.meta_hover);
        let n = self.mods.len();
        let focused = focus(state, n);
        let shown = macro_shown(n);
        for (k, m) in self.mods.iter().take(shown).enumerate() {
            let (at, r) = macro_place(k, n);
            meta_dial(&mut f, at, r, m, k == focused, active == Some(k));
        }
        if shown < n {
            f.fill_text(Text {
                content: format!("+{}", n - shown),
                position: macro_place(shown, n).0,
                color: c.frame_muted,
                size: Pixels(7.5),
                font: theme::font::MONO,
                align_x: Alignment::Center,
                align_y: alignment::Vertical::Center,
                ..Text::default()
            });
        }
        if let Some(m) = self.mods.get(focused) {
            let s = m.settings;
            let at = |v: f32| START + SWEEP * v.clamp(0.0, 1.0);
            let ink = theme::alpha(m.colour, if s.bypass { 0.3 } else { 0.85 });
            let reach = reach(s);
            for to in [Some(self.value + reach), s.bipolar.then_some(self.value - reach)].into_iter().flatten() {
                let (a, b) = (at(self.value), at(to));
                if (b - a).abs() > 1e-3 {
                    f.stroke(&arc(MACRO_RANGE_R, a.min(b), a.max(b)), stroke(ink, 1.5));
                }
            }
            // Live value (all modulators summed): a soft white dot riding the outer ring.
            let live = at(self.effective);
            let dot = centre + Vector::new(live.cos(), live.sin()) * MACRO_RANGE_R;
            f.fill(&Path::circle(dot, 3.0), theme::alpha(m.colour, 0.16));
            f.fill(&Path::circle(dot, 2.1), Color { a: 0.35, ..Color::WHITE });
            f.fill(&Path::circle(dot, 1.2), Color::WHITE);
        }

        // The slot, and the value (bright while dragging).
        let row = h - 9.0;
        let mono = |content: String, x: f32, color: Color, align_x: Alignment| Text {
            content,
            position: Point::new(x, row),
            color,
            size: Pixels(8.5),
            font: theme::font::MONO,
            align_x,
            align_y: alignment::Vertical::Center,
            ..Text::default()
        };
        if let Some(m) = active.and_then(|k| self.mods.get(k)) {
            // A meta knob under the pointer: its source and amount in place of slot and value.
            f.fill_text(Text { content: fit(&meta_text(m), w - 6.0, 8.0), position: Point::new(w / 2.0, row), color: m.colour, size: Pixels(8.0), ..mono(String::new(), 0.0, m.colour, Alignment::Center) });
        } else {
            f.fill_text(mono(format!("M{}", self.slot + 1), 5.0, fade(theme::alpha(source_ink(MACRO), 0.8)), Alignment::Left));
            let ink = if state.drag.is_some() { c.frame_ink } else { c.frame_ink_2 };
            f.fill_text(mono(fit(&self.value_text, w - 22.0, 8.5), w - 5.0, fade(ink), Alignment::Right));
        }

        if self.picked {
            let outline = Path::rounded_rectangle(Point::new(0.75, 0.75), Size::new(w - 1.5, h - 1.5), 7.5.into());
            f.stroke(&outline, Stroke::default().with_color(MACRO).with_width(1.5));
        }
        drop_outline(&mut f, MACRO_SIZE, self.drop, state.hover);
        vec![f.into_geometry()]
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        // No hand over a knob; hidden while dragging (the shell hides it window-wide too).
        if state.drag.is_some() || state.meta_drag.is_some() {
            mouse::Interaction::Hidden
        } else if cursor.position_over(bounds).is_some_and(|p| on_bar(bounds, p)) {
            mouse::Interaction::Grab
        } else if cursor.position_over(bounds).and_then(|p| meta_at(&MACRO_DIALS, &self.mods, self.value, state, bounds, p)).is_some() {
            mouse::Interaction::ResizingVertically
        } else {
            mouse::Interaction::None
        }
    }
}

/// The macro cell's meta knobs: beside the knob, left then right, two rows (four places; with
/// more, the fourth says "+N").
fn macro_place(k: usize, _n: usize) -> (Point, f32) {
    let x = if k % 2 == 0 { 6.5 } else { MACRO_SIZE.width - 6.5 };
    let y = MACRO_BAR + if k < 2 { 13.0 } else { 29.0 };
    (Point::new(x, y), 4.2)
}

fn macro_shown(n: usize) -> usize {
    if n <= 4 { n } else { 3 }
}

/// The macro knob's range arc: just outside its track.
const MACRO_RANGE_R: f32 = 13.8;

const MACRO_DIALS: Dials = Dials { size: MACRO_SIZE, place: macro_place, shown: macro_shown, handle: None };

/// How much a source fades while another one is focused in the dock.
pub const DIM: f32 = 0.55;

fn on_bar(bounds: Rectangle, p: Point) -> bool {
    let scale = bounds.width / MACRO_SIZE.width;
    (p.y - bounds.y) / scale <= MACRO_BAR
}

/// A dock cell's title bar, `MACRO_BAR` high across the top of a `w` wide cell (top corners
/// rounded): the source's name at the left, a grip (three lines) at the right.
pub fn title_bar(f: &mut Frame, w: f32, fill: Color, ink: Color, name: &str, pad: f32) {
    let radius = nice_plug_iced::iced::border::Radius { top_left: 8.0, top_right: 8.0, bottom_right: 0.0, bottom_left: 0.0 };
    f.fill(&Path::rounded_rectangle(Point::ORIGIN, Size::new(w, MACRO_BAR), radius), fill);
    f.fill_text(Text {
        content: name.to_string(),
        position: Point::new(pad, MACRO_BAR / 2.0),
        color: ink,
        size: Pixels(8.5),
        font: theme::font::SANS_SEMIBOLD,
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });
    let grip = Stroke::default().with_color(theme::alpha(ink, ink.a * 0.8)).with_width(1.2).with_line_cap(canvas::LineCap::Round);
    for dy in [-2.5, 0.0, 2.5] {
        let y = MACRO_BAR / 2.0 + dy;
        f.stroke(&Path::line(Point::new(w - pad - 8.0, y), Point::new(w - pad, y)), grip.clone());
    }
}

/// A title bar's height.
pub const BAR: f32 = MACRO_BAR;

/// A source's ink on its title bar: its colour, lightened towards the frame's ink (pale yellow
/// on the dark frame, a deeper tone on the light one).
pub fn source_ink(colour: Color) -> Color {
    theme::mix(colour, theme::c().frame_ink, 0.3)
}

/// While a source is dragged (`drop`): hover tracking, and a release over the cell is a drop.
/// `None` = not dragging (the cell's own input runs); `Some(None)` = swallowed.
fn drop_input(drop: Option<Color>, state: &mut State, event: &Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Option<KnobEvent>> {
    drop?;
    Some(match event {
        Event::Mouse(mouse::Event::CursorMoved { .. }) => {
            state.hover = cursor.is_over(bounds);
            None
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if cursor.is_over(bounds) => {
            state.hover = false;
            Some(KnobEvent::Drop)
        }
        _ => None,
    })
}

/// A cell that can take the dragged source: a faint outline in its colour, strong under the pointer.
fn drop_outline(f: &mut Frame, size: Size, drop: Option<Color>, hover: bool) {
    let Some(colour) = drop else { return };
    let outline = Path::rounded_rectangle(Point::new(0.75, 0.75), Size::new(size.width - 1.5, size.height - 1.5), 7.5.into());
    if hover {
        f.fill(&outline, theme::alpha(colour, 0.12));
    }
    f.stroke(&outline, Stroke::default().with_color(theme::alpha(colour, if hover { 0.9 } else { 0.35 })).with_width(1.5));
}

/// `s` cut with "…" to about `width` at font `size` (average glyph ≈ 0.56 em; canvas text has no
/// clipping or measuring). TODO: scroll long labels (concept), once we measure text.
pub fn fit(s: &str, width: f32, size: f32) -> String {
    let max = (width / (size * 0.56)).floor() as usize;
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The macro cell as an element.
pub fn macro_cell<'a, M: From<(usize, KnobEvent)> + 'a>(m: Macro<'a>) -> Element<'a, M> {
    macro_cell_scaled(m, 1.0)
}

/// The macro cell, `scale` times its size (everything in it grows alike).
pub fn macro_cell_scaled<'a, M: From<(usize, KnobEvent)> + 'a>(m: Macro<'a>, scale: f32) -> Element<'a, M> {
    canvas::Canvas::new(m).width(MACRO_SIZE.width * scale).height(MACRO_SIZE.height * scale).into()
}

// ---- param cell --------------------------------------------------------------------------------

/// Param cell size.
pub const PARAM_SIZE: Size = Size::new(64.0, 92.0);

/// A block knob: tick ring, disc, pointer, label (the value while dragging or hovering).
/// `mods`: the macros that modulate it, one meta knob each. The knob edits the base value; the
/// modulation arc (yellow, just outside the ticks) runs from the base to where the macros put the
/// param right now, with a dot at that end.
pub struct Param<'a> {
    /// The param's index (what the shell gets back with the events).
    pub index: usize,
    pub label: &'a str,
    pub value_text: String,
    /// Normalized base value.
    pub value: f32,
    /// Normalized value after modulation (= `value` when nothing modulates it).
    pub effective: f32,
    pub default: f32,
    pub mods: Vec<Meta<'a>>,
    /// A source is being dragged and could go here: its colour.
    pub drop: Option<Color>,
    /// Two modes for the value (`Hz | sync`), `None` = one.
    pub modes: Option<Modes<'a>>,
    /// The patch's colours (the surface): lit ticks and pointer in `accent`, label and value in
    /// `ink`. `None` = the shell's.
    pub colours: Option<Colours>,
}

/// A knob's two modes: the pills' labels, and whether the second is on.
pub struct Modes<'a> {
    pub labels: [&'a str; 2],
    pub second: bool,
}

/// The mode pills at the right of the top line (unscaled): each pill's rectangle.
fn mode_pills() -> [Rectangle; 2] {
    let (w, h, y) = (20.0, 12.0, 3.5);
    let right = PARAM_SIZE.width - 4.0;
    [Rectangle::new(Point::new(right - 2.0 * w, y), Size::new(w, h)), Rectangle::new(Point::new(right - w, y), Size::new(w, h))]
}

/// One route into a param knob: its meta knob.
pub struct Meta<'a> {
    /// The source's name ("Heat", "LFO 2").
    pub name: &'a str,
    pub settings: ModSettings,
    /// The source's colour.
    pub colour: Color,
}

/// How the top line fits `n` meta knobs: radius, distance between centres, how many dials show
/// (with more, the place after the last says "+N").
fn meta_layout(n: usize) -> (f32, f32, usize) {
    match n {
        0..=3 => (5.5, 16.0, n),
        4..=5 => (4.2, 11.5, n),
        _ => (4.2, 11.5, 4),
    }
}
/// The outer ring, outside the ticks: the range arc and, on it, the live dot.
const RANGE_R: f32 = 21.0;
/// The knob's centre in the cell (unscaled).
const KNOB_CENTRE: Point = Point::new(PARAM_SIZE.width / 2.0, 38.0);

/// The slot whose range arc the knob shows: the last meta knob touched.
fn focus(state: &State, mods: usize) -> usize {
    state.focus.min(mods.saturating_sub(1))
}

/// The `k`-th of `n` meta knobs' centre in the cell (unscaled).
fn meta_centre(k: usize, n: usize) -> Point {
    let (r, step, _) = meta_layout(n);
    Point::new(5.0 + r + 1.0 + k as f32 * step, 9.5)
}

/// A patch's colours for a knob cell (`theme.colors` of the patch).
#[derive(Debug, Clone, Copy)]
pub struct Colours {
    pub accent: Color,
    pub ink: Color,
}

impl<'a, M: 'a> canvas::Program<M> for Param<'a>
where
    M: From<(usize, KnobEvent)>,
{
    type State = State;

    fn update(&self, state: &mut State, event: &Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Action<M>> {
        if let Some(e) = drop_input(self.drop, state, event, bounds, cursor) {
            return e.map(|e| Action::publish(M::from((self.index, e))).and_capture());
        }
        if let (Some(_), Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))) = (&self.modes, event)
            && let Some(p) = cursor.position_over(bounds)
        {
            let scale = bounds.width / PARAM_SIZE.width;
            let local = Point::new((p.x - bounds.x) / scale, (p.y - bounds.y) / scale);
            if let Some(k) = mode_pills().iter().position(|r| r.expand(1.5).contains(local)) {
                return Some(Action::publish(M::from((self.index, KnobEvent::Mode(k == 1)))).and_capture());
            }
        }
        if let Some(action) = meta_input(&PARAM_DIALS, self.index, &self.mods, self.value, state, event, bounds, cursor) {
            return Some(action);
        }
        let e = input(state, event, bounds, cursor, self.value, self.default)?;
        Some(Action::publish(M::from((self.index, e))).and_capture())
    }

    fn draw(&self, state: &State, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let c = theme::c();
        let mut f = Frame::new(renderer, bounds.size());
        // Drawn at PARAM_SIZE and scaled to the bounds (the surface shows bigger cells).
        f.scale(bounds.width / PARAM_SIZE.width);
        let w = PARAM_SIZE.width;
        let live = state.hover || state.drag.is_some();

        // Cell: white 3 % (dark mode), a touch more on hover / drag.
        let wash = if live { 0.05 } else { 0.03 };
        f.fill(&Path::rounded_rectangle(Point::ORIGIN, PARAM_SIZE, 8.0.into()), theme::alpha(c.frame_ink, wash));

        // The top line (18 px): a meta knob per route (mode pills come later).
        let active = state.meta_drag.as_ref().map(|(k, _)| *k).or(state.meta_hover);
        let focused = focus(state, self.mods.len());
        let n = self.mods.len();
        let (r, _, shown) = meta_layout(n);
        for (k, m) in self.mods.iter().take(shown).enumerate() {
            meta_dial(&mut f, meta_centre(k, n), r, m, k == focused, active == Some(k));
        }
        if shown < n {
            f.fill_text(Text {
                content: format!("+{}", n - shown),
                position: meta_centre(shown, n) - Vector::new(r, 0.0),
                color: c.frame_muted,
                size: Pixels(8.5),
                font: theme::font::MONO,
                align_y: alignment::Vertical::Center,
                ..Text::default()
            });
        }
        if let Some(m) = &self.modes {
            let accent = self.colours.map_or(c.frame_ink, |p| p.accent);
            for (k, r) in mode_pills().into_iter().enumerate() {
                let on = (k == 1) == m.second;
                f.fill(&Path::rounded_rectangle(r.position(), r.size(), 3.0.into()), if on { theme::alpha(accent, 0.22) } else { theme::alpha(c.frame_ink, 0.05) });
                f.fill_text(Text {
                    content: m.labels[k].to_string(),
                    position: r.center(),
                    color: if on { accent } else { c.frame_muted },
                    size: Pixels(8.0),
                    font: theme::font::SANS_MEDIUM,
                    align_x: Alignment::Center,
                    align_y: alignment::Vertical::Center,
                    ..Text::default()
                });
            }
        }
        drop_outline(&mut f, PARAM_SIZE, self.drop, state.hover);

        // Knob (40 px): 11 ticks (lit up to the value), flat disc, pointer.
        let centre = KNOB_CENTRE;
        let stroke = |color: Color, width: f32| Stroke::default().with_color(color).with_width(width).with_line_cap(canvas::LineCap::Round);
        for i in 0..11 {
            let t = i as f32 / 10.0;
            let a = START + SWEEP * t;
            let d = Vector::new(a.cos(), a.sin());
            let lit = t <= self.value + 1e-4;
            let ink = match (lit, &self.colours) {
                (true, Some(p)) => theme::alpha(p.accent, 0.9),
                _ => theme::alpha(c.frame_ink, if lit { 0.45 } else { 0.16 }),
            };
            f.stroke(&Path::line(centre + d * 14.5, centre + d * 17.0), stroke(ink, 1.2));
        }
        let disc = Path::circle(centre, 10.0);
        f.fill(&disc, theme::mix(c.frame, c.frame_ink, 0.08));
        f.stroke(&disc, Stroke::default().with_color(theme::alpha(c.frame_ink, 0.08)).with_width(1.0));
        let angle = START + SWEEP * self.value;
        let d = Vector::new(angle.cos(), angle.sin());
        f.stroke(&Path::line(centre + d * 3.5, centre + d * 9.0), stroke(self.colours.map_or(c.frame_ink, |p| p.accent), 1.6));

        if let Some(m) = self.mods.get(focus(state, self.mods.len())) {
            // Range arc (the focused slot): base → base + amount, or ±amount/2 when bipolar (the
            // same size); the handle at its end drags the amount.
            let s = m.settings;
            let angle = |v: f32| START + SWEEP * v.clamp(0.0, 1.0);
            let ink = theme::alpha(m.colour, if s.bypass { 0.3 } else { 0.85 });
            let range = |f: &mut Frame, to: f32| {
                let (a, b) = (angle(self.value), angle(to));
                if (b - a).abs() > 1e-3 {
                    let arc = Path::new(|p| p.arc(Arc { center: centre, radius: RANGE_R, start_angle: a.min(b).into(), end_angle: a.max(b).into() }));
                    f.stroke(&arc, stroke(ink, 1.8));
                }
            };
            let reach = reach(s);
            range(&mut f, self.value + reach);
            if s.bipolar {
                range(&mut f, self.value - reach);
            }
            let end = angle(self.value + reach);
            f.fill(&Path::circle(centre + Vector::new(end.cos(), end.sin()) * RANGE_R, 2.3), ink);

            // Live value (all slots summed, clamped): a soft white dot riding the outer ring.
            let live = angle(self.effective);
            let at = centre + Vector::new(live.cos(), live.sin()) * RANGE_R;
            f.fill(&Path::circle(at, 3.4), theme::alpha(m.colour, 0.16));
            f.fill(&Path::circle(at, 2.4), Color { a: 0.35, ..Color::WHITE });
            f.fill(&Path::circle(at, 1.4), Color::WHITE);
        }

        // Label, or the value while dragging or hovering; a meta knob's macro and amount.
        let (content, color, font) = if let Some(m) = active.and_then(|k| self.mods.get(k)) {
            (meta_text(m), m.colour, theme::font::MONO)
        } else if state.drag.is_some() || state.hover {
            (self.value_text.clone(), self.colours.map_or(c.frame_ink, |p| p.ink), theme::font::MONO)
        } else {
            (self.label.to_string(), self.colours.map_or(c.frame_ink_2, |p| theme::alpha(p.ink, 0.8)), theme::font::SANS)
        };
        f.fill_text(Text {
            content: fit(&content, w - 8.0, 10.0),
            position: Point::new(w / 2.0, 72.0),
            color,
            size: Pixels(10.0),
            font,
            align_x: Alignment::Center,
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });

        vec![f.into_geometry()]
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.drag.is_some() || state.meta_drag.is_some() {
            // The shell hides the cursor while anything drags (`hide_cursor`); this is for the cell.
            mouse::Interaction::Hidden
        } else if cursor.position_over(bounds).and_then(|p| meta_at(&PARAM_DIALS, &self.mods, self.value, state, bounds, p)).is_some() {
            mouse::Interaction::ResizingVertically
        } else {
            mouse::Interaction::None
        }
    }
}

/// Where a kind of cell puts its meta knobs (unscaled; the cell scales by its width).
#[derive(Clone, Copy)]
struct Dials {
    size: Size,
    /// The `k`-th of `n` dials: centre and radius.
    place: fn(usize, usize) -> (Point, f32),
    /// How many of `n` show (with more, the place after the last says "+N").
    shown: fn(usize) -> usize,
    /// The focused range arc's handle: the knob's centre and the arc's radius.
    handle: Option<(Point, f32)>,
}

fn param_place(k: usize, n: usize) -> (Point, f32) {
    (meta_centre(k, n), meta_layout(n).0)
}

fn param_shown(n: usize) -> usize {
    meta_layout(n).2
}

const PARAM_DIALS: Dials = Dials { size: PARAM_SIZE, place: param_place, shown: param_shown, handle: Some((KNOB_CENTRE, RANGE_R)) };

/// The meta knob (or the focused range arc's handle) at window point `p`.
fn meta_at(d: &Dials, mods: &[Meta<'_>], value: f32, state: &State, bounds: Rectangle, p: Point) -> Option<usize> {
    let scale = bounds.width / d.size.width;
    let local = Point::new((p.x - bounds.x) / scale, (p.y - bounds.y) / scale);
    let n = mods.len();
    let dial = (0..(d.shown)(n)).find(|&k| {
        let (c, r) = (d.place)(k, n);
        local.distance(c) <= r + 2.5
    });
    dial.or_else(|| {
        let (centre, radius) = d.handle?;
        let k = focus(state, n);
        let m = mods.get(k)?;
        let end = START + SWEEP * (value + reach(m.settings)).clamp(0.0, 1.0);
        (local.distance(centre + Vector::new(end.cos(), end.sin()) * radius) <= 4.5).then_some(k)
    })
}

/// Input on the meta knobs and the range arc's handle; `None` = not theirs (the knob's own
/// input runs). Pressing a meta knob focuses its slot (its range arc shows).
fn meta_input<M: From<(usize, KnobEvent)>>(
    d: &Dials,
    index: usize,
    mods: &[Meta<'_>],
    value: f32,
    state: &mut State,
    event: &Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
) -> Option<Action<M>> {
    let scale = bounds.width / d.size.width;
    let local = |p: Point| Point::new((p.x - bounds.x) / scale, (p.y - bounds.y) / scale);
    let n = mods.len();
    let dial_at = |p: Point| {
        (0..(d.shown)(n)).find(|&k| {
            let (c, r) = (d.place)(k, n);
            local(p).distance(c) <= r + 2.5
        })
    };
    let publish = |k: usize, e: MetaEvent| Some(Action::publish(M::from((index, KnobEvent::Meta(k, e)))).and_capture());
    // Dragging up turns the amount clockwise (negative = the other way).
    match event {
        Event::Mouse(mouse::Event::CursorMoved { position }) => {
            state.meta_hover = cursor.position_over(bounds).and_then(dial_at);
            let (k, drag) = state.meta_drag.as_mut()?;
            let fine = if state.fine { FINE } else { 1.0 };
            // 200 px = the whole −100…+100 % span.
            let v = drag.moved(position.y, fine * 2.0 / DRAG_RANGE, -1.0..=1.0);
            publish(*k, MetaEvent::Amount(v))
        }
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            let p = cursor.position_over(bounds)?;
            let on_dial = dial_at(p);
            let k = meta_at(d, mods, value, state, bounds, p)?;
            state.focus = k;
            let double = state.last_press.is_some_and(|t| t.elapsed() < Duration::from_millis(350));
            state.last_press = Some(Instant::now());
            if double && on_dial.is_some() {
                state.meta_drag = None;
                return publish(k, MetaEvent::Reset);
            }
            state.meta_drag = Some((k, Drag::start(mods[k].settings.amount, p.y)));
            publish(k, MetaEvent::Begin)
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            let (k, _) = state.meta_drag.take()?;
            publish(k, MetaEvent::End)
        }
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
            let p = cursor.position_over(bounds)?;
            let k = dial_at(p)?;
            state.focus = k;
            publish(k, MetaEvent::Menu(p))
        }
        Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
            let k = dial_at(cursor.position_over(bounds)?)?;
            let notches = match *delta {
                mouse::ScrollDelta::Lines { x, y } => if y != 0.0 { y } else { x },
                mouse::ScrollDelta::Pixels { x, y } => (if y != 0.0 { y } else { x }) / 40.0,
            };
            let step = notches * SCROLL_STEP * if state.fine { FINE } else { 1.0 };
            publish(k, MetaEvent::Amount((mods[k].settings.amount + step).clamp(-1.0, 1.0)))
        }
        _ => None,
    }
}

/// A meta knob of radius `r`: a faint ring and the amount as an arc in the source's colour from
/// the top (−100 % = all the way left, +100 % all the way right), a dot in the middle. Faded
/// while bypassed.
fn meta_dial(f: &mut Frame, centre: Point, r: f32, m: &Meta<'_>, focused: bool, active: bool) {
    let (s, colour) = (m.settings, m.colour);
    let a = if s.bypass { 0.35 } else { 1.0 };
    let ring = Path::circle(centre, r);
    let ring_alpha = if active { 0.5 } else if focused { 0.35 } else { 0.18 };
    f.stroke(&ring, Stroke::default().with_color(theme::alpha(colour, ring_alpha * a)).with_width(1.2));
    let top = START + SWEEP * 0.5;
    let end = top + SWEEP * 0.5 * s.amount.clamp(-1.0, 1.0);
    if (end - top).abs() > 0.01 {
        let arc = Path::new(|p| p.arc(Arc { center: centre, radius: r, start_angle: top.min(end).into(), end_angle: top.max(end).into() }));
        f.stroke(&arc, Stroke::default().with_color(theme::alpha(colour, a)).with_width(1.8).with_line_cap(canvas::LineCap::Round));
    }
    f.fill(&Path::circle(centre, 1.3), theme::alpha(colour, if focused { a } else { 0.6 * a }));
}

/// How far the range arc reaches from the base: the amount, or half of it each way when bipolar.
fn reach(s: ModSettings) -> f32 {
    if s.bipolar { s.amount / 2.0 } else { s.amount }
}

/// "Heat +45%", "Heat −30% bi", "Heat +45% off".
fn meta_text(m: &Meta<'_>) -> String {
    let s = m.settings;
    let bi = if s.bipolar { " bi" } else { "" };
    let off = if s.bypass { " off" } else { "" };
    format!("{} {:+.0}%{bi}{off}", m.name, s.amount * 100.0).replace('-', "−")
}

/// The param cell as an element.
pub fn param_cell<'a, M: From<(usize, KnobEvent)> + 'a>(p: Param<'a>) -> Element<'a, M> {
    param_cell_scaled(p, 1.0)
}

/// The param cell, `scale` times its size (everything in it grows alike).
pub fn param_cell_scaled<'a, M: From<(usize, KnobEvent)> + 'a>(p: Param<'a>, scale: f32) -> Element<'a, M> {
    canvas::Canvas::new(p).width(PARAM_SIZE.width * scale).height(PARAM_SIZE.height * scale).into()
}
