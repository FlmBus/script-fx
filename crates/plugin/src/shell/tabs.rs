//! A tab bar whose highlight slides: the pill moves from the old tab to the new one (180 ms,
//! ease-out) instead of jumping. Used by every tab bar (notch view switch, drawer tabs, settings
//! theme switch).
//!
//! The bar is given twice, with the same layout: `off` (every tab unselected, interactive) and
//! `on` (every tab selected, drawn only inside the pill). So a tab's label changes colour exactly
//! where the pill covers it, also mid-slide (dark ink on the notch's white pill).
//! `selected: None` = no pill (the drawer's tabs while it's closed).
//!
//! The pill is the selected tab's face: wrap the tabs' button style in [`style`], so the `on` copies
//! draw only their label (their own face, clipped to the moving pill, would show seams).

use std::time::{Duration, Instant};

use nice_plug_iced::iced::{
    Border, Color, Element, Event, Length, Rectangle, Shadow, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget,
        layout, mouse, renderer,
        widget::{Operation, Tree, tree},
    },
    widget::button,
    window,
};

const DURATION: Duration = Duration::from_millis(180);

/// A tab's button style for a sliding tab bar: unchanged when off; when on, without its own face
/// (background, shadow), since the pill draws that.
pub fn style<T>(on: bool, style: impl Fn(&T, button::Status) -> button::Style) -> impl Fn(&T, button::Status) -> button::Style {
    move |theme, status| {
        let s = style(theme, status);
        if on { button::Style { background: None, shadow: Shadow::default(), ..s } } else { s }
    }
}

/// The sliding highlight.
#[derive(Debug, Clone, Copy)]
pub struct Pill {
    pub background: Color,
    pub radius: f32,
    pub shadow: Shadow,
}

pub struct Tabs<'a, M, Theme, Renderer> {
    off: Element<'a, M, Theme, Renderer>,
    on: Element<'a, M, Theme, Renderer>,
    selected: Option<usize>,
    pill: Pill,
}

/// `off` and `on`: the same row of tabs, unselected and selected (see the module docs).
pub fn tabs<'a, M, Theme, Renderer>(
    off: impl Into<Element<'a, M, Theme, Renderer>>,
    on: impl Into<Element<'a, M, Theme, Renderer>>,
    selected: Option<usize>,
    pill: Pill,
) -> Tabs<'a, M, Theme, Renderer> {
    Tabs { off: off.into(), on: on.into(), selected, pill }
}

/// Where the pill is: rectangles relative to the bar, so it moves along with the bar.
#[derive(Default)]
struct State {
    shown: Option<usize>,
    from: Rectangle,
    to: Rectangle,
    start: Option<Instant>,
}

impl State {
    fn rect(&self, now: Instant) -> Rectangle {
        let Some(start) = self.start else { return self.to };
        let t = (now.saturating_duration_since(start).as_secs_f32() / DURATION.as_secs_f32()).min(1.0);
        let e = 1.0 - (1.0 - t).powi(3);
        let lerp = |a: f32, b: f32| a + (b - a) * e;
        Rectangle {
            x: lerp(self.from.x, self.to.x),
            y: lerp(self.from.y, self.to.y),
            width: lerp(self.from.width, self.to.width),
            height: lerp(self.from.height, self.to.height),
        }
    }

    fn moving(&self, now: Instant) -> bool {
        self.start.is_some_and(|s| now.saturating_duration_since(s) < DURATION)
    }
}

/// The selected tab's rectangle, relative to the bar.
fn target(layout: Layout<'_>, selected: usize) -> Option<Rectangle> {
    let bar = layout.bounds();
    let tab = layout.children().nth(selected)?.bounds();
    Some(Rectangle { x: tab.x - bar.x, y: tab.y - bar.y, ..tab })
}

impl<M, Theme, Renderer: renderer::Renderer> Widget<M, Theme, Renderer> for Tabs<'_, M, Theme, Renderer> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.off.as_widget()), Tree::new(self.on.as_widget())]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.off.as_widget(), self.on.as_widget()]);
    }

    fn size(&self) -> Size<Length> {
        self.off.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.off.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let off = self.off.as_widget_mut().layout(&mut tree.children[0], renderer, limits);
        let on = self.on.as_widget_mut().layout(&mut tree.children[1], renderer, &layout::Limits::new(off.size(), off.size()));
        layout::Node::with_children(off.size(), vec![off, on])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let off = layout.children().next().expect("off");
        self.off.as_widget_mut().update(&mut tree.children[0], event, off, cursor, renderer, clipboard, shell, viewport);

        // Follow the selection: slide from wherever the pill is now.
        let now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => *now,
            _ => Instant::now(),
        };
        let state = tree.state.downcast_mut::<State>();
        match self.selected.and_then(|i| target(off, i)) {
            Some(to) if state.shown != self.selected => {
                state.from = if state.shown.is_some() { state.rect(now) } else { to };
                state.to = to;
                state.start = state.shown.is_some().then_some(now);
                state.shown = self.selected;
            }
            // Same tab: keep up with layout changes.
            Some(to) => state.to = to,
            None => state.shown = None,
        }
        if state.moving(now) {
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let mut children = layout.children();
        let (off, on) = (children.next().expect("off"), children.next().expect("on"));
        self.off.as_widget().draw(&tree.children[0], renderer, theme, style, off, cursor, viewport);

        let state = tree.state.downcast_ref::<State>();
        // Before the first update the state doesn't know the tab yet: draw it in place.
        let rect = match (self.selected, state.shown) {
            (Some(_), Some(_)) => state.rect(Instant::now()),
            (Some(i), None) => match target(off, i) {
                Some(r) => r,
                None => return,
            },
            (None, _) => return,
        };
        let bar = layout.bounds();
        let pill = rect + Vector::new(bar.x, bar.y);
        renderer.fill_quad(
            renderer::Quad {
                bounds: pill,
                border: Border { radius: self.pill.radius.into(), ..Default::default() },
                shadow: self.pill.shadow,
                snap: false,
            },
            self.pill.background,
        );
        renderer.with_layer(pill, |r| {
            self.on.as_widget().draw(&tree.children[1], r, theme, style, on, mouse::Cursor::Unavailable, &pill);
        });
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        let off = layout.children().next().expect("off");
        self.off.as_widget().mouse_interaction(&tree.children[0], off, cursor, viewport, renderer)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        let off = layout.children().next().expect("off");
        self.off.as_widget_mut().operate(&mut tree.children[0], off, renderer, operation);
    }
}

impl<'a, M: 'a, Theme: 'a, Renderer: renderer::Renderer + 'a> From<Tabs<'a, M, Theme, Renderer>> for Element<'a, M, Theme, Renderer> {
    fn from(t: Tabs<'a, M, Theme, Renderer>) -> Self {
        Element::new(t)
    }
}
