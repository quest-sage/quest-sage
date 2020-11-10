use crate::graphics::*;
use futures::future::{BoxFuture, FutureExt};
use stretch::geometry::Point;

/// A `MultiBatch` renders layers of content by sending data to multiple batches
/// to be rendered with the fewest possible draw calls (within reasonable computational complexity).
///
/// To render something using a multibatch, it must be split into several layers, where the elements of
/// each layer are ideally rendered concurrently where possible.
pub struct MultiBatch {
    pub batch: Batch,
    pub text_renderer: TextRenderer,
}

impl MultiBatch {
    pub fn new(batch: Batch, text_renderer: TextRenderer) -> Self {
        Self {
            batch,
            text_renderer,
        }
    }

    pub async fn render(
        &mut self,
        renderable: MultiRenderable,
        frame: &wgpu::SwapChainTexture,
        camera: &Camera,
        mut profiler: qs_common::profile::ProfileSegmentGuard<'_>,
    ) {
        let render_data = self.generate_render_data(renderable).await;
        self.text_renderer.draw_text(&render_data, frame, camera, profiler.task("text").time()).await;
    }

    fn generate_render_data(&self, renderable: MultiRenderable) -> BoxFuture<Vec<(Point<f32>, RenderableWord)>> {
        async move {
            match renderable {
                MultiRenderable::Nothing => Vec::new(),
                MultiRenderable::Layered(layers) => { unimplemented!() }
                MultiRenderable::Adjacent(items) => {
                    let mut text = Vec::new();
                    for item in items {
                        text.append(&mut self.generate_render_data(item).await);
                    }
                    text
                }
                MultiRenderable::Text { word, offset } => {
                    vec![ (offset, word) ]
                }
            }
        }.boxed()
    }
}

/// This contains high-level information about which batch to use for rendering, and how to configure it.
pub enum MultiRenderable {
    /// Render nothing.
    Nothing,

    /// This is a list of layers of renderables to render.
    /// - If this is empty, nothing will be rendered. No draw calls will be used.
    /// - If this has one element, that element will be rendered alongside sibling
    /// elements. The order of rendering is not guaranteed.
    /// - If this has more than one element, then previous layers are rendered before later layers.
    /// The whole element is rendered alongside sibling elements; the order of rendering between
    /// siblings is not guaranteed.
    Layered(Vec<MultiRenderable>),

    /// The list of items are rendered alongside each other with no regard for ordering.
    Adjacent(Vec<MultiRenderable>),

    /// Render some text using the text render batch.
    Text {
        word: RenderableWord,
        offset: Point<f32>,
    },
}
