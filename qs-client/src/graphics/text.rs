use crate::graphics::Batch;
use crate::ui::*;
use qs_common::assets::OwnedAsset;
use rusttype::gpu_cache::Cache;
use std::sync::Arc;
use wgpu::*;

/// Caches rendered glyphs to speed up the rendering process of text.
/// Contains a font used to render this text.
/// Contains its own batch configured for the text rendering workflow.
pub struct TextRenderer {
    /// `wgpu` handles so that we can dynamically update the texture.
    device: Arc<Device>,
    queue: Arc<Queue>,
    batch: Batch,

    /// The UI scale factor.
    scale_factor: f32,

    /// A cache containing CPU-side rendered font glyphs.
    cache: Cache<'static>,
    /// The texture containing pre-rendered GPU-side font glyphs.
    font_texture: OwnedAsset<crate::graphics::Texture>,

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
            &*device,
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
        let font_texture = OwnedAsset::new(crate::graphics::Texture::from_wgpu_with_sampler(
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
        ));

        Self {
            device,
            queue,
            batch,

            scale_factor,

            cache,
            font_texture,

            cache_generation: 0,
        }
    }

    pub async fn draw_text(
        &mut self,
        text: &RichText,
        frame: &wgpu::SwapChainTexture,
        camera: &crate::graphics::Camera,
        profiler: qs_common::profile::ProfileSegmentGuard<'_>,
    ) {
        let mut write = text.0.write().unwrap();
        if let Some(text) = &mut write.typeset {
            self.cache_generation = text
                .render(
                    profiler,
                    &*self.device,
                    &*self.queue,
                    frame,
                    &mut self.cache,
                    &mut self.batch,
                    &self.font_texture,
                    camera,
                    self.cache_generation,
                )
                .await;
        }
    }
}
