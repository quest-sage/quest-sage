use qs_common::assets::Asset;

use crate::ui::Colour;

use super::{MultiRenderable, Renderable, Vertex};

/// Represents a texture. Encapsulates several `wgpu` and `image` operations, such
/// as loading the image from raw bytes.
pub struct Texture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

// https://sotrh.github.io/learn-wgpu/beginner/tutorial5-textures/#cleaning-things-up
impl Texture {
    /// Create a texture directly from a texture on the graphics card.
    pub fn from_wgpu(device: &wgpu::Device, texture: wgpu::Texture) -> Self {
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
        )
    }

    /// Create a texture directly from a texture on the graphics card.
    pub fn from_wgpu_with_sampler(
        device: &wgpu::Device,
        texture: wgpu::Texture,
        desc: &wgpu::SamplerDescriptor,
    ) -> Self {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
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
            texture,
            view,
            sampler,
        })
    }
}

/// Splits a texture into nine pieces, a 3x3 grid, represented as proportions of the total width and height of the texture.
/// The margins given should all be positive, and the totals of x-direction and y-direction margins should not exceed the total texture size.
#[derive(Debug, Clone)]
pub struct NinePatch {
    pub texture: Asset<Texture>,

    pub texture_width: f32,
    pub texture_height: f32,

    pub left_margin: f32,
    pub right_margin: f32,
    pub top_margin: f32,
    pub bottom_margin: f32,
}

impl NinePatch {
    pub fn no_margins(texture: Asset<Texture>, width: f32, height: f32) -> Self {
        Self {
            texture,
            texture_width: width,
            texture_height: height,
            left_margin: 0.0,
            right_margin: 0.0,
            top_margin: 0.0,
            bottom_margin: 0.0,
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

        // Therefore, we have four x-positions and four y-positions for coordinates,
        // and four u-positions and v-positions for texture coordinates.
        let u_positions = [
            0.0,
            self.left_margin / self.texture_width,
            1.0 - self.right_margin / self.texture_width,
            1.0,
        ];
        let v_positions = [
            0.0,
            self.bottom_margin / self.texture_height,
            1.0 - self.top_margin / self.texture_height,
            1.0,
        ];

        let x_positions = [
            x,
            x + self.left_margin,
            x + width - self.right_margin,
            x + width,
        ];
        let y_positions = [
            y,
            y + self.bottom_margin,
            y + height - self.top_margin,
            y + height,
        ];

        let color = colour.into();

        MultiRenderable::Image {
            texture: self.texture.clone(),
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
