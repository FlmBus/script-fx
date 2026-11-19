//! The stage: the patch window. Shows the patch's ControlPanel (component layers, rendered on
//! the CPU by `ui_components`, one image per layer) scaled into the window, and turns pointer
//! input into param changes (knob drag, button click, double-click reset). The window's
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
    widget::{canvas, center, column, container, image, pin, stack, text},
};
use patch::{Control, Patch, Source, Values};
use ui_components::{Bound, Hit, Inputs};

use super::theme;

/// A ControlPanel and its interaction state.
pub struct Panel {
    ui: ui_components::Ui,
    /// One image per layer, in draw order (logical rect inside the 840×560 panel).
    layers: Vec<(u64, ui_components::Rect, image::Handle)>,
    hover: Option<Hit>,
    active: Option<u64>,
    drag: Option<Drag>,
    last_press: Option<(u64, Instant)>,
    fine: bool,
}

#[derive(Clone, Copy)]
enum Target {
    Macro(usize),
    Param(usize),
}

struct Drag {
    target: Target,
    start: f32,
    start_y: f32,
}

/// What the panel wants changed; the shell applies it (host params need the GUI context).
pub enum Change {
    BeginMacro(usize),
    SetMacro(usize, f32),
    EndMacro(usize),
    /// Free param, plain value.
    SetFree(usize, f32),
}

/// Pointer input over the panel, in panel coordinates (logical, 840×560).
#[derive(Debug, Clone)]
pub enum Pointer {
    Moved(Point),
    Pressed(Point),
    Released,
    Left,
    Fine(bool),
}

/// Pixels of drag (panel units) for the full 0..1 range.
const DRAG_RANGE: f32 = 200.0;

impl Panel {
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

    /// Re-renders changed components at `scale` (device pixels per panel unit) and refreshes
    /// the layer images.
    pub fn update(&mut self, patch: &Patch, values: &Values, scale: f32) {
        let bound = |b: &str| -> Option<Bound> {
            match patch.resolve_source(b).ok()? {
                Source::Macro(slot) => {
                    let v = values.macros[slot];
                    Some(Bound { value: v, display: patch.macro_text(slot, v), locked: false })
                }
                Source::Param(i) => {
                    let v = values.params.get(i).copied().unwrap_or(0.0);
                    let locked = !matches!(patch.controls[i], Control::Free);
                    Some(Bound { value: v, display: patch.params[i].display(v), locked })
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
                if let Some(d) = &mut self.drag {
                    let fine = if self.fine { 0.2 } else { 1.0 };
                    let v = (d.start - (p.y - d.start_y) * fine / DRAG_RANGE).clamp(0.0, 1.0);
                    out.push(match d.target {
                        Target::Macro(slot) => Change::SetMacro(slot, v),
                        Target::Param(i) => Change::SetFree(i, patch.params[i].plain(v)),
                    });
                    if self.fine {
                        // Fine mode is relative: rebase so toggling shift doesn't jump.
                        d.start = v;
                        d.start_y = p.y;
                    }
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
                            Target::Param(i) => values.params.get(i).copied().unwrap_or(0.0),
                        };
                        self.drag = Some(Drag { target: t, start, start_y: p.y });
                        self.active = Some(hit.id);
                    }
                } else if hit.kind == "button" {
                    let _ = self.ui.click(hit.id);
                }
                self.hover = Some(hit);
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

    fn cursor(&self) -> mouse::Interaction {
        if self.drag.is_some() {
            return mouse::Interaction::ResizingVertically;
        }
        match self.hover.as_ref().map(|h| h.kind.as_str()) {
            Some("knob") => mouse::Interaction::ResizingVertically,
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        }
    }

    pub fn stats(&self) -> ui_components::FrameStats {
        self.ui.stats
    }
}

/// Resolves what a control drags: a macro-driven param drags its macro instead.
fn target(patch: &Patch, src: &Source) -> Option<Target> {
    match *src {
        Source::Macro(slot) => Some(Target::Macro(slot)),
        Source::Param(i) => match patch.controls[i] {
            Control::Free => Some(Target::Param(i)),
            Control::Macro { slot, .. } => Some(Target::Macro(slot)),
        },
        _ => None,
    }
}

/// tiny-skia pixmaps are premultiplied; iced images are straight alpha.
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

/// Size of the stage (the patch window) and where the 840×560 panel sits in it.
pub fn panel_fit() -> (f32, Vector) {
    let (w, h) = theme::stage_size();
    let s = (w / ui_components::WIDTH).min(h / ui_components::HEIGHT);
    (s, Vector::new((w - ui_components::WIDTH * s) / 2.0, (h - ui_components::HEIGHT * s) / 2.0))
}

/// The patch window. `bg`: the patch's background colour (fills around the panel).
pub fn view<'a>(patch: &'a Patch, panel: Option<&'a Panel>, bg: Color) -> Element<'a, Pointer> {
    let (s, offset) = panel_fit();
    let ground = container(column![]).width(Fill).height(Fill).style(move |_: &Theme| container::Style {
        background: Some(bg.into()),
        ..Default::default()
    });

    let content: Element<'a, Pointer> = match panel {
        Some(panel) => {
            let mut layers = stack![].width(Fill).height(Fill);
            for (_, r, handle) in &panel.layers {
                layers = layers.push(
                    pin(image(handle.clone()).width(r.w * s).height(r.h * s).content_fit(ContentFit::Fill))
                        .x(offset.x + r.x * s)
                        .y(offset.y + r.y * s),
                );
            }
            layers = layers.push(PointerArea { scale: s, offset, cursor: panel.cursor() });
            layers.into()
        }
        None => center(
            text(format!("{} has no ControlPanel. Its macros are under Settings.", patch.name))
                .size(12.5)
                .color(theme::FRAME_MUTED),
        )
        .into(),
    };

    stack![ground, content, corners()].width(Fill).height(Fill).into()
}

// ---- rounded window corners ---------------------------------------------------------------------

fn corners<'a, M: 'a>() -> Element<'a, M> {
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
            frame.fill(&path, theme::FRAME);
        }
        vec![frame.into_geometry()]
    }
}

// ---- pointer input over the panel ---------------------------------------------------------------

/// Transparent, fills the stage, reports pointer input in panel coordinates.
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
        let to_panel = |p: Point| {
            Point::new((p.x - bounds.x - self.offset.x) / self.scale, (p.y - bounds.y - self.offset.y) / self.scale)
        };
        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let inside = bounds.contains(*position);
                if inside || state.pressed {
                    shell.publish(Pointer::Moved(to_panel(*position)));
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
                    shell.publish(Pointer::Pressed(to_panel(p)));
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
