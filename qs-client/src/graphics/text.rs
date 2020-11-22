use crate::graphics::Batch;
use crate::ui::*;
use rusttype::gpu_cache::Cache;
use std::sync::Arc;
use stretch::geometry::Point;
use wgpu::*;

use super::{Renderable, Vertex};

/// Caches rendered glyphs to speed up the rendering process of text.
/// Contains a font used to render this text.
/// Contains its own batch configured for the text rendering workflow.
pub struct TextRenderer {
    /// `wgpu` handles so that we can dynamically update the texture.
    queue: Arc<Queue>,
    batch: Batch,

    /// The UI scale factor.
    /// TODO maybe make this some kind of global state?
    //scale_factor: f32,

    /// A cache containing CPU-side rendered font glyphs.
    cache: Cache<'static>,
    /// The texture containing pre-rendered GPU-side font glyphs.
    font_texture: crate::graphics::Texture,

    /// Sometimes when we add new elements to the cache, we need to reorder or delete previous elements.
    /// Whenever this happens, we increment the 'generation' of the cache. Whenever the generation of the
    /// cache does not match with cached texture coordinates in `TypesetText`, we will need to recalculate them.
    cache_generation: u64,
}

impl TextRenderer {
    /// # Arguments
    /// - `font_size`: The size of the font, in points.
    /// - `scale_factor`: The UI scale factor.
    pub fn new(
        device: Arc<Device>,
        queue: Arc<Queue>,
        texture_bind_group_layout: BindGroupLayout,
        uniform_bind_group_layout: BindGroupLayout,
        swap_chain_format: TextureFormat,
        scale_factor: f32,
    ) -> Self {
        let batch = Batch::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            include_spirv!("text.vert.spv"),
            include_spirv!("text.frag.spv"),
            texture_bind_group_layout,
            uniform_bind_group_layout,
            swap_chain_format,
        );

        const SIZE: f32 = 1024.0;
        let (cache_width, cache_height) =
            ((SIZE * scale_factor) as u32, (SIZE * scale_factor) as u32);

        let cache = Cache::builder()
            .dimensions(cache_width, cache_height)
            .multithread(true)
            .build();

        let font_texture = device.create_texture(&TextureDescriptor {
            label: Some("font_cache"),
            size: wgpu::Extent3d {
                width: cache_width,
                height: cache_height,
                depth: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
        });
        let font_texture = crate::graphics::Texture::from_wgpu_with_sampler(
            &*device,
            font_texture,
            &wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                mipmap_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            },
            (cache_width, cache_height),
        );

        Self {
            queue,
            batch,

            //scale_factor,
            cache,
            font_texture,

            cache_generation: 0,
        }
    }

    /// Text is a list of words together with an offset at which to draw them.
    pub fn draw_text(
        &mut self,
        text: Vec<(Point<f32>, RenderableWord)>,
        frame: &wgpu::SwapChainTexture,
        camera: &crate::graphics::Camera,
        //mut profiler: qs_common::profile::ProfileSegmentGuard<'_>,
    ) {
        {
            //let _guard = profiler.task("queuing glyphs").time();
            for (_, word) in &text {
                for RenderableGlyph { font, glyph, .. } in &word.glyphs {
                    self.cache.queue_glyph(*font, glyph.clone());
                }
            }
        }

        {
            //let _guard = profiler.task("caching glyphs").time();
            let cache = &mut self.cache;
            let queue = &self.queue;
            let font_texture = &self.font_texture;
            let cache_method = cache
                .cache_queued(|rect, data| {
                    queue.write_texture(
                        wgpu::TextureCopyView {
                            texture: &font_texture.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: rect.min.x,
                                y: rect.min.y,
                                z: 0,
                            },
                        },
                        data,
                        wgpu::TextureDataLayout {
                            offset: 0,
                            bytes_per_row: rect.width(),
                            rows_per_image: 0,
                        },
                        wgpu::Extent3d {
                            width: rect.width(),
                            height: rect.height(),
                            depth: 1,
                        },
                    );
                })
                .unwrap();
            if let rusttype::gpu_cache::CachedBy::Reordering = cache_method {
                self.cache_generation += 1;
            }
        }

        let mut items = Vec::new();
        {
            //let _guard = profiler.task("creating texture coordinates").time();
            /*if self.cache_generation == cache_generation && self.cached_renderables.is_some() {
                items = self.cached_renderables.as_ref().unwrap().clone();
            } else */
            {
                for (offset, word) in text {
                    for RenderableGlyph {
                        font,
                        colour,
                        glyph,
                    } in &word.glyphs
                    {
                        if let Some((uv_rect, pixel_rect)) = self
                            .cache
                            .rect_for(*font, glyph)
                            .expect("Could not load cache entry for glyph")
                        {
                            // TODO this includes the height of descenders of glyphs, which is not intended!
                            // This displays text slightly too low!
                            let line_height = word.size.1 as f32;
                            let (x1, y1) = (
                                pixel_rect.min.x as f32 + offset.x,
                                -pixel_rect.min.y as f32 - line_height - offset.y,
                            );
                            let (x2, y2) = (
                                pixel_rect.max.x as f32 + offset.x,
                                -pixel_rect.max.y as f32 - line_height - offset.y,
                            );
                            let (u1, v1) = (uv_rect.min.x, uv_rect.min.y);
                            let (u2, v2) = (uv_rect.max.x, uv_rect.max.y);
                            let color = (*colour).into();
                            items.push(Renderable::Quadrilateral(
                                Vertex {
                                    position: [x1, y1, 0.0],
                                    color,
                                    tex_coords: [u1, v1],
                                },
                                Vertex {
                                    position: [x2, y1, 0.0],
                                    color,
                                    tex_coords: [u2, v1],
                                },
                                Vertex {
                                    position: [x2, y2, 0.0],
                                    color,
                                    tex_coords: [u2, v2],
                                },
                                Vertex {
                                    position: [x1, y2, 0.0],
                                    color,
                                    tex_coords: [u1, v2],
                                },
                            ));
                        }
                    }
                }

                //word.cached_renderables = Some(items.clone());
            }
        }

        {
            //let _guard = profiler.task("rendering text").time();
            self.batch
                .render(frame, &self.font_texture, camera, items.into_iter());
        }
    }
}
