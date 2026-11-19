//! `Slide`: fills its area, keeps its content at its own width against the right (or left) edge,
//! pushed outwards by `offset`, and clips it to the area. Used to slide the sheets in from the
//! window's edges. (iced's `pin` squeezes content that doesn't fit; `float` doesn't clip.)

use nice_plug_iced::iced::{
    Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        renderer::Renderer as _,
        widget::{self, Tree},
    },
};

pub struct Slide<'a, M> {
    content: Element<'a, M>,
    width: f32,
    offset: f32,
    left: bool,
}

/// Against the right edge. `width`: the content's width; `offset`: how far it is pushed out to
/// the right (0 = fully in).
pub fn slide<'a, M>(content: impl Into<Element<'a, M>>, width: f32, offset: f32) -> Slide<'a, M> {
    Slide { content: content.into(), width, offset, left: false }
}

/// Against the left edge, pushed out to the left by `offset`.
pub fn slide_left<'a, M>(content: impl Into<Element<'a, M>>, width: f32, offset: f32) -> Slide<'a, M> {
    Slide { content: content.into(), width, offset, left: true }
}

impl<M> Widget<M, Theme, Renderer> for Slide<'_, M> {
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        Size { width: Length::Fill, height: Length::Fill }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let area = limits.width(Length::Fill).height(Length::Fill).max();
        let content_limits = layout::Limits::new(Size::ZERO, Size::new(self.width, area.height));
        let node = self.content.as_widget_mut().layout(tree, renderer, &content_limits);
        let x = if self.left { -self.offset } else { area.width - node.size().width + self.offset };
        layout::Node::with_children(area, vec![node.move_to((x, 0.0))])
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn widget::Operation) {
        self.content.as_widget_mut().operate(tree, layout.children().next().unwrap(), renderer, operation);
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
        self.content
            .as_widget_mut()
            .update(tree, event, layout.children().next().unwrap(), cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(tree, layout.children().next().unwrap(), cursor, viewport, renderer)
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
        let bounds = layout.bounds();
        if let Some(clip) = bounds.intersection(viewport) {
            renderer.with_layer(clip, |renderer| {
                self.content.as_widget().draw(tree, renderer, theme, style, layout.children().next().unwrap(), cursor, &clip);
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout.children().next().unwrap(), renderer, viewport, translation)
    }
}

impl<'a, M: 'a> From<Slide<'a, M>> for Element<'a, M> {
    fn from(s: Slide<'a, M>) -> Self {
        Element::new(s)
    }
}
