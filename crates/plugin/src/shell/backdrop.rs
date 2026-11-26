//! What the renderer's blur applies to: the content is drawn in a layer of its own, marked as the
//! backdrop (`iced_wgpu::postfx`, patched). While a blur runs, only this layer (and what's under
//! it) goes through it; everything drawn after it (the block modal, the notch, the view switch,
//! sheets, the modulator editor, menus) stays sharp, whatever its shape.
//!
//! Used for the chain under the camera in the chain ↔ block modal transition.

use nice_plug_iced::iced::{
    Element, Event, Length, Rectangle, Size,
    advanced::{
        Clipboard, Layout, Shell, Widget,
        layout, mouse, renderer,
        widget::{Operation, Tree, tree},
    },
};

pub struct Backdrop<'a, M, Theme, Renderer> {
    content: Element<'a, M, Theme, Renderer>,
}

pub fn backdrop<'a, M, Theme, Renderer>(content: impl Into<Element<'a, M, Theme, Renderer>>) -> Backdrop<'a, M, Theme, Renderer> {
    Backdrop { content: content.into() }
}

impl<M, Theme, Renderer: renderer::Renderer> Widget<M, Theme, Renderer> for Backdrop<'_, M, Theme, Renderer> {
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
        self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
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
        iced_wgpu::postfx::mark_backdrop_start();
        renderer.with_layer(layout.bounds(), |r| {
            self.content.as_widget().draw(tree, r, theme, style, layout, cursor, viewport);
            iced_wgpu::postfx::mark_backdrop_end();
        });
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
    }
}

impl<'a, M: 'a, Theme: 'a, Renderer: renderer::Renderer + 'a> From<Backdrop<'a, M, Theme, Renderer>> for Element<'a, M, Theme, Renderer> {
    fn from(b: Backdrop<'a, M, Theme, Renderer>) -> Self {
        Element::new(b)
    }
}
