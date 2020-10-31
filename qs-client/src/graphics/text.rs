use crate::graphics::{Batch, Renderable, Vertex};
use qs_common::assets::{Asset, OwnedAsset};
use rusttype::{gpu_cache::Cache, point, vector, Font, PositionedGlyph, Rect, Scale};
use std::sync::{Arc, RwLock};
use wgpu::*;

/// Caches rendered glyphs to speed up the rendering process of text.
/// Contains a font used to render this text.
/// Contains its own batch configured for the text rendering workflow.
pub struct TextRenderer {
    /// `wgpu` handles so that we can dynamically update the texture.
    device: Arc<Device>,
    queue: Arc<Queue>,
    batch: Batch,

    font: Asset<Font<'static>>,
    /// The size of the font, in points.
    font_size: f32,
    /// The UI scale factor.
    scale_factor: f32,

    /// A cache containing CPU-side rendered font glyphs.
    cache: Cache<'static>,
    /// The texture containing pre-rendered GPU-side font glyphs.
    font_texture: OwnedAsset<crate::graphics::Texture>,
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
        font: Asset<Font<'static>>,
        font_size: f32,
        scale_factor: f32,
    ) -> Self {
        let batch = Batch::new(
            &*device,
            include_spirv!("text.vert.spv"),
            include_spirv!("text.frag.spv"),
            texture_bind_group_layout,
            uniform_bind_group_layout,
            swap_chain_format,
        );

        let (cache_width, cache_height) =
            ((512.0 * scale_factor) as u32, (512.0 * scale_factor) as u32);

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
        let font_texture =
            OwnedAsset::new(crate::graphics::Texture::from_wgpu(&*device, font_texture));

        Self {
            device,
            queue,
            batch,

            font,
            font_size,
            scale_factor,

            cache,
            font_texture,
        }
    }

    pub fn draw_text(
        &mut self,
        text: &str,
        width: u32,
        frame: &wgpu::SwapChainTexture,
        camera: &crate::graphics::Camera,
    ) {
        self.font.clone().if_loaded(|font| {
            let glyphs = layout_paragraph(
                font,
                Scale::uniform(self.font_size * self.scale_factor),
                width,
                text,
            );
            for glyph in &glyphs {
                self.cache.queue_glyph(0, glyph.clone());
            }

            let queue = &self.queue;
            let cache = &mut self.cache;
            self.font_texture.if_loaded(|font_texture| {
                cache
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
            });

            let mut items = Vec::new();
            for glyph in glyphs {
                if let Some((uv_rect, pixel_rect)) = cache
                    .rect_for(0, &glyph)
                    .expect("Could not load cache entry for glyph")
                {
                    let (x1, y1) = (pixel_rect.min.x as f32, -pixel_rect.min.y as f32);
                    let (x2, y2) = (pixel_rect.max.x as f32, -pixel_rect.max.y as f32);
                    let (u1, v1) = (uv_rect.min.x, uv_rect.min.y);
                    let (u2, v2) = (uv_rect.max.x, uv_rect.max.y);
                    items.push(Renderable::Quadrilateral(
                        Vertex {
                            position: [x1, y1, 0.0],
                            color: [1.0, 1.0, 1.0, 1.0],
                            tex_coords: [u1, v1],
                        },
                        Vertex {
                            position: [x2, y1, 0.0],
                            color: [1.0, 1.0, 1.0, 1.0],
                            tex_coords: [u2, v1],
                        },
                        Vertex {
                            position: [x2, y2, 0.0],
                            color: [1.0, 1.0, 1.0, 1.0],
                            tex_coords: [u2, v2],
                        },
                        Vertex {
                            position: [x1, y2, 0.0],
                            color: [1.0, 1.0, 1.0, 1.0],
                            tex_coords: [u1, v2],
                        },
                    ));
                }
            }

            self.batch.render(
                &*self.device,
                &*self.queue,
                frame,
                &self.font_texture,
                camera,
                items.into_iter(),
            );
        });
    }
}

fn layout_paragraph<'a>(
    font: &Font<'a>,
    scale: Scale,
    width: u32,
    text: &str,
) -> Vec<PositionedGlyph<'a>> {
    // https://gitlab.redox-os.org/redox-os/rusttype/-/blob/master/dev/examples/gpu_cache.rs
    let mut result = Vec::new();
    let v_metrics = font.v_metrics(scale);
    let advance_height = v_metrics.ascent - v_metrics.descent + v_metrics.line_gap;
    let mut caret = point(0.0, v_metrics.ascent);
    let mut last_glyph_id = None;
    for c in text.chars() {
        if c.is_control() {
            match c {
                '\r' => {
                    caret = point(0.0, caret.y + advance_height);
                }
                '\n' => {}
                _ => {}
            }
            continue;
        }
        let base_glyph = font.glyph(c);
        if let Some(id) = last_glyph_id.take() {
            caret.x += font.pair_kerning(scale, id, base_glyph.id());
        }
        last_glyph_id = Some(base_glyph.id());
        let mut glyph = base_glyph.scaled(scale).positioned(caret);
        if let Some(bb) = glyph.pixel_bounding_box() {
            if bb.max.x > width as i32 {
                caret = point(0.0, caret.y + advance_height);
                glyph.set_position(caret);
                last_glyph_id = None;
            }
        }
        caret.x += glyph.unpositioned().h_metrics().advance_width;
        result.push(glyph);
    }
    result
}
