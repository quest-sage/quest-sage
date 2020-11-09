use crate::graphics::{Batch, Camera, Renderable, Texture, Vertex};
use qs_common::assets::{Asset, OwnedAsset};
use rusttype::{gpu_cache::Cache, point, Font, PositionedGlyph, Scale};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use stretch::geometry::Size;
use stretch::style::*;
use wgpu::{Device, Queue, SwapChainTexture};

use super::{Colour, UiElement};

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
/// We define a segment to be completely indivisible, so words are often split into many segments.
#[derive(Debug, Clone)]
struct RichTextSegment {
    text: String,
    style: RichTextStyle,
    /// If true, this segment cannot be split up with the previous segment.
    glue_to_previous: bool,
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
            colour: Colour::default(),
        }
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

pub struct RichText(pub Arc<RwLock<RichTextContents>>);

impl RichText {
    pub fn new(max_width: Option<u32>) -> Self {
        Self(Arc::new(RwLock::new(RichTextContents {
            paragraphs: Vec::new(),
            typeset: None,
            current_text_id: 0,
            max_width,
        })))
    }

    pub fn set_text(&mut self, font_family: Arc<FontFamily>) -> RichTextContentsBuilder {
        let mut write = self.0.write().unwrap();
        write.current_text_id += 1;
        RichTextContentsBuilder {
            output: Self(Arc::clone(&self.0)),
            style: RichTextStyle::default(font_family),
            paragraphs: Vec::new(),
            current_paragraph: Vec::new(),
            max_width: write.max_width,
            is_internal: false,
            text_id: write.current_text_id,
        }
    }
}

#[async_trait::async_trait]
impl UiElement for RichText {
    async fn get_size(&self) -> Size<Dimension> {
        let read = self.0.read().unwrap();
        if let Some(value) = &read.typeset {
            Size {
                width: Dimension::Points(value.size.0 as f32),
                height: Dimension::Points(value.size.1 as f32),
            }
        } else {
            Size {
                width: Dimension::Points(0.0),
                height: Dimension::Points(0.0),
            }
        }
    }
}

/// Represents text that may be styled with colours and other formatting, such as bold and italic letters.
/// The text is assumed to live inside an infinitely tall rectangle of a given maximum width.
/// If this rich text is being used in a label (one line of text), the list of paragraphs should contain only one element.
pub struct RichTextContents {
    /// Represents the content of the rich text. This is broken up into paragraphs which are laid out vertically. Each paragraph
    /// may contain any number of rich text segments, which represent the contiguous indivisible segments of text that have
    /// identical formatting. In particular, rich text segments are typeset individually without regard to the rest
    /// of the paragraph or the text in general. Then, the segments are "glued together" to form the paragraph.
    paragraphs: Vec<RichTextParagraph>,

    /// The actual typeset text. Whenever the list of paragraphs is updated, a background task should be spawned to render this text,
    /// thus assigning a value to this variable.
    pub typeset: Option<TypesetText>,

    /// This is a counter that tracks how many times this text object has been updated. Every time `finish` is called on a builder
    /// to set the text contents, this is incremented. Background tasks typesetting this new text will only update
    /// the `typeset` variable if their `text_id` matches this `current_text_id`. This ensures that when we update text twice quickly,
    /// the first task is essentially cancelled.
    current_text_id: u64,

    /// Contains a value in pixels if the rich text object has a maximum width.
    max_width: Option<u32>,
}

impl RichTextContents {
    fn write(&mut self, text_id: u64, paragraphs: Vec<RichTextParagraph>, typeset: TypesetText) {
        if self.current_text_id == text_id {
            self.paragraphs = paragraphs;
            self.typeset = Some(typeset);
        }
    }
}

/// Builds up a rich text object to be put into a `RichText` object. When the builder is finished, the text in the rich text object will be updated.
/// Then, a background task will typeset the text.
#[must_use = "call the finish function to let the builder update the rich text object"]
pub struct RichTextContentsBuilder {
    /// Where should we write the output to once this builder is finished?
    output: RichText,

    style: RichTextStyle,
    paragraphs: Vec<RichTextParagraph>,
    current_paragraph: RichTextParagraph,

    /// Contains a value in pixels if the rich text object has a maximum width.
    max_width: Option<u32>,

    /// True if this builder is an "internal" builder, i.e. if it's being used to style some subset of the
    /// text, and isn't the main contents builder. If `finish` is called on an internal builder, it will panic.
    is_internal: bool,

    /// The ID of the text we're typesetting. If this does not match the value contained within the text object, we won't produce any output.
    /// This can happen when we set the text again while we're still typesetting the old value. The typesetting task
    /// we're currently doing is therefore pointless, so it can be cancelled.
    text_id: u64,
}

impl RichTextContentsBuilder {
    /// Write some text into this rich text object.
    /// This function copies the input text, splitting it by whitespace, which is consumed.
    pub fn write(self, text: &str) -> Self {
        self.write_maybe_glued(text, false)
    }

    /// Write some text into this rich text object, without
    /// inserting a space after the previous call to `write`.
    pub fn write_glued(self, text: &str) -> Self {
        self.write_maybe_glued(text, true)
    }

    /// Writes some text which might be glued to the previous text or not, depending
    /// on the `glue_to_previous` argument.
    pub fn write_maybe_glued(mut self, text: &str, mut glue_to_previous: bool) -> Self {
        let chars = text.chars().collect::<Vec<_>>(); // TODO could optimise this, we only really need two chars at a time
        let mut word_start_index = 0;
        for i in 1..chars.len() {
            if self.should_split_between(chars[i - 1], chars[i]) {
                self.current_paragraph.push(RichTextSegment {
                    text: chars[word_start_index..i].iter().copied().collect(),
                    style: self.style.clone(),
                    glue_to_previous,
                });
                word_start_index = i;
                glue_to_previous = false;
            }
        }
        self.current_paragraph.push(RichTextSegment {
            text: chars[word_start_index..].iter().copied().collect(),
            style: self.style.clone(),
            glue_to_previous,
        });
        self
    }

    fn should_split_between(&self, left: char, right: char) -> bool {
        left.is_whitespace() && !right.is_whitespace()
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
            // The output field should never be used because `finish` should never be called on this internal builder.
            output: RichText(Arc::clone(&self.output.0)),
            style,
            paragraphs: Vec::new(),
            current_paragraph: Vec::new(),
            max_width: self.max_width,
            is_internal: true,
            text_id: self.text_id,
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
        let max_width = self.max_width;
        let text_id = self.text_id;
        tokio::spawn(async move {
            // We clone the paragraph data here so that the background thread can't cause the main thread to halt.
            let paragraphs_cloned = paragraphs.clone();
            let typeset_text = typeset_rich_text(paragraphs_cloned, max_width).await;

            let mut rich_text = output.0.write().unwrap();
            rich_text.write(text_id, paragraphs, typeset_text);
        });
    }
}

pub struct TypesetText {
    /// The area of pixels required to draw this text in the x and y directions.
    /// The pixel glyphs will never extend past this area.
    size: (u32, u32),

    /// A list of glyphs together with their font IDs. New font IDs are created for each font face ID, style and size variant.
    glyphs: Vec<RenderableGlyph>,

    /// When we try to render this text, we need to convert it to a list of renderables.
    /// However, this is quite expensive, so we cache the result here.
    cached_renderables: Option<Vec<Renderable>>,
    /// What cache generation was the `cached_renderables` variable built for? If this does not match the `cache_generation` in
    /// the `TextRenderer`, we will have to recalculate the cached renderables list.
    cache_generation: u64,
}

impl TypesetText {
    /// Renders the given text using the provided cache, batch etc.
    /// Returns the new cache generation, if a new one was created.
    pub async fn render(
        &mut self,
        mut profiler: qs_common::profile::ProfileSegmentGuard<'_>,
        device: &Device,
        queue: &Queue,
        frame: &SwapChainTexture,
        cache: &mut Cache<'_>,
        batch: &mut Batch,
        font_texture: &OwnedAsset<Texture>,
        camera: &Camera,
        mut cache_generation: u64,
    ) -> u64 {
        {
            let _guard = profiler.task("queuing glyphs").time();
            for RenderableGlyph { font, glyph, .. } in &self.glyphs {
                cache.queue_glyph(*font, glyph.clone());
            }
        }

        {
            let _guard = profiler.task("caching glyphs").time();
            font_texture
                .if_loaded(|font_texture| {
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
                        cache_generation += 1;
                    }
                })
                .await;
        }

        let mut items = Vec::new();
        {
            let _guard = profiler.task("creating texture coordinates").time();
            if self.cache_generation == cache_generation && self.cached_renderables.is_some() {
                items = self.cached_renderables.as_ref().unwrap().clone();
            } else {
                for RenderableGlyph {
                    font,
                    colour,
                    glyph,
                } in &self.glyphs
                {
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

                self.cache_generation = cache_generation;
                self.cached_renderables = Some(items.clone());
            }
        }

        {
            let _guard = profiler.task("rendering text").time();
            batch
                .render(
                    &*device,
                    &*queue,
                    frame,
                    &font_texture,
                    camera,
                    items.into_iter(),
                )
                .await;
        }

        cache_generation
    }
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
    static ref FONT_ID_MAP: tokio::sync::RwLock<HashMap<FontIdSpecifier, usize>> = tokio::sync::RwLock::new(HashMap::new());
    /// A many-to-one map, mapping font IDs to the actual font asset.
    static ref FONT_ID_TO_FONT_MAP: tokio::sync::RwLock<HashMap<usize, Asset<Font<'static>>>> = tokio::sync::RwLock::new(HashMap::new());
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

    let mut largest_line_width = 0.0;
    let mut caret_y = 0.0;
    let mut glyphs = Vec::new();
    for paragraph in paragraphs {
        let mut segments: &[RichTextSegment] = &paragraph;
        while !segments.is_empty() {
            let mut line_result = typeset_rich_text_line(segments, max_width, scale_factor).await;
            tracing::trace!(
                "Rendering line of {} segments: {} excess",
                segments.len(),
                line_result.excess.len()
            );
            segments = line_result.excess;

            caret_y += line_result.line_height;
            if line_result.line_width > largest_line_width {
                largest_line_width = line_result.line_width;
            }
            for RenderableGlyph { glyph, .. } in &mut line_result.line {
                glyph.set_position(point(glyph.position().x, caret_y))
            }
            glyphs.append(&mut line_result.line);
        }
    }

    TypesetText {
        size: (largest_line_width as u32, caret_y as u32),
        glyphs,
        cached_renderables: None,
        cache_generation: 0,
    }
}

struct RichTextLineTypesetResult<'a> {
    line: Vec<RenderableGlyph>,
    line_width: f32,
    line_height: f32,
    excess: &'a [RichTextSegment],
}

/// Typeset a single line. Assumes that the Y coordinate of each character is zero.
async fn typeset_rich_text_line(
    paragraph: &[RichTextSegment],
    max_width: Option<u32>,
    scale_factor: f32,
) -> RichTextLineTypesetResult<'_> {
    // The current line, which is filled with glyphs.
    let mut line = Vec::new();
    // The current word, defined as a sequence of whitespace characters followed by one or more non-whitespace characters.
    let mut word = Vec::new();
    // The segment index of the start of the current word. This is where we backtrack to if a word could not be added to this line.
    let mut word_start_index = 0;

    // The current X position on the line.
    let mut caret_x = 0.0;
    let mut line_height = 0.0;

    // Contains the last glyph's font ID and glyph ID, if there was a previous glyph on this line.
    let mut last_glyph = None;

    let mut segment_index = 0;
    while segment_index < paragraph.len() {
        let segment = &paragraph[segment_index];

        let scale = match segment.style.size {
            FontSize::H1 => Scale::uniform(72.0 * scale_factor),
            FontSize::H2 => Scale::uniform(48.0 * scale_factor),
            FontSize::H3 => Scale::uniform(36.0 * scale_factor),
            FontSize::Text => Scale::uniform(24.0 * scale_factor),
        };

        if !segment.glue_to_previous {
            // Add the previous word to the line, as we now know it completely fits.
            line.append(&mut word);
            word_start_index = segment_index;
        }

        for c in segment.text.chars() {
            let mut font_and_glyph = get_font_for_character(
                &*segment.style.font_family,
                segment.style.emphasis,
                segment.style.size,
                c,
            )
            .await;

            if font_and_glyph.is_none() {
                // Replace this glyph with a generic 'character not found' glyph.
                font_and_glyph = get_font_for_character(
                    &*segment.style.font_family,
                    segment.style.emphasis,
                    segment.style.size,
                    '\u{FFFD}',
                )
                .await;

                if font_and_glyph.is_none() {
                    // If that glyph wasn't in the font, we'll just try a normal question mark.
                    font_and_glyph = get_font_for_character(
                        &*segment.style.font_family,
                        segment.style.emphasis,
                        segment.style.size,
                        '?',
                    )
                    .await;

                    if font_and_glyph.is_none() {
                        // Really at this point there's no alternatives left.
                        // We'll just not render this character.
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
                        caret_x += font_data.pair_kerning(scale, last_glyph_id, base_glyph.id());
                    };
                }
            }

            last_glyph = Some((font, base_glyph.id()));
            let glyph = base_glyph.scaled(scale).positioned(point(caret_x, 0.0));

            if let Some(max_width) = max_width {
                if let Some(bb) = glyph.pixel_bounding_box() {
                    if bb.max.x >= max_width as i32 {
                        // We've exceeded the width of the line.
                        // So, we won't submit the current word, and we'll just return here.
                        return RichTextLineTypesetResult {
                            line,
                            line_height,
                            line_width: caret_x,
                            excess: &paragraph[word_start_index..],
                        };
                    }
                }
            }

            caret_x += glyph.unpositioned().h_metrics().advance_width;
            let v_metrics = glyph.unpositioned().font().v_metrics(scale);
            let glyph_line_height = v_metrics.ascent - v_metrics.descent + v_metrics.line_gap;
            if glyph_line_height > line_height {
                line_height = glyph_line_height
            }
            word.push(RenderableGlyph {
                font,
                colour: segment.style.colour,
                glyph,
            });
        }

        segment_index += 1;
    }

    // Add the current word to the line.
    line.append(&mut word);
    RichTextLineTypesetResult {
        line,
        line_height,
        line_width: caret_x,
        excess: &[],
    }
}
