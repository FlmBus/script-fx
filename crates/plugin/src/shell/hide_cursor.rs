//! `hide_cursor`: the whole window's cursor while something is dragged (a knob, a fader, a meta
//! knob): hidden, wherever the pointer goes. iced picks the "largest" cursor among siblings and
//! `Hidden` is nearly the smallest, so a dragged control can't hide it on its own; the shell
//! wraps the window in this and says when something drags.
//!
//! Every left-button release reports `released` (window-wide, before the content sees it), so a
//! drag whose end got lost can't leave the cursor hidden.

use nice_plug_iced::iced::{
    Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{self, Tree},
    },
};

pub struct HideCursor<'a, M> {
    content: Element<'a, M>,
    hidden: bool,
    released: M,
}

/// `hidden`: something drags right now; `released`: what a left-button release reports.
pub fn hide_cursor<'a, M>(content: impl Into<Element<'a, M>>, hidden: bool, released: M) -> HideCursor<'a, M> {
    HideCursor { content: content.into(), hidden, released }
}

impl<M: Clone> Widget<M, Theme, Renderer> for HideCursor<'_, M> {
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
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn widget::Operation) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
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
        // Every input event passes through here, so this is where the diagnostics log counts them
        // (`diag.rs`): iced hands a frame the whole batch that piled up since the last one, so
        // "events per frame" is how far behind input is running.
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => crate::diag::note_event(true),
            Event::Mouse(_) | Event::Keyboard(_) | Event::Touch(_) => crate::diag::note_event(false),
            _ => {}
        }
        if let Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) = event {
            // Counted before the content sees it, so a control whose own release gets lost can
            // still tell that the button went up (`pointer_lock::Drag::ended`).
            super::pointer_lock::note_release();
            shell.publish(self.released.clone());
        }
        self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        if self.hidden {
            return mouse::Interaction::Hidden;
        }
        self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation)
    }
}

impl<'a, M: Clone + 'a> From<HideCursor<'a, M>> for Element<'a, M> {
    fn from(w: HideCursor<'a, M>) -> Self {
        Element::new(w)
    }
}
