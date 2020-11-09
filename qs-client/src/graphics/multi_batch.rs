use crate::graphics::*;

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
        match renderable {
            MultiRenderable::Nothing => {}
            MultiRenderable::Layered(layers) => { unimplemented!() }
            MultiRenderable::Adjacent(items) => { unimplemented!() }
            MultiRenderable::Text(text) => {
                self.text_renderer.draw_text(&text, frame, camera, profiler.task("text").time()).await;
            }
        }
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
    Text(RichText),
}
