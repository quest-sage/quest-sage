use crate::graphics::{MultiRenderable, Renderable};
use qs_common::assets::Asset;
use rusttype::{point, Font, PositionedGlyph, Scale};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use stretch::geometry::Size;
use stretch::style::*;
use tokio::task::JoinHandle;

use super::{Colour, UiElement, Widget};

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

/// You may clone this rich text object to get another view of it which can be safely passed between threads.
#[derive(Clone)]
pub struct RichText(pub Arc<RwLock<RichTextContents>>);

impl RichText {
    pub fn new(style: Style) -> Self {
        // This root widget contains paragraphs. The paragraphs contain words.
        let widget = Widget::new(
            RichTextWidgetContainer,
            Vec::new(),
            Vec::new(),
            Style {
                flex_direction: FlexDirection::Column,
                ..style
            },
        );
        Self(Arc::new(RwLock::new(RichTextContents {
            paragraphs: Vec::new(),
            current_text_id: 0,
            widget,
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
            is_internal: false,
            text_id: write.current_text_id,
        }
    }
}

/// This struct is essentially a box into which we can put RenderableWord objects.
struct RichTextWidgetContainer;
impl UiElement for RichTextWidgetContainer {
    fn get_size(&self) -> Size<Dimension> {
        Size {
            width: Dimension::Auto,
            height: Dimension::Auto,
        }
    }

    fn generate_render_info(&self, _layout: &stretch::result::Layout) -> MultiRenderable {
        // The rich text object itself doesn't render anything. It's just the RenderableWord children that render stuff.
        MultiRenderable::Nothing
    }
}

impl UiElement for RenderableWord {
    fn get_size(&self) -> Size<Dimension> {
        Size {
            width: Dimension::Points(self.size.0 as f32),
            height: Dimension::Points(self.size.1 as f32),
        }
    }

    fn generate_render_info(&self, layout: &stretch::result::Layout) -> MultiRenderable {
        MultiRenderable::Text {
            word: self.clone(),
            offset: layout.location,
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

    /// The widget representing the actual typeset rich text.
    /// The root element is a RichTextWidgetContainer, containing some RenderableWord children.
    /// Whenever the list of paragraphs is updated, a background task is spawned to render this text.
    pub widget: Widget,

    /// This is a counter that tracks how many times this text object has been updated. Every time `finish` is called on a builder
    /// to set the text contents, this is incremented. Background tasks typesetting this new text will only update
    /// the `typeset` variable if their `text_id` matches this `current_text_id`. This ensures that when we update text twice quickly,
    /// the first task is essentially cancelled.
    current_text_id: u64,
}

impl RichTextContents {
    fn write(&mut self, text_id: u64, paragraphs: Vec<RichTextParagraph>, typeset: TypesetText) {
        if self.current_text_id == text_id {
            self.paragraphs = paragraphs;
            // TODO invalidate hierarchy, force re-layout
            let cloned = self.widget.clone();
            tokio::task::spawn(async move {
                // Construct the widget hierarchy.
                let mut write = cloned.0.write().await;
                write.children = typeset
                    .paragraphs
                    .into_iter()
                    .map(|paragraph| {
                        let words: Vec<_> = paragraph
                            .0
                            .into_iter()
                            .map(|word| {
                                Widget::new(word, Vec::new(), Vec::new(), Default::default())
                            })
                            .collect();
                        Widget::new(
                            RichTextWidgetContainer,
                            words,
                            Vec::new(),
                            Style {
                                flex_wrap: FlexWrap::Wrap,
                                align_items: AlignItems::FlexEnd,
                                ..Default::default()
                            },
                        )
                    })
                    .collect();
            });
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

    /// Apply the `h2` style to the rich text produced in this function.
    /// Do not call `finish` on this internal builder.
    pub fn h2(self, styled: impl FnOnce(Self) -> Self) -> Self {
        let mut style = self.style.clone();
        style.size = FontSize::H2;
        self.internal(style, styled)
    }

    /// Apply the `h3` style to the rich text produced in this function.
    /// Do not call `finish` on this internal builder.
    pub fn h3(self, styled: impl FnOnce(Self) -> Self) -> Self {
        let mut style = self.style.clone();
        style.size = FontSize::H3;
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
    pub fn finish(self) -> JoinHandle<()> {
        if self.is_internal {
            panic!("cannot call `finish` on internal builders");
        }

        let mut paragraphs = self.paragraphs;
        if !self.current_paragraph.is_empty() {
            paragraphs.push(self.current_paragraph);
        }
        let output = self.output;
        let text_id = self.text_id;
        tokio::spawn(async move {
            // We clone the paragraph data here so that the background thread can't cause the main thread to halt.
            let paragraphs_cloned = paragraphs.clone();
            let typeset_text = typeset_rich_text(paragraphs_cloned).await;

            let mut rich_text = output.0.write().unwrap();
            rich_text.write(text_id, paragraphs, typeset_text);
        })
    }
}

pub struct TypesetText {
    /// A list of words, containing glyphs together with their font IDs. New font IDs are created for each font face ID, style and size variant.
    /// Each word is assumed to start at position (0, 0). The actual positions of each word are determined by the container the text is placed in.
    /// Therefore, this object has no control over the actual width or height of the typeset text when it is rendered; this responsibility is
    /// delegated to the UI manager.
    paragraphs: Vec<RenderableParagraph>,
}

#[derive(Debug, Clone)]
pub struct RenderableGlyph {
    pub font: usize,
    pub colour: Colour,
    pub glyph: PositionedGlyph<'static>,
}

/// An indivisible unit of text, represented as a list of glyphs positioned relative to the word's origin point.
#[derive(Debug, Clone)]
pub struct RenderableWord {
    pub glyphs: Vec<RenderableGlyph>,
    pub size: (u32, u32),

    /// When we try to render this text, we need to convert it to a list of renderables.
    /// However, this is quite expensive, so we cache the result here.
    /// TODO actually make this cache work
    cached_renderables: Option<Vec<Renderable>>,

    /// What cache generation was the `cached_renderables` variable built for? If this does not match the `cache_generation` in
    /// the `TextRenderer`, we will have to recalculate the cached renderables list.
    cache_generation: u64,
}

/// An paragraph of text comprised of a number of words.
pub struct RenderableParagraph(pub Vec<RenderableWord>);

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

async fn typeset_rich_text(paragraphs: Vec<RichTextParagraph>) -> TypesetText {
    let scale_factor = 1.0;

    let mut renderable_paragraphs = Vec::new();
    for paragraph in paragraphs {
        let line_result = typeset_rich_text_paragraph(paragraph, scale_factor).await;
        renderable_paragraphs.push(line_result);
    }

    TypesetText {
        paragraphs: renderable_paragraphs,
    }
}

/// Typeset a single paragraph. Assumes that the Y coordinate of each character is zero.
async fn typeset_rich_text_paragraph(
    paragraph: Vec<RichTextSegment>,
    scale_factor: f32,
) -> RenderableParagraph {
    // The current paragraph, which is filled with words.
    let mut output = Vec::new();
    // The current word, defined as a sequence of whitespace characters followed by one or more non-whitespace characters.
    let mut word = Vec::new();

    // The current X position on the word.
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

        if !segment.glue_to_previous {
            // Add the previous word to the paragraph.
            output.push(RenderableWord {
                glyphs: std::mem::take(&mut word),
                size: (caret_x as u32, line_height as u32),
                cached_renderables: None,
                cache_generation: 0,
            });
            caret_x = 0.0;
            line_height = 0.0;
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
    }

    // Add the current word to the line.
    output.push(RenderableWord {
        glyphs: std::mem::take(&mut word),
        size: (caret_x as u32, line_height as u32),
        cached_renderables: None,
        cache_generation: 0,
    });

    RenderableParagraph(output)
}
