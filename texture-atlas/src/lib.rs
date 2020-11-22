use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents some metadata about sprites packed into a single image, which can be referred to by their (relative) file names.
#[derive(Serialize, Deserialize)]
pub struct TextureAtlas {
    /// The width of the backing texture.
    pub width: u32,
    /// The height of the backing texture.
    pub height: u32,

    /// The individual texture regions, addressable by file names.
    pub frames: HashMap<String, TextureRegionInformation>,
}

/// Roughly corresponds to [texture_packer::Frame].
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct TextureRegionInformation {
    /// Rectangle describing the texture coordinates and size.
    pub frame: Rect,
    /// True if the texture was rotated during packing.
    /// If it was rotated, it was rotated 90 degrees clockwise.
    pub rotated: bool,
    /// True if the texture was trimmed during packing.
    pub trimmed: bool,

    // (x, y) is the trimmed frame position at original image
    // (w, h) is original image size
    //
    //            w
    //     +--------------+
    //     | (x, y)       |
    //     |  ^           |
    //     |  |           |
    //     |  *********   |
    //     |  *       *   |  h
    //     |  *       *   |
    //     |  *********   |
    //     |              |
    //     +--------------+
    /// Source texture size before any trimming.
    pub source: Rect,
}

/// Copied from the `texture_packer` crate.
/// Defines a rectangle in pixels with the origin at the top-left of the texture atlas.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct Rect {
    /// Horizontal position the rectangle begins at.
    pub x: u32,
    /// Vertical position the rectangle begins at.
    pub y: u32,
    /// Width of the rectangle.
    pub w: u32,
    /// Height of the rectangle.
    pub h: u32,
}
