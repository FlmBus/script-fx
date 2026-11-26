//! The stage: the patch window. Shows the patch's own surface (component layers, rendered on
//! the CPU by `ui_components`, one image per layer) scaled into the window, and turns pointer
//! input into param changes (knob drag and scroll, Shift = fine, button click, double-click
//! reset, right-click = type a value). The window's
//! rounded corners are frame-coloured masks on top, so patch art never needs to know about them.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use nice_plug_iced::iced::{
    Color, ContentFit, Element, Event, Fill, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, renderer,
        widget::{Tree, tree},
    },
    keyboard,
    widget::{canvas, column, container, image, pin, stack},
};
use patch::{Patch, Source, Values};
use ui_components::{Bound, Hit, Inputs};

use super::{pointer_lock, theme};

/// A patch's own surface and its interaction state.
pub struct Surface {
    ui: ui_components::Ui,
    /// One image per layer, in draw order (logical rect inside the 840×560 surface).
    layers: Vec<(u64, ui_components::Rect, image::Handle)>,
    hover: Option<Hit>,
    active: Option<u64>,
    drag: Option<Drag>,
    last_press: Option<(u64, Instant)>,
    fine: bool,
}

/// What a control changes.
#[derive(Clone, Copy, Debug)]
pub enum Target {
    Macro(usize),
    Param(usize),
}

struct Drag {
    target: Target,
    /// The value and the locked pointer.
    drag: pointer_lock::Drag,
}

/// What the surface wants changed; the shell applies it (host params need the GUI context).
pub enum Change {
    BeginMacro(usize),
    SetMacro(usize, f32),
    EndMacro(usize),
    /// Free param, plain value.
    SetFree(usize, f32),
    /// Right-click on a control: ask for a typed value (`at`: surface coordinates).
    OpenEntry { target: Target, at: Point },
}

/// Pointer input over the surface, in surface coordinates (logical, 840×560).
#[derive(Debug, Clone)]
pub enum Pointer {
    Moved(Point),
    Pressed(Point),
    RightPressed(Point),
    /// Wheel over the surface: notches (up = positive).
    Scrolled(Point, f32),
    Released,
    Left,
    Fine(bool),
}

/// Pixels of drag (surface units) for the full 0..1 range.
const DRAG_RANGE: f32 = 200.0;
/// Value change per wheel notch (50 notches for the full range).
const SCROLL_STEP: f32 = 0.02;
/// Shift: drag and scroll this much finer.
const FINE: f32 = 0.2;

impl Surface {
    pub fn load(dir: &std::path::Path) -> Result<Self, String> {
        Ok(Self {
            ui: ui_components::Ui::load(dir)?,
            layers: Vec::new(),
            hover: None,
            active: None,
            drag: None,
            last_press: None,
            fine: false,
        })
    }

    /// Hot reload: re-evaluates the UI files, keeps local component state.
    pub fn reload(&mut self) -> Result<(), String> {
        self.ui.reload()?;
        self.layers.clear();
        Ok(())
    }

    /// Re-renders changed components at `scale` (device pixels per surface unit) and refreshes
    /// the layer images.
    pub fn update(&mut self, patch: &Patch, values: &Values, scale: f32) {
        let bound = |b: &str| -> Option<Bound> {
            match patch.resolve_source(b).ok()? {
                Source::Macro(slot) => {
                    let v = values.macros[slot];
                    Some(Bound { value: v, display: patch.macro_text(slot, v), locked: false })
                }
                Source::Param(i) => {
                    // A modulated param stays editable: the knob drags its base value. The text
                    // says what's heard (after modulation): surfaces can't show the range arc or
                    // the live dot, the shell never draws on patch art.
                    let v = values.base(i);
                    let heard = values.params.get(i).copied().unwrap_or(v);
                    Some(Bound { value: v, display: patch.params[i].display(heard), locked: false })
                }
                _ => None,
            }
        };
        let meter = |m: &str| -> Option<f32> {
            match patch.resolve_source(m).ok()? {
                Source::MeterIn => Some(values.meter_in),
                Source::MeterOut => Some(values.meter_out),
                Source::Block(i) => values.blocks.get(i).copied(),
                _ => None,
            }
        };
        let series = |_: &str| None;
        let inputs = Inputs {
            time: values.time,
            hover: self.hover.as_ref().map(|h| h.id),
            active: self.active,
            bound: &bound,
            meter: &meter,
            series: &series,
        };
        self.ui.update(&inputs, scale);

        let mut old: HashMap<u64, image::Handle> = self.layers.drain(..).map(|(id, _, h)| (id, h)).collect();
        for layer in self.ui.layers() {
            let Some(pm) = layer.pixmap else { continue };
            let handle = match old.remove(&layer.id) {
                Some(h) if !layer.changed => h,
                _ => image::Handle::from_rgba(pm.width(), pm.height(), unpremultiply(pm.data())),
            };
            self.layers.push((layer.id, layer.rect, handle));
        }
    }

    /// Applies pointer input; returns param changes for the shell.
    pub fn pointer(&mut self, ev: Pointer, patch: &Patch, values: &Values) -> Vec<Change> {
        let mut out = Vec::new();
        match ev {
            Pointer::Fine(on) => self.fine = on,
            Pointer::Left => {
                if self.drag.is_none() {
                    self.hover = None;
                }
            }
            Pointer::Moved(p) => {
                // The button went up without the release reaching us (see `pointer_lock`): end
                // the drag, and the host gesture a macro drag opened.
                if self.drag.as_ref().is_some_and(|d| d.drag.ended()) {
                    if let Some(Drag { target: Target::Macro(slot), .. }) = self.drag.take() {
                        out.push(Change::EndMacro(slot));
                    }
                    self.drag = None;
                    self.active = None;
                    return out;
                }
                if let Some(d) = &mut self.drag {
                    let fine = if self.fine { FINE } else { 1.0 };
                    // Relative to the last event: the pointer stays locked where it grabbed.
                    let v = d.drag.moved(p.y, fine / DRAG_RANGE, 0.0..=1.0);
                    out.push(match d.target {
                        Target::Macro(slot) => Change::SetMacro(slot, v),
                        Target::Param(i) => Change::SetFree(i, patch.params[i].plain(v)),
                    });
                } else {
                    self.hover = self.ui.hit(p.x, p.y);
                }
            }
            Pointer::Pressed(p) => {
                let Some(hit) = self.ui.hit(p.x, p.y) else { return out };
                let target = hit.bind.as_deref().and_then(|b| target(patch, &patch.resolve_source(b).ok()?));
                let double = self.last_press.is_some_and(|(id, t)| id == hit.id && t.elapsed() < Duration::from_millis(350));
                self.last_press = Some((hit.id, Instant::now()));

                if double {
                    // Double-click: back to the default.
                    match target {
                        Some(Target::Macro(slot)) => {
                            out.push(Change::BeginMacro(slot));
                            out.push(Change::SetMacro(slot, patch.macro_default(slot)));
                            out.push(Change::EndMacro(slot));
                        }
                        Some(Target::Param(i)) => out.push(Change::SetFree(i, patch.params[i].default_plain())),
                        None => {}
                    }
                } else if hit.kind == "knob" {
                    if let Some(t) = target {
                        let start = match t {
                            Target::Macro(slot) => {
                                out.push(Change::BeginMacro(slot));
                                values.macros[slot]
                            }
                            Target::Param(i) => values.base(i),
                        };
                        self.drag = Some(Drag { target: t, drag: pointer_lock::Drag::start(start, p.y) });
                        self.active = Some(hit.id);
                    }
                } else if hit.kind == "button" {
                    let _ = self.ui.click(hit.id);
                }
                self.hover = Some(hit);
            }
            Pointer::Scrolled(p, notches) => {
                let Some(hit) = self.ui.hit(p.x, p.y) else { return out };
                let Some(t) = hit.bind.as_deref().and_then(|b| target(patch, &patch.resolve_source(b).ok()?)) else { return out };
                if hit.kind != "knob" || self.drag.is_some() {
                    return out;
                }
                let step = notches * SCROLL_STEP * if self.fine { FINE } else { 1.0 };
                match t {
                    Target::Macro(slot) => {
                        let v = (values.macros[slot] + step).clamp(0.0, 1.0);
                        out.extend([Change::BeginMacro(slot), Change::SetMacro(slot, v), Change::EndMacro(slot)]);
                    }
                    Target::Param(i) => {
                        let v = (values.base(i) + step).clamp(0.0, 1.0);
                        out.push(Change::SetFree(i, patch.params[i].plain(v)));
                    }
                }
                self.hover = Some(hit);
            }
            Pointer::RightPressed(p) => {
                let Some(hit) = self.ui.hit(p.x, p.y) else { return out };
                if let Some(t) = hit.bind.as_deref().and_then(|b| target(patch, &patch.resolve_source(b).ok()?)) {
                    out.push(Change::OpenEntry { target: t, at: p });
                }
            }
            Pointer::Released => {
                if let Some(Drag { target: Target::Macro(slot), .. }) = self.drag.take() {
                    out.push(Change::EndMacro(slot));
                }
                self.drag = None;
                self.active = None;
            }
        }
        out
    }

    /// No hand over knobs and faders, hidden while dragging one; the hand stays for buttons.
    fn cursor(&self) -> mouse::Interaction {
        if self.drag.is_some() {
            return mouse::Interaction::Hidden;
        }
        match self.hover.as_ref().map(|h| h.kind.as_str()) {
            Some("knob" | "fader" | "slider") | None => mouse::Interaction::None,
            Some(_) => mouse::Interaction::Pointer,
        }
    }

    /// Is a control being dragged (the shell hides the cursor meanwhile)?
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn stats(&self) -> ui_components::FrameStats {
        self.ui.stats
    }
}

/// Resolves what a control drags. Params always drag their own base value, also when macros
/// modulate them (the macros add offsets around it).
fn target(_patch: &Patch, src: &Source) -> Option<Target> {
    match *src {
        Source::Macro(slot) => Some(Target::Macro(slot)),
        Source::Param(i) => Some(Target::Param(i)),
        _ => None,
    }
}

/// tiny-skia pixmaps are premultiplied; iced images are straight alpha.
///
/// One copy per changed layer per frame — a full-size layer at 2× is ~7.5 MB, which is why a
/// layer that re-renders every frame is the most expensive thing the editor does. Unchanged
/// layers keep their handle (and their place in the texture atlas) and don't come through here.
pub(super) fn unpremultiply(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    for px in out.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    out
}

/// Size of the stage (the patch window) and where the 840×560 surface sits in it.
pub fn surface_fit() -> (f32, Vector) {
    let (w, h) = theme::stage_size();
    let s = (w / ui_components::WIDTH).min(h / ui_components::HEIGHT);
    (s, Vector::new((w - ui_components::WIDTH * s) / 2.0, (h - ui_components::HEIGHT * s) / 2.0))
}

/// The patch window. `bg`: the patch's background colour (fills around the surface).
pub fn view<'a>(surface: &'a Surface, bg: Color) -> Element<'a, Pointer> {
    let (s, offset) = surface_fit();
    let ground = container(column![]).width(Fill).height(Fill).style(move |_: &Theme| container::Style {
        background: Some(bg.into()),
        ..Default::default()
    });

    let mut layers = stack![].width(Fill).height(Fill);
    // `SCRIPTFX_DIAG_NO_PATCH_LAYERS`: the patch's own art is the biggest thing we push through
    // the texture atlas, and the only part of it that changes every frame (`diag.rs`).
    let draw_layers = !crate::diag::switches().no_patch_layers;
    for (_, r, handle) in surface.layers.iter().filter(|_| draw_layers) {
        layers = layers.push(
            pin(image(handle.clone()).width(r.w * s).height(r.h * s).content_fit(ContentFit::Fill))
                .x(offset.x + r.x * s)
                .y(offset.y + r.y * s),
        );
    }
    layers = layers.push(PointerArea { scale: s, offset, cursor: surface.cursor() });

    stack![ground, layers, corners()].width(Fill).height(Fill).into()
}

// ---- rounded window corners ---------------------------------------------------------------------

/// The window's rounded corners: frame-coloured masks over content that doesn't round itself.
pub(super) fn corners<'a, M: 'a>() -> Element<'a, M> {
    canvas(Corners).width(Fill).height(Fill).into()
}

struct Corners;

impl<M> canvas::Program<M> for Corners {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let r = theme::RADIUS_WINDOW;
        let (w, h) = (bounds.width, bounds.height);
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // Each corner: the corner point, out along one edge, the window's rounded edge back to
        // the other edge, closed. Filled in frame colour.
        let corners = [
            (Point::new(0.0, 0.0), Point::new(0.0, r), Point::new(r, 0.0)),
            (Point::new(w, 0.0), Point::new(w - r, 0.0), Point::new(w, r)),
            (Point::new(w, h), Point::new(w, h - r), Point::new(w - r, h)),
            (Point::new(0.0, h), Point::new(r, h), Point::new(0.0, h - r)),
        ];
        for (c, a, b) in corners {
            let path = canvas::Path::new(|p| {
                p.move_to(c);
                p.line_to(a);
                p.arc_to(c, b, r);
                p.close();
            });
            frame.fill(&path, theme::c().frame);
        }
        vec![frame.into_geometry()]
    }
}

// ---- pointer input over the surface -------------------------------------------------------------

/// Transparent, fills the stage, reports pointer input in surface coordinates.
struct PointerArea {
    scale: f32,
    offset: Vector,
    cursor: mouse::Interaction,
}

#[derive(Default)]
struct PointerState {
    pressed: bool,
    inside: bool,
}

impl Widget<Pointer, Theme, Renderer> for PointerArea {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<PointerState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(PointerState::default())
    }

    fn size(&self) -> Size<Length> {
        Size { width: Length::Fill, height: Length::Fill }
    }

    fn layout(&mut self, _: &mut Tree, _: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::Node::new(limits.width(Length::Fill).height(Length::Fill).max())
    }

    fn draw(&self, _: &Tree, _: &mut Renderer, _: &Theme, _: &renderer::Style, _: Layout<'_>, _: mouse::Cursor, _: &Rectangle) {}

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, Pointer>,
        _: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<PointerState>();
        let bounds = layout.bounds();
        let to_surface = |p: Point| {
            Point::new((p.x - bounds.x - self.offset.x) / self.scale, (p.y - bounds.y - self.offset.y) / self.scale)
        };
        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                // `position_over` is None while a layer above (a sheet) covers the point.
                let inside = cursor.position_over(bounds).is_some();
                if inside || state.pressed {
                    shell.publish(Pointer::Moved(to_surface(*position)));
                } else if state.inside {
                    shell.publish(Pointer::Left);
                }
                state.inside = inside;
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                state.inside = false;
                shell.publish(Pointer::Left);
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(p) = cursor.position_over(bounds) {
                    state.pressed = true;
                    shell.publish(Pointer::Pressed(to_surface(p)));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                if let Some(p) = cursor.position_over(bounds) {
                    shell.publish(Pointer::RightPressed(to_surface(p)));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if let Some(p) = cursor.position_over(bounds) {
                    // Some systems turn Shift+wheel into horizontal scrolling.
                    let notches = match *delta {
                        mouse::ScrollDelta::Lines { x, y } => if y != 0.0 { y } else { x },
                        mouse::ScrollDelta::Pixels { x, y } => (if y != 0.0 { y } else { x }) / 40.0,
                    };
                    shell.publish(Pointer::Scrolled(to_surface(p), notches));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.pressed => {
                state.pressed = false;
                shell.publish(Pointer::Released);
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => shell.publish(Pointer::Fine(m.shift())),
            _ => {}
        }
    }

    fn mouse_interaction(&self, _: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, _: &Rectangle, _: &Renderer) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) { self.cursor } else { mouse::Interaction::default() }
    }
}

impl<'a> From<PointerArea> for Element<'a, Pointer> {
    fn from(p: PointerArea) -> Self {
        Element::new(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Neon Saturator's Tape knob (macro slot 0) sits around (165, 388) in the surface.
    fn neon() -> (Patch, Surface, Values) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../patches/neon-saturator");
        let patch = Patch::load(&dir).unwrap();
        let mut surface = Surface::load(&dir).unwrap();
        let mut values = Values { params: vec![0.0; patch.params.len()], ..Default::default() };
        values.macros[0] = 0.5;
        surface.update(&patch, &values, 1.0);
        (patch, surface, values)
    }

    fn set_macro(changes: &[Change]) -> Option<f32> {
        changes.iter().find_map(|c| if let Change::SetMacro(0, v) = c { Some(*v) } else { None })
    }

    #[test]
    fn wheel_turns_a_knob_shift_finer() {
        let (patch, mut surface, values) = neon();
        let at = Point::new(165.0, 388.0);
        let coarse = set_macro(&surface.pointer(Pointer::Scrolled(at, 1.0), &patch, &values)).expect("scroll changes Tape");
        assert!((coarse - (0.5 + SCROLL_STEP)).abs() < 1e-6);
        surface.pointer(Pointer::Fine(true), &patch, &values);
        let fine = set_macro(&surface.pointer(Pointer::Scrolled(at, -1.0), &patch, &values)).unwrap();
        assert!((fine - (0.5 - SCROLL_STEP * FINE)).abs() < 1e-6);
        // Wheel over empty surface space changes nothing.
        assert!(surface.pointer(Pointer::Scrolled(Point::new(5.0, 5.0), 1.0), &patch, &values).is_empty());
    }

    #[test]
    fn right_click_asks_for_a_value() {
        let (patch, mut surface, values) = neon();
        let out = surface.pointer(Pointer::RightPressed(Point::new(165.0, 388.0)), &patch, &values);
        assert!(matches!(out.as_slice(), [Change::OpenEntry { target: Target::Macro(0), .. }]));
    }
}

#[cfg(test)]
mod advanced_tab {
    use super::*;

    /// Neon's ADVANCED tab shows a spectrum `series` that has no data yet: the component must get
    /// an empty list, not the series' name (it failed in `ipairs`).
    #[test]
    fn neon_advanced_tab_renders() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../patches/neon-saturator");
        let patch = Patch::load(&dir).unwrap();
        let mut surface = Surface::load(&dir).unwrap();
        let values = Values { params: vec![0.0; patch.params.len()], ..Default::default() };
        surface.update(&patch, &values, 1.0);
        let at = Point::new(470.0, 304.0);
        surface.pointer(Pointer::Pressed(at), &patch, &values);
        surface.pointer(Pointer::Released, &patch, &values);
        surface.update(&patch, &values, 1.0);
        assert_eq!(surface.ui.errors(), Vec::<String>::new());
    }
}
