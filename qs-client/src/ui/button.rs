use stretch::{geometry::Size, style::Dimension, result::Layout};

use crate::graphics::MultiRenderable;

use super::UiElement;

pub struct Button;

impl UiElement for Button {
    fn get_size(&self) -> Size<Dimension> {
        Size {
            width: Dimension::Auto,
            height: Dimension::Auto,
        }
    }

    fn generate_render_info(&self, _layout: &Layout) -> MultiRenderable {
        MultiRenderable::Nothing
    }

    fn mouse_enter(&mut self) {
        tracing::trace!("Entered button")
    }

    fn mouse_move(&mut self, pos: stretch::geometry::Point<f32>) {
        tracing::trace!("Moved in button {:?}", pos)
    }

    fn mouse_leave(&mut self) {
        tracing::trace!("Left button")
    }
}
