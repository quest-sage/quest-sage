use crate::graphics::{Batch, Renderable, Vertex};
use qs_common::assets::{Asset, OwnedAsset};
use rusttype::{gpu_cache::Cache, point, vector, Font, PositionedGlyph, Rect, Scale};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
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
        let font_texture =
            OwnedAsset::new(crate::graphics::Texture::from_wgpu(&*device, font_texture));

        Self {
            device,
            queue,
            batch,

            scale_factor,

            cache,
            font_texture,
        }
    }

    pub async fn draw_text(
        &mut self,
        text: &RichText,
        frame: &wgpu::SwapChainTexture,
        camera: &crate::graphics::Camera,
    ) {
        let text = &*text.typeset.read().await;
        if let Some(text) = text {
            for RenderableGlyph { font, glyph, ..} in &text.glyphs {
                self.cache.queue_glyph(*font, glyph.clone());
            }

            let queue = &self.queue;
            let cache = &mut self.cache;
            self.font_texture
                .if_loaded(|font_texture| {
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
                })
                .await;

            let mut items = Vec::new();
            for RenderableGlyph { font, colour, glyph } in &text.glyphs {
                if let Some((uv_rect, pixel_rect)) = cache
                    .rect_for(*font, glyph)
                    .expect("Could not load cache entry for glyph")
                {
                    let (x1, y1) = (pixel_rect.min.x as f32, -pixel_rect.min.y as f32);
                    let (x2, y2) = (pixel_rect.max.x as f32, -pixel_rect.max.y as f32);
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

            self.batch
                .render(
                    &*self.device,
                    &*self.queue,
                    frame,
                    &self.font_texture,
                    camera,
                    items.into_iter(),
                )
                .await;
        }
    }
}

static FONT_FACE_ID_COUNTER: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(1);
fn new_font_face_id() -> usize {
    FONT_FACE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A font, together with bold, italic, and bold-italic variants. All variants, except regular, are optional. If a variant is not specified, the next closest variant is used.
/// Specifically, bold and italic fall back to regular, and bold-italic falls back to bold, then italic, then regular.
#[derive(Clone)]
pub struct FontFace {
    /// This is the unique identifier for the font face. This allows the text renderer to produce individual font IDs for combinations of font ID, style and size.
    id: usize,
    /// A (preferably) unique name to distinguish font faces in debug messages.
    name: String,
    regular: Asset<Font<'static>>,
    bold: Option<Asset<Font<'static>>>,
    italic: Option<Asset<Font<'static>>>,
    bold_italic: Option<Asset<Font<'static>>>,
}

impl FontFace {
    pub fn new(
        name: String,
        regular: Asset<Font<'static>>,
        bold: Option<Asset<Font<'static>>>,
        italic: Option<Asset<Font<'static>>>,
        bold_italic: Option<Asset<Font<'static>>>,
    ) -> Self {
        Self {
            id: new_font_face_id(),
            name,
            regular,
            bold,
            italic,
            bold_italic,
        }
    }
}

impl std::fmt::Debug for FontFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontFace")
            .field("name", &self.name)
            .finish()
    }
}

/// A list of prioritised font faces. Towards the start of the list are the most preferred fonts, and the end of the list contains the least preferred fonts.
#[derive(Debug, Clone)]
pub struct FontFamily(Vec<FontFace>);

impl FontFamily {
    pub fn new(list: Vec<FontFace>) -> Self {
        Self(list)
    }
}

/// Represents a single segment of rich text that has the same formatting.
#[derive(Debug, Clone)]
struct RichTextSegment {
    text: String,
    style: RichTextStyle,
}

/// The styling information (font, size, bold, italic, colour) of a span of rich text.
#[derive(Debug, Clone)]
pub struct RichTextStyle {
    font_family: Arc<FontFamily>,
    size: FontSize,
    emphasis: FontEmphasis,
    colour: Colour,
}

impl RichTextStyle {
    pub fn default(font_family: Arc<FontFamily>) -> Self {
        Self {
            font_family,
            size: Default::default(),
            emphasis: Default::default(),
            colour: Default::default(),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Colour {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Colour {
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    pub const CLEAR: Self = Self::rgba(1.0, 1.0, 1.0, 0.0);

    pub const RED: Self = Self::rgb(1.0, 0.0, 0.0);
    pub const GREEN: Self = Self::rgb(0.0, 1.0, 0.0);
    pub const BLUE: Self = Self::rgb(0.0, 0.0, 1.0);
    pub const CYAN: Self = Self::rgb(0.0, 1.0, 1.0);
    pub const MAGENTA: Self = Self::rgb(1.0, 0.0, 1.0);
    pub const YELLOW: Self = Self::rgb(1.0, 1.0, 0.0);
}

impl Default for Colour {
    fn default() -> Self {
        Self {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        }
    }
}

impl From<Colour> for [f32; 4] {
    fn from(colour: Colour) -> [f32; 4] {
        [colour.r, colour.g, colour.b, colour.a]
    }
}

/// An abstract font size, which may be scaled to various sizes according to the user's preferences.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum FontSize {
    /// The largest font size, suitable for top-level headings.
    H1,
    /// A secondary heading font size.
    H2,
    /// A tertiary heading font size.
    H3,
    /// A font size suitable for text in a paragraph.
    Text,
}

impl Default for FontSize {
    fn default() -> Self {
        FontSize::Text
    }
}

/// A font emphasis style. This could be regular, bold, italic or bold and italic.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum FontEmphasis {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl Default for FontEmphasis {
    fn default() -> Self {
        FontEmphasis::Regular
    }
}

type RichTextParagraph = Vec<RichTextSegment>;

/// Represents text that may be styled with colours and other formatting, such as bold and italic letters.
/// The text is assumed to live inside an infinitely tall rectangle of a given maximum width.
/// If this rich text is being used in a label (one line of text), the list of paragraphs should contain only one element.
pub struct RichText {
    /// Represents the content of the rich text. This is broken up into paragraphs which are laid out vertically. Each paragraph
    /// may contain any number of rich text segments, which represent the contiguous indivisible segments of text that have
    /// identical formatting. In particular, rich text segments are typeset individually without regard to the rest
    /// of the paragraph or the text in general. Then, the segments are "glued together" to form the paragraph.
    paragraphs: Arc<RwLock<Vec<RichTextParagraph>>>,

    /// The actual typeset text. Whenever the list of paragraphs is updated, a background task should be spawned to render this text,
    /// thus assigning a value to this variable.
    typeset: Arc<RwLock<Option<TypesetText>>>,

    /// Contains a value in pixels if the rich text object has a maximum width.
    max_width: Option<u32>,
}

impl RichText {
    pub fn new(max_width: Option<u32>) -> Self {
        Self {
            paragraphs: Arc::new(RwLock::new(Vec::new())),
            typeset: Arc::new(RwLock::new(None)),
            max_width,
        }
    }

    pub fn set_text(&self, font_family: Arc<FontFamily>) -> RichTextContentsBuilder {
        RichTextContentsBuilder {
            output: Arc::clone(&self.paragraphs),
            typeset: Arc::clone(&self.typeset),
            style: RichTextStyle::default(font_family),
            paragraphs: Vec::new(),
            current_paragraph: Vec::new(),
            max_width: self.max_width,
            is_internal: false,
        }
    }
}

/// Builds up a rich text object to be put into a `RichText` object. When the builder is finished, the text in the rich text object will be updated.
/// Then, a background task will typeset the text.
#[must_use = "call the finish function to let the builder update the rich text object"]
pub struct RichTextContentsBuilder {
    /// Where should we write the output to once this builder is finished?
    output: Arc<RwLock<Vec<RichTextParagraph>>>,
    /// Where should we output the typeset text to once this builder is finished?
    typeset: Arc<RwLock<Option<TypesetText>>>,

    style: RichTextStyle,
    paragraphs: Vec<RichTextParagraph>,
    current_paragraph: RichTextParagraph,

    /// Contains a value in pixels if the rich text object has a maximum width.
    max_width: Option<u32>,

    /// True if this builder is an "internal" builder, i.e. if it's being used to style some subset of the
    /// text, and isn't the main contents builder. If `finish` is called on an internal builder, it will panic.
    is_internal: bool,
}

impl RichTextContentsBuilder {
    pub fn write(mut self, text: String) -> Self {
        self.current_paragraph.push(RichTextSegment {
            text,
            style: self.style.clone(),
        });
        self
    }

    /// Call this if you want to begin a new paragraph.
    pub fn end_paragraph(mut self) -> Self {
        self.paragraphs.push(self.current_paragraph);
        self.current_paragraph = Vec::new();
        self
    }

    /// Apply the `h1` style to the rich text produced in this function.
    /// Do not call `finish` on this internal builder.
    pub fn h1(self, styled: impl FnOnce(Self) -> Self) -> Self {
        let mut style = self.style.clone();
        style.size = FontSize::H1;
        self.internal(style, styled)
    }

    /// Apply the `bold` style to the rich text produced in this function.
    /// Do not call `finish` on this internal builder.
    pub fn bold(self, styled: impl FnOnce(Self) -> Self) -> Self {
        let mut style = self.style.clone();
        style.emphasis = match style.emphasis {
            FontEmphasis::Regular | FontEmphasis::Bold => FontEmphasis::Bold,
            FontEmphasis::Italic | FontEmphasis::BoldItalic => FontEmphasis::BoldItalic,
        };
        self.internal(style, styled)
    }

    /// Apply the `italic` style to the rich text produced in this function.
    /// Do not call `finish` on this internal builder.
    pub fn italic(self, styled: impl FnOnce(Self) -> Self) -> Self {
        let mut style = self.style.clone();
        style.emphasis = match style.emphasis {
            FontEmphasis::Regular | FontEmphasis::Italic => FontEmphasis::Italic,
            FontEmphasis::Bold | FontEmphasis::BoldItalic => FontEmphasis::BoldItalic,
        };
        self.internal(style, styled)
    }

    /// Apply a colour to the rich text produced in this function.
    /// Do not call `finish` on this internal builder.
    pub fn coloured(self, colour: Colour, styled: impl FnOnce(Self) -> Self) -> Self {
        let mut style = self.style.clone();
        style.colour = colour;
        self.internal(style, styled)
    }

    /// Call the given `styled` function on a new internal builder with the given style,
    /// then append all of its result data to this original builder.
    /// This allows functions to create styles on specific spans of text with ease.
    /// Do not call `finish` on the internal builder provided in the `styled` function.
    fn internal(mut self, style: RichTextStyle, styled: impl FnOnce(Self) -> Self) -> Self {
        let child = Self {
            // The output and typeset fields should never be used because `finish` should never be called on this internal builder.
            output: Arc::clone(&self.output),
            typeset: Arc::clone(&self.typeset),
            style,
            paragraphs: Vec::new(),
            current_paragraph: Vec::new(),
            max_width: self.max_width,
            is_internal: true,
        };
        let mut result = styled(child);
        for mut paragraph in result.paragraphs {
            self.current_paragraph.append(&mut paragraph);
            self = self.end_paragraph()
        }
        self.current_paragraph.append(&mut result.current_paragraph);
        self
    }

    /// Writes the output of this builder to the rich text struct.
    ///
    /// # Panics
    /// If this is an internal builder (e.g. produced by the `h1` function), this will panic.
    pub fn finish(self) {
        if self.is_internal {
            panic!("cannot call `finish` on internal builders");
        }

        let mut paragraphs = self.paragraphs;
        if !self.current_paragraph.is_empty() {
            paragraphs.push(self.current_paragraph);
        }
        let output = self.output;
        let typeset = self.typeset;
        let max_width = self.max_width;
        tokio::spawn(async move {
            // We clone the paragraph data here so that the background thread can't cause the main thread to halt.
            let paragraphs_cloned = paragraphs.clone();
            *output.write().await = paragraphs;
            let typeset_text = typeset_rich_text(paragraphs_cloned, max_width).await;
            // TODO cancel previous task when the text is updated twice in quick succession.
            *typeset.write().await = Some(typeset_text);
        });
    }
}

struct TypesetText {
    /// The area of pixels required to draw this text in the x and y directions.
    /// The pixel glyphs will never extend past this area.
    size: (u32, u32),

    /// A list of glyphs together with their font IDs. New font IDs are created for each font face ID, style and size variant.
    glyphs: Vec<RenderableGlyph>,
}

struct RenderableGlyph {
    font: usize,
    colour: Colour,
    glyph: PositionedGlyph<'static>,
}

#[derive(PartialEq, Eq, Hash)]
struct FontIdSpecifier {
    font_face_id: usize,
    emphasis: FontEmphasis,
    font_size: FontSize,
}

lazy_static::lazy_static! {
    /// Maps font specifiers to the font IDs.
    static ref FONT_ID_MAP: RwLock<HashMap<FontIdSpecifier, usize>> = RwLock::new(HashMap::new());
    /// A many-to-one map, mapping font IDs to the actual font asset.
    static ref FONT_ID_TO_FONT_MAP: RwLock<HashMap<usize, Asset<Font<'static>>>> = RwLock::new(HashMap::new());
}

static FONT_ID_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

async fn get_font_id(font_face: &FontFace, emphasis: FontEmphasis, font_size: FontSize) -> usize {
    let mut font_id_map = FONT_ID_MAP.write().await;
    let mut font_id_to_font_map = FONT_ID_TO_FONT_MAP.write().await;
    let specifier = FontIdSpecifier {
        font_face_id: font_face.id,
        emphasis,
        font_size,
    };
    *(font_id_map.entry(specifier).or_insert_with(|| {
        let id = FONT_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        font_id_to_font_map.insert(
            id,
            match emphasis {
                FontEmphasis::Regular => font_face.regular.clone(),
                FontEmphasis::Bold => font_face
                    .bold
                    .clone()
                    .expect("could not retrieve bold font face variant"),
                FontEmphasis::Italic => font_face
                    .italic
                    .clone()
                    .expect("could not retrieve italic font face variant"),
                FontEmphasis::BoldItalic => font_face
                    .bold_italic
                    .clone()
                    .expect("could not retrieve bold-italic font face variant"),
            },
        );
        id
    }))
}

async fn get_font_for_character(
    font_family: &FontFamily,
    emphasis: FontEmphasis,
    font_size: FontSize,
    c: char,
) -> Option<(usize, rusttype::Glyph<'static>)> {
    for font_face in &font_family.0 {
        if emphasis == FontEmphasis::BoldItalic {
            if let Some(ref font_style) = font_face.bold_italic {
                font_style.wait_until_loaded().await;
                if let Some(data) = font_style.data.upgrade() {
                    if let qs_common::assets::LoadStatus::Loaded(ref font) = &*data.write().await {
                        let glyph = font.glyph(c);
                        if glyph.id().0 != 0 {
                            return Some((
                                get_font_id(font_face, FontEmphasis::BoldItalic, font_size).await,
                                glyph,
                            ));
                        }
                    }
                }
            }
        }

        if emphasis == FontEmphasis::Bold || emphasis == FontEmphasis::BoldItalic {
            if let Some(ref font_style) = font_face.bold {
                font_style.wait_until_loaded().await;
                if let Some(data) = font_style.data.upgrade() {
                    if let qs_common::assets::LoadStatus::Loaded(ref font) = &*data.write().await {
                        let glyph = font.glyph(c);
                        if glyph.id().0 != 0 {
                            return Some((
                                get_font_id(font_face, FontEmphasis::Bold, font_size).await,
                                glyph,
                            ));
                        }
                    }
                }
            }
        }

        if emphasis == FontEmphasis::Italic || emphasis == FontEmphasis::BoldItalic {
            if let Some(ref font_style) = font_face.italic {
                font_style.wait_until_loaded().await;
                if let Some(data) = font_style.data.upgrade() {
                    if let qs_common::assets::LoadStatus::Loaded(ref font) = &*data.write().await {
                        let glyph = font.glyph(c);
                        if glyph.id().0 != 0 {
                            return Some((
                                get_font_id(font_face, FontEmphasis::Italic, font_size).await,
                                glyph,
                            ));
                        }
                    }
                }
            }
        }

        font_face.regular.wait_until_loaded().await;
        if let Some(data) = font_face.regular.data.upgrade() {
            if let qs_common::assets::LoadStatus::Loaded(ref font) = &*data.write().await {
                let glyph = font.glyph(c);
                if glyph.id().0 != 0 {
                    return Some((
                        get_font_id(font_face, FontEmphasis::Regular, font_size).await,
                        glyph,
                    ));
                }
            }
        }
    }

    None
}

async fn typeset_rich_text(
    paragraphs: Vec<RichTextParagraph>,
    max_width: Option<u32>,
) -> TypesetText {
    let scale_factor = 1.0;

    let mut size = (0, 0);
    let mut caret_y = 0.0;
    let mut glyphs = Vec::new();
    for paragraph in paragraphs {
        let mut line = Vec::new();
        // The current X position on the line.
        let mut caret_x = 0.0;
        let mut line_height = 0.0;

        // Contains the last glyph's font ID and glyph ID, if there was a previous glyph on this line.
        let mut last_glyph = None;
        for segment in paragraph {
            let scale = match segment.style.size {
                FontSize::H1 => Scale::uniform(72.0 * scale_factor),
                FontSize::H2 => Scale::uniform(48.0 * scale_factor),
                FontSize::H3 => Scale::uniform(36.0 * scale_factor),
                FontSize::Text => Scale::uniform(24.0 * scale_factor),
            };

            for c in segment.text.chars() {
                let mut font_and_glyph = get_font_for_character(
                    &*segment.style.font_family,
                    segment.style.emphasis,
                    segment.style.size,
                    c,
                )
                .await;

                if let None = font_and_glyph {
                    // Replace this glyph with a generic '?' glyph.
                    font_and_glyph = get_font_for_character(
                        &*segment.style.font_family,
                        segment.style.emphasis,
                        segment.style.size,
                        '\u{FFFD}',
                    )
                    .await;

                    if let None = font_and_glyph {
                        // If even that glyph wasn't in the font, we'll just try a normal question mark.
                        font_and_glyph = get_font_for_character(
                            &*segment.style.font_family,
                            segment.style.emphasis,
                            segment.style.size,
                            '?',
                        )
                        .await;

                        if let None = font_and_glyph {
                            // Really at this point there's no alternatives left.
                            continue;
                        }
                    }
                }

                let (font, base_glyph) =
                    font_and_glyph.expect("no replacement characters found in font");
                if let Some((last_font_id, last_glyph_id)) = last_glyph.take() {
                    if font == last_font_id {
                        let font_id_to_font_map = FONT_ID_TO_FONT_MAP.read().await;
                        let font_asset = font_id_to_font_map
                            .get(&font)
                            .expect("could not retrieve font for font ID");
                        let font_asset_data = font_asset
                            .data
                            .upgrade()
                            .expect("asset manager containing font was dropped");
                        if let qs_common::assets::LoadStatus::Loaded(font_data) =
                            &*font_asset_data.read().await
                        {
                            caret_x +=
                                font_data.pair_kerning(scale, last_glyph_id, base_glyph.id());
                        };
                    }
                }

                last_glyph = Some((font, base_glyph.id()));
                let glyph = base_glyph.scaled(scale).positioned(point(caret_x, caret_y));
                // TODO check line overflow
                /*if let Some(bb) = glyph.pixel_bounding_box() {
                    if bb.max.x > width as i32 {
                        caret = point(0.0, caret.y + advance_height);
                        glyph.set_position(caret);
                        last_glyph_id = None;
                    }
                }*/
                caret_x += glyph.unpositioned().h_metrics().advance_width;
                let v_metrics = glyph.unpositioned().font().v_metrics(scale);
                let glyph_line_height = v_metrics.ascent - v_metrics.descent + v_metrics.line_gap;
                if glyph_line_height > line_height {
                    line_height = glyph_line_height
                }
                line.push(RenderableGlyph {
                    font,
                    colour: segment.style.colour,
                    glyph,
                });
            }
        }

        tracing::info!("Line height {}", line_height);
        caret_y += line_height;
        for RenderableGlyph { glyph, ..} in &mut line {
            glyph.set_position(point(glyph.position().x, glyph.position().y + line_height))
        }
        glyphs.append(&mut line);
    }

    TypesetText { size, glyphs }
}

/// Some text that can be rendered and manipulated.
/// Contains a reference to the fonts that we can use to shape and then render the text.
struct RenderableText {
    font_family: Arc<FontFamily>,
    text: RichText,
}
