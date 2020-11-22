use std::sync::{Arc, Mutex};

use qs_common::assets::Asset;
use texture_atlas::{TextureAtlas, TextureRegionInformation};

use crate::ui::Colour;

use super::{MultiRenderable, Renderable, Vertex};

/// Represents a texture. Encapsulates several `wgpu` and `image` operations, such
/// as loading the image from raw bytes.
pub struct Texture {
    pub dimensions: (u32, u32),
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

// https://sotrh.github.io/learn-wgpu/beginner/tutorial5-textures/#cleaning-things-up
impl Texture {
    /// Create a texture directly from a texture on the graphics card.
    pub fn from_wgpu(
        device: &wgpu::Device,
        texture: wgpu::Texture,
        dimensions: (u32, u32),
    ) -> Self {
        Self::from_wgpu_with_sampler(
            device,
            texture,
            &wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Nearest,
                mipmap_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            },
            dimensions,
        )
    }

    /// Create a texture directly from a texture on the graphics card.
    pub fn from_wgpu_with_sampler(
        device: &wgpu::Device,
        texture: wgpu::Texture,
        desc: &wgpu::SamplerDescriptor,
        dimensions: (u32, u32),
    ) -> Self {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            dimensions,
            texture,
            view,
            sampler: device.create_sampler(desc),
        }
    }

    pub fn from_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        label: &str,
    ) -> Result<Self, image::ImageError> {
        let img = image::load_from_memory(bytes)?;
        Self::from_image(device, queue, &img, Some(label))
    }

    pub fn from_image(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        img: &image::DynamicImage,
        label: Option<&str>,
    ) -> Result<Self, image::ImageError> {
        use image::GenericImageView;
        let rgba = img.to_rgba();
        let dimensions = img.dimensions();

        let size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
        });

        queue.write_texture(
            wgpu::TextureCopyView {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            &rgba,
            wgpu::TextureDataLayout {
                offset: 0,
                bytes_per_row: 4 * dimensions.0,
                rows_per_image: dimensions.1,
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Ok(Self {
            dimensions,
            texture,
            view,
            sampler,
        })
    }
}

/// Represents a texture that has been split into several regions.
/// The regions are addressable using the texture atlas provided.
pub struct PartitionedTexture {
    /// The texture from which to retrieve texture regions.
    pub base_texture: Texture,
    /// The atlas that contains useful information about how texture regions are contained within this texture.
    pub atlas: TextureAtlas,
}

#[derive(Debug, Copy, Clone)]
struct InternalTextureRegionInformation {
    /// Contains most of the info about how to render this region.
    info: TextureRegionInformation,
    /// The width and height of the original partitioned texture.
    partitioned_texture_size: (u32, u32),
}

/// A smaller region of a partitioned texture. This is commonly used to refer to smaller images inside a large texture that packs them all together.
///
/// The info field is populated automatically on a background task when the texture has finished loading.
#[derive(Debug, Clone)]
pub struct TextureRegion {
    /// The texture that this region is contained within.
    pub partitioned_texture: Asset<PartitionedTexture>,

    /// Tells us where the region is located within the base texture.
    /// This is a mutex not a rwlock for simplicity since it'll only ever be written to once.
    info: Arc<Mutex<Option<InternalTextureRegionInformation>>>,
}

impl TextureRegion {
    /// Creates a new texture region as a named region of the given partitioned texture.
    pub async fn new(partitioned_texture: Asset<PartitionedTexture>, name: String) -> Self {
        let region = Self {
            partitioned_texture: partitioned_texture.clone(),
            info: Arc::new(Mutex::new(None)),
        };
        let cloned = region.clone();
        partitioned_texture
            .on_load(move |tex| match tex.atlas.frames.get(&name) {
                Some(info) => {
                    *cloned.info.try_lock().unwrap() = Some(InternalTextureRegionInformation {
                        info: *info,
                        partitioned_texture_size: tex.base_texture.dimensions,
                    });
                }
                None => {
                    tracing::error!("region {} not found in partitioned texture", name);
                }
            })
            .await;
        region
    }
}

/// Splits a texture into nine pieces, a 3x3 grid, where the sizes of the pieces are represented using pixel measurements.
/// The margins given should all be positive, and the totals of x-direction and y-direction margins should not exceed the total texture size.
#[derive(Debug, Clone)]
pub struct NinePatch {
    pub texture_region: TextureRegion,

    pub left_margin: u32,
    pub right_margin: u32,
    pub top_margin: u32,
    pub bottom_margin: u32,
}

impl NinePatch {
    pub fn no_margins(texture_region: TextureRegion) -> Self {
        Self {
            texture_region,
            left_margin: 0,
            right_margin: 0,
            top_margin: 0,
            bottom_margin: 0,
        }
    }

    /// `x` and `y` represent the bottom-left corner of the shape.
    pub fn generate_render_info(
        &self,
        colour: Colour,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> MultiRenderable {
        // We need to create 16 vertices for the 3x3 grid.

        let InternalTextureRegionInformation {
            info: TextureRegionInformation { frame, .. },
            partitioned_texture_size,
        } = match *self.texture_region.info.try_lock().unwrap() {
            Some(tex) => tex,
            None => return MultiRenderable::Nothing,
        };

        let tex_w = partitioned_texture_size.0 as f32;
        let tex_h = partitioned_texture_size.1 as f32;

        // Therefore, we have four x-positions and four y-positions for coordinates,
        // and four u-positions and v-positions for texture coordinates.
        let u_positions = [
            frame.x as f32 / tex_w,
            (frame.x as f32 + self.left_margin as f32) / tex_w,
            (frame.x as f32 + frame.w as f32 - self.right_margin as f32) / tex_w,
            (frame.x as f32 + frame.w as f32) / tex_w,
        ];
        let v_positions = [
            frame.y as f32 / tex_h,
            (frame.y as f32 + self.bottom_margin as f32) / tex_h,
            (frame.y as f32 + frame.h as f32 - self.top_margin as f32) / tex_h,
            (frame.y as f32 + frame.h as f32) / tex_h,
        ];

        let x_positions = [
            x,
            x + self.left_margin as f32,
            x + width - self.right_margin as f32,
            x + width,
        ];
        let y_positions = [
            y,
            y + self.bottom_margin as f32,
            y + height - self.top_margin as f32,
            y + height,
        ];

        let color = colour.into();

        MultiRenderable::ImageRegion {
            texture: self.texture_region.clone(),
            renderables: [
                (0, 0),
                (0, 1),
                (0, 2),
                (1, 0),
                (1, 1),
                (1, 2),
                (2, 0),
                (2, 1),
                (2, 2),
            ]
            .iter()
            .copied()
            .map(|(i, j)| {
                Renderable::Quadrilateral(
                    Vertex {
                        position: [x_positions[i], y_positions[j], 0.0],
                        color,
                        tex_coords: [u_positions[i], v_positions[j]],
                    },
                    Vertex {
                        position: [x_positions[i + 1], y_positions[j], 0.0],
                        color,
                        tex_coords: [u_positions[i + 1], v_positions[j]],
                    },
                    Vertex {
                        position: [x_positions[i + 1], y_positions[j + 1], 0.0],
                        color,
                        tex_coords: [u_positions[i + 1], v_positions[j + 1]],
                    },
                    Vertex {
                        position: [x_positions[i], y_positions[j + 1], 0.0],
                        color,
                        tex_coords: [u_positions[i], v_positions[j + 1]],
                    },
                )
            })
            .collect(),
        }
    }
}
