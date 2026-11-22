//! A widget that draws its content scaled around a point and then moved (a camera), clipped to
//! its own bounds, in normal draw order (iced's `Float` only scales up and moves the content into
//! the overlay layer, above the notch and the sheets). Used by the chain ↔ block modal
//! transition: the chain zooms into a block, the modal's content grows out of it. While moved or
//! scaled, the content gets no input.

use nice_plug_iced::iced::{
    Element, Length, Point, Rectangle, Size, Transformation, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget,
        layout, mouse, renderer,
        widget::{Operation, Tree, tree},
    },
    Event,
};

pub struct Zoom<'a, M, Theme, Renderer> {
    content: Element<'a, M, Theme, Renderer>,
    scale: f32,
    /// The fixed point, relative to the widget's top-left.
    origin: Point,
    /// Where the fixed point moves to, after scaling.
    offset: Vector,
}

/// `content` scaled by `scale` around `origin` (relative to the widget).
pub fn zoom<'a, M, Theme, Renderer>(content: impl Into<Element<'a, M, Theme, Renderer>>, scale: f32, origin: Point) -> Zoom<'a, M, Theme, Renderer> {
    Zoom { content: content.into(), scale, origin, offset: Vector::ZERO }
}

impl<'a, M, Theme, Renderer: renderer::Renderer> Zoom<'a, M, Theme, Renderer> {
    /// Then moved by `offset`: the content point at `origin` lands at `origin + offset`.
    pub fn offset(mut self, offset: Vector) -> Self {
        self.offset = offset;
        self
    }

    fn still(&self) -> bool {
        (self.scale - 1.0).abs() < 1e-4 && self.offset.x.abs() < 1e-3 && self.offset.y.abs() < 1e-3
    }
}

impl<M, Theme, Renderer: renderer::Renderer> Widget<M, Theme, Renderer> for Zoom<'_, M, Theme, Renderer> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
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
        if self.still() {
            self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
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
        if self.still() {
            return self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        let bounds = layout.bounds();
        let o = Vector::new(bounds.x + self.origin.x, bounds.y + self.origin.y);
        let t = Transformation::translate(o.x + self.offset.x, o.y + self.offset.y)
            * Transformation::scale(self.scale)
            * Transformation::translate(-o.x, -o.y);
        // The content as it is laid out, seen through the transformation; the viewport is what
        // of it lands inside our bounds (screen = o + offset + (content − o) · scale).
        let inverse = 1.0 / self.scale.max(1e-4);
        let seen = Rectangle {
            x: o.x + (bounds.x - o.x - self.offset.x) * inverse,
            y: o.y + (bounds.y - o.y - self.offset.y) * inverse,
            width: bounds.width * inverse,
            height: bounds.height * inverse,
        };
        renderer.with_layer(bounds, |r| {
            r.with_transformation(t, |r| {
                self.content.as_widget().draw(tree, r, theme, style, layout, mouse::Cursor::Unavailable, &seen);
            });
        });
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        if self.still() {
            self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
        } else {
            mouse::Interaction::default()
        }
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
    }
}

impl<'a, M: 'a, Theme: 'a, Renderer: renderer::Renderer + 'a> From<Zoom<'a, M, Theme, Renderer>> for Element<'a, M, Theme, Renderer> {
    fn from(z: Zoom<'a, M, Theme, Renderer>) -> Self {
        Element::new(z)
    }
}
