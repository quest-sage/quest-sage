//! This module contains implementations of common asset managers used by clients.

use std::sync::Arc;
use wgpu::{Device, Queue};
use tokio::io::AsyncReadExt;
use qs_common::assets::*;
use crate::graphics::Texture;

/// Loads textures from a file.
pub struct TextureAssetLoader {
    device: Arc<Device>,
    queue: Arc<Queue>,
}

impl TextureAssetLoader {
    pub fn new(device: Arc<Device>, queue: Arc<Queue>) -> Self {
        Self {
            device,
            queue,
        }
    }
}

#[async_trait::async_trait]
impl Loader<AssetPath, Texture> for TextureAssetLoader {
    async fn load(&self, key: AssetPath) -> Result<Texture, LoadError> {
        match key.read_file().await {
            Ok(mut reader) => {
                let mut result = Vec::new();
                match reader.read_to_end(&mut result).await {
                    Ok(_) => match Texture::from_bytes(&self.device, &self.queue, &result, "texture") {
                        Ok(texture) => Ok(texture),
                        Err(_) => Err(LoadError::InvalidData),
                    },
                    Err(_) => Err(LoadError::FileNotReadable),
                }
            }
            Err(_) => Err(LoadError::FileNotFound)
        }
    }
}
