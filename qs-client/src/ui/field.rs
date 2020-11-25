use std::sync::Arc;

use stretch::{
    geometry::{Point, Size},
    style::{Dimension, Style},
};
use winit::event::{ElementState, MouseButton};

use crate::graphics::{MultiRenderable, NinePatch};

use super::*;

/// A text box the user can type into.
pub struct Field {
    rich_text: RichText,
    contents: String,
    widget: Widget,
}

/// A UI element for fields.
struct FieldElement {
    /// A clone of the rich text object contained within the Field.
    rich_text: RichText,
    /// The texture to draw the cursor with.
    caret_texture: NinePatch,

    /// The position and size of the caret relative to this widget, if this widget has keyboard focus.
    caret_position: Option<Caret>,
}

#[derive(Debug)]
struct Caret {
    /// The index in the `contents` string that the caret is at.
    edit_index: usize,
    /// The position to render the caret.
    pos: (f32, f32),
    /// The height in pixels to render the caret.
    height: f32,
}

impl UiElement for FieldElement {
    fn get_size(&self) -> Size<Dimension> {
        Default::default()
    }

    fn generate_render_info(&self, layout: &stretch::result::Layout) -> MultiRenderable {
        if let Some(Caret { pos: (x, y), height, .. }) = self.caret_position {
            self.caret_texture.generate_render_info(
                Colour::WHITE,
                layout.location.x + x - 2.0,
                -layout.location.y - y - height + 1.0,
                5.0,
                height - 2.0,
            )
        } else {
            MultiRenderable::Nothing
        }
    }

    fn mouse_move(&mut self, pos: Point<f32>) {
        tracing::trace!("Caret: {:#?}", self.get_caret_position(pos));
    }

    fn process_mouse_input(&mut self, button: MouseButton, state: ElementState) -> MouseInputProcessResult {
        if button == MouseButton::Left {
            match state {
                ElementState::Pressed => {
                    MouseInputProcessResult::TakeKeyboardFocus
                }
                ElementState::Released => {
                    // Don't let child widgets process this event.
                    MouseInputProcessResult::Processed
                }
            }
        } else {
            // Maybe add right-click events later?
            MouseInputProcessResult::NotProcessed
        }
    }
}

impl FieldElement {
    /// Returns the position of the caret when the mouse is hovered over the given point.
    fn get_caret_position(&self, pos: Point<f32>) -> Option<Caret> {
        let widget = self.rich_text.get_widget();
        let paragraphs = widget.0.read().unwrap();
        // Check where the mouse is hovering over.
        for paragraph in paragraphs
            .get_children()
            .iter()
            .map(|paragraph| paragraph.0.read().unwrap())
        {
            // We're iterating over each paragraph from top to bottom.
            // We will determine which paragraph the mouse is over by checking if the `y` position of the mouse is within the
            // paragraph's bounds.
            if let Some(paragraph_layout) = paragraph.get_layout() {
                // Check if the mouse's `y` position is within bounds of this paragraph.
                let local_y = pos.y - paragraph_layout.location.y;
                if 0.0 <= local_y && local_y < paragraph_layout.size.height {
                    // The mouse is in this paragraph. Which word are we hovering over, if any?
                    // We'll implement a naive algorithm (for now) that just checks if the mouse is over the given word's bounding box.
                    // Eventually we need to work out what to do when the mouse is too far right (select the last word) or too far left (select the first word)
                    // and deal with multi-line scenarios better.
                    for word in paragraph
                        .get_children()
                        .iter()
                        .map(|word| word.0.read().unwrap())
                    {
                        if let Some(word_layout) = word.get_layout() {
                            let local_x = pos.x - word_layout.location.x;
                            let local_y = pos.y - word_layout.location.y;
                            if 0.0 <= local_x
                                && 0.0 <= local_y
                                && local_x < word_layout.size.width
                                && local_y < word_layout.size.height
                            {
                                // We're hovering over this word.
                                if let Some(word_info) = self.rich_text.get_word_info(word.get_id()) {
                                    // Now, let's work out where our cursor is supposed to go within this word.
                                    // The right edges of characters (along with the left edge of the initial character) are 'anchor points';
                                    // the closest anchor point to the mouse is where the caret will go.
                                    let mut closest_anchor_point_index = 0;
                                    let mut closest_anchor_point_x_position = 0.0;
                                    let mut closest_anchor_point_distance = f32::MAX;
                                    for glyph in word_info.glyphs {
                                        if let Some(bounding_box) = glyph.bounding_box {
                                            // Evaluate the left edge if this is the first glyph with a bounding box (i.e. we haven't updated the closest point yet).
                                            if closest_anchor_point_distance == f32::MAX {
                                                let distance = (bounding_box.min.x as f32 - local_x).abs();
                                                if distance < closest_anchor_point_distance {
                                                    closest_anchor_point_index = glyph.character_index;
                                                    closest_anchor_point_x_position = bounding_box.min.x as f32;
                                                    closest_anchor_point_distance = distance;
                                                }
                                            }

                                            // Evaluate the right edge.
                                            let distance = (bounding_box.max.x as f32 - local_x).abs();
                                            if distance < closest_anchor_point_distance {
                                                closest_anchor_point_index = glyph.character_index + 1;
                                                closest_anchor_point_x_position = bounding_box.max.x as f32;
                                                closest_anchor_point_distance = distance;
                                            }
                                        }
                                    }

                                    // Now, `closest_anchor_point_index` is the index of the glyph before which our cursor should go,
                                    // and `closest_anchor_point_x_position` is the x-position that the caret should be rendered at.
                                    let caret = Caret {
                                        edit_index: closest_anchor_point_index,
                                        pos: (closest_anchor_point_x_position + word_layout.location.x, word_layout.location.y),
                                        height: word_layout.size.height,
                                    };
                                    return Some(caret);
                                }

                                // Don't check any other words, we've computed which one we're hovering over already.
                                return None;
                            }
                        }
                    }

                    // Don't check any other paragraphs, we've computed which one we're hovering over already.
                    return None;
                }
            }
        }

        None
    }
}

impl Field {
    pub fn new(
        caret_texture: NinePatch,
        font_family: Arc<FontFamily>,
        style: Style,
        text_style: Style,
    ) -> Self {
        let mut rich_text = RichText::new(text_style);
        let field_element = FieldElement {
            rich_text: rich_text.clone(),
            caret_texture,
            caret_position: None,
        };
        let widget = Widget::new(
            field_element,
            vec![rich_text.get_widget()],
            Vec::new(),
            style,
        );
        rich_text
            .set_text(font_family)
            .write("Hello, world! This is a field.")
            .finish();
        Self {
            rich_text,
            contents: String::new(),
            widget,
        }
    }

    pub fn get_widget(&self) -> Widget {
        self.widget.clone()
    }
}
