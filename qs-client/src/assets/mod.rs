//! This module contains implementations of common asset managers used by clients.

use crate::graphics::Texture;
use qs_common::assets::*;
use rusttype::Font;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use wgpu::{Device, Queue};

/// Loads textures from a file.
pub struct TextureAssetLoader {
    device: Arc<Device>,
    queue: Arc<Queue>,
}

impl TextureAssetLoader {
    pub fn new(device: Arc<Device>, queue: Arc<Queue>) -> Self {
        Self { device, queue }
    }
}

#[async_trait::async_trait]
impl Loader<AssetPath, Texture> for TextureAssetLoader {
    async fn load(&self, key: AssetPath) -> Result<Texture, LoadError> {
        match key.read_file().await {
            Ok(mut reader) => {
                let mut result = Vec::new();
                match reader.read_to_end(&mut result).await {
                    Ok(_) => {
                        match Texture::from_bytes(&self.device, &self.queue, &result, "texture") {
                            Ok(texture) => Ok(texture),
                            Err(_) => Err(LoadError::InvalidData),
                        }
                    }
                    Err(_) => Err(LoadError::FileNotReadable),
                }
            }
            Err(_) => Err(LoadError::FileNotFound),
        }
    }
}

/// Loads fonts from a file.
pub struct FontAssetLoader {}

impl FontAssetLoader {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait::async_trait]
impl Loader<AssetPath, Font<'static>> for FontAssetLoader {
    /// The asset should be a `.ttf` file, not an `.otf` file. This increases
    /// compatibility with the `rusttype` libary that we use to load fonts.
    async fn load(&self, key: AssetPath) -> Result<Font<'static>, LoadError> {
        match key.read_file().await {
            Ok(mut reader) => {
                let mut result = Vec::new();
                match reader.read_to_end(&mut result).await {
                    Ok(_) => match Font::try_from_vec(result) {
                        Some(font) => Ok(font),
                        None => Err(LoadError::InvalidData),
                    },
                    Err(_) => Err(LoadError::FileNotReadable),
                }
            }
            Err(_) => Err(LoadError::FileNotFound),
        }
    }
}
