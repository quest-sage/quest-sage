use qs_common::assets::Asset;
use stretch::{geometry::Size, result::Layout, style::Dimension};

use crate::graphics::{MultiRenderable, Renderable, Texture, Vertex};

use super::{Colour, UiElement};

pub struct ImageWidget {
    pub size: Size<Dimension>,
    pub colour: Colour,
    pub texture: Asset<Texture>,
}

impl UiElement for ImageWidget {
    fn get_size(&self) -> Size<Dimension> {
        self.size
    }

    fn generate_render_info(&self, layout: &Layout) -> MultiRenderable {
        let color = self.colour.into();
        MultiRenderable::Image {
            texture: self.texture.clone(),
            renderables: vec![Renderable::Quadrilateral(
                Vertex {
                    position: [layout.location.x, -layout.location.y, 0.0],
                    color,
                    tex_coords: [0.0, 0.0],
                },
                Vertex {
                    position: [
                        layout.location.x + layout.size.width,
                        -layout.location.y,
                        0.0,
                    ],
                    color,
                    tex_coords: [1.0, 0.0],
                },
                Vertex {
                    position: [
                        layout.location.x + layout.size.width,
                        -layout.location.y - layout.size.height,
                        0.0,
                    ],
                    color,
                    tex_coords: [1.0, 1.0],
                },
                Vertex {
                    position: [
                        layout.location.x,
                        -layout.location.y - layout.size.height,
                        0.0,
                    ],
                    color,
                    tex_coords: [0.0, 1.0],
                },
            )],
        }
    }
}
