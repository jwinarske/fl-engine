//! Text and paragraph styles: what `ParagraphBuilder` is given, decoded from
//! dart:ui's encodings (text.dart `_encodeTextStyle`, `_encodeParagraphStyle`,
//! `_encodeStrut`) as the engine decodes them (paragraph_builder.cc), with
//! txt's defaults (text_style.h, paragraph_style.h).

/// A paint the caller keeps, for a foreground or background: fl-text only
/// carries its id to the glyph runs it paints.
pub type PaintId = usize;

/// `FontWeight` and `FontStyle`, as `SkFontStyle` holds them (width is
/// always normal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontStyle {
    /// 100 to 900.
    pub weight: u16,
    pub italic: bool,
}

impl Default for FontStyle {
    fn default() -> Self {
        FontStyle {
            weight: 400,
            italic: false,
        }
    }
}

/// `TextDecoration`'s mask.
pub mod decoration {
    pub const UNDERLINE: u32 = 0x1;
    pub const OVERLINE: u32 = 0x2;
    pub const LINE_THROUGH: u32 = 0x4;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DecorationStyle {
    #[default]
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}

/// `TextAlign`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    Left,
    Right,
    Center,
    Justify,
    #[default]
    Start,
    End,
}

/// `TextDirection`: dart:ui's index, 0 for rtl.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextDirection {
    Rtl,
    #[default]
    Ltr,
}

/// `TextBaseline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextBaseline {
    #[default]
    Alphabetic,
    Ideographic,
}

/// `TextHeightBehavior`'s bits, as `_encode` packs them.
pub mod height_behavior {
    pub const DISABLE_FIRST_ASCENT: i32 = 0x1;
    pub const DISABLE_LAST_DESCENT: i32 = 0x2;
}

/// A text shadow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    /// `0xAARRGGBB`.
    pub color: u32,
    pub offset: [f32; 2],
    pub blur_sigma: f64,
}

/// One run's style. Only what layout reads, and what painting carries.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// `0xAARRGGBB`.
    pub color: u32,
    pub foreground: Option<PaintId>,
    pub background: Option<PaintId>,
    pub decoration: u32,
    pub decoration_color: u32,
    pub decoration_style: DecorationStyle,
    pub decoration_thickness: f64,
    pub font_style: FontStyle,
    pub text_baseline: TextBaseline,
    pub font_families: Vec<String>,
    pub font_size: f32,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    /// The height multiplier, when [`TextStyle::height_override`].
    pub height: f32,
    pub height_override: bool,
    pub half_leading: bool,
    pub locale: String,
    pub shadows: Vec<Shadow>,
    /// OpenType features, tag and value.
    pub font_features: Vec<([u8; 4], i32)>,
    /// Variation axis values, tag and value. `wght` is the weight unless set.
    pub font_variations: Vec<([u8; 4], f32)>,
    /// The style of a placeholder's object-replacement character.
    pub placeholder: bool,
}

impl Default for TextStyle {
    /// txt's `TextStyle`: white, 14, weight 400. dart:ui always sends a
    /// color, so white is only what nothing set.
    fn default() -> Self {
        TextStyle {
            color: 0xFFFF_FFFF,
            foreground: None,
            background: None,
            decoration: 0,
            decoration_color: 0,
            decoration_style: DecorationStyle::Solid,
            decoration_thickness: 1.0,
            font_style: FontStyle::default(),
            text_baseline: TextBaseline::Alphabetic,
            font_families: Vec::new(),
            font_size: 14.0,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            height: 1.0,
            height_override: false,
            half_leading: false,
            locale: String::new(),
            shadows: Vec::new(),
            font_features: Vec::new(),
            font_variations: Vec::new(),
            placeholder: false,
        }
    }
}

impl TextStyle {
    /// SkParagraph's `matchOneAttribute(kFont)`: whether two blocks shape
    /// alike.
    pub(crate) fn same_font(&self, other: &TextStyle) -> bool {
        self.font_style == other.font_style
            && self.locale == other.locale
            && self.font_families == other.font_families
            && self.font_size == other.font_size
            && self.height_multiplier() == other.height_multiplier()
            && self.half_leading == other.half_leading
            && self.font_variations == other.font_variations
    }

    /// What a run's metrics are multiplied to: the height when overridden,
    /// else 0, which leaves the font's own (OneLineShaper `fHeight`).
    pub(crate) fn height_multiplier(&self) -> f32 {
        if self.height_override {
            self.height
        } else {
            0.0
        }
    }

    /// The variation coordinates the face is set to: the style's, with
    /// `wght` at the weight unless the style set it (paragraph_builder_skia.cc).
    pub(crate) fn variations(&self) -> Vec<([u8; 4], f32)> {
        let mut v = self.font_variations.clone();
        if !v.iter().any(|(t, _)| t == b"wght") {
            v.push((*b"wght", f32::from(self.font_style.weight)));
        }
        v
    }

    /// `cloneForPlaceholder`.
    pub(crate) fn for_placeholder(&self) -> TextStyle {
        TextStyle {
            placeholder: true,
            ..self.clone()
        }
    }
}

/// `StrutStyle`.
#[derive(Debug, Clone, PartialEq)]
pub struct StrutStyle {
    pub enabled: bool,
    pub font_families: Vec<String>,
    pub font_style: FontStyle,
    pub font_size: f32,
    pub height: f32,
    pub height_override: bool,
    pub half_leading: bool,
    /// Negative for the font's own leading.
    pub leading: f32,
    pub force_height: bool,
}

impl Default for StrutStyle {
    fn default() -> Self {
        StrutStyle {
            enabled: false,
            font_families: Vec::new(),
            font_style: FontStyle::default(),
            font_size: 14.0,
            height: 1.0,
            height_override: false,
            half_leading: false,
            leading: -1.0,
            force_height: false,
        }
    }
}

/// `ParagraphStyle`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphStyle {
    /// The default style: the base of the builder's stack.
    pub text_style: TextStyle,
    pub strut: StrutStyle,
    pub text_align: TextAlign,
    pub text_direction: TextDirection,
    /// `usize::MAX` for unlimited.
    pub max_lines: usize,
    /// UTF-8; empty for none.
    pub ellipsis: String,
    pub text_height_behavior: i32,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        let text_style = TextStyle {
            // The paragraph's root style takes txt's default paint color
            // (paragraph_builder_skia.cc: a DlPaint of TextStyle's color).
            ..TextStyle::default()
        };
        ParagraphStyle {
            text_style,
            strut: StrutStyle::default(),
            text_align: TextAlign::Start,
            text_direction: TextDirection::Ltr,
            max_lines: usize::MAX,
            ellipsis: String::new(),
            text_height_behavior: 0,
        }
    }
}

impl ParagraphStyle {
    /// `effective_align`: start and end resolved by direction.
    pub(crate) fn effective_align(&self) -> TextAlign {
        let rtl = self.text_direction == TextDirection::Rtl;
        match self.text_align {
            TextAlign::Start => {
                if rtl {
                    TextAlign::Right
                } else {
                    TextAlign::Left
                }
            }
            TextAlign::End => {
                if rtl {
                    TextAlign::Left
                } else {
                    TextAlign::Right
                }
            }
            a => a,
        }
    }

    pub(crate) fn unlimited_lines(&self) -> bool {
        self.max_lines == usize::MAX
    }

    pub(crate) fn ellipsized(&self) -> bool {
        !self.ellipsis.is_empty()
    }

    pub(crate) fn strut_force_height(&self) -> bool {
        self.strut.enabled && self.strut.force_height
    }
}

/// `PlaceholderAlignment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaceholderAlignment {
    #[default]
    Baseline,
    AboveBaseline,
    BelowBaseline,
    Top,
    Bottom,
    Middle,
}

/// `PlaceholderStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlaceholderStyle {
    pub width: f32,
    pub height: f32,
    pub alignment: PlaceholderAlignment,
    pub baseline: TextBaseline,
    pub baseline_offset: f32,
}

/// What `ParagraphBuilder::Create` is given, beside the encoded ints.
#[derive(Debug, Clone, Default)]
pub struct ParagraphArgs {
    pub font_family: String,
    pub strut_font_families: Vec<String>,
    pub font_size: f64,
    pub height: f64,
    pub ellipsis: String,
    pub locale: String,
}

/// What `ParagraphBuilder::pushStyle` is given, beside the encoded ints.
#[derive(Debug, Clone, Default)]
pub struct TextStyleArgs {
    pub font_families: Vec<String>,
    pub font_size: f64,
    pub letter_spacing: f64,
    pub word_spacing: f64,
    pub height: f64,
    pub decoration_thickness: f64,
    pub locale: String,
    pub background: Option<PaintId>,
    pub foreground: Option<PaintId>,
    /// 16 bytes each: color, x, y, blur.
    pub shadows: Vec<u8>,
    /// 8 bytes each: tag, value.
    pub font_features: Vec<u8>,
    /// 8 bytes each: tag, value.
    pub font_variations: Vec<u8>,
}

/// The engine's `kTextHeightNone`.
const TEXT_HEIGHT_NONE: f64 = 0.0;

fn bit(mask: i32, index: u32) -> bool {
    mask & (1 << index) != 0
}

fn font_style_index(i: i32) -> bool {
    i == 1
}

fn u32_le(b: &[u8]) -> u32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[..4]);
    u32::from_le_bytes(a)
}

impl ParagraphStyle {
    /// `ParagraphBuilder::ParagraphBuilder`: `encoded` is `Int32List(7)`,
    /// `strut` the strut's `ByteData` (empty for none).
    pub fn decode(encoded: &[i32], strut: &[u8], args: &ParagraphArgs) -> ParagraphStyle {
        let at = |i: usize| encoded.get(i).copied().unwrap_or(0);
        let mask = at(0);
        let mut s = ParagraphStyle::default();
        if bit(mask, 1) {
            s.text_align = match at(1) {
                0 => TextAlign::Left,
                1 => TextAlign::Right,
                2 => TextAlign::Center,
                3 => TextAlign::Justify,
                5 => TextAlign::End,
                _ => TextAlign::Start,
            };
        }
        if bit(mask, 2) {
            s.text_direction = if at(2) == 0 {
                TextDirection::Rtl
            } else {
                TextDirection::Ltr
            };
        }
        if bit(mask, 3) {
            s.text_style.font_style.weight = at(3).clamp(1, 1000) as u16;
        }
        if bit(mask, 4) {
            s.text_style.font_style.italic = font_style_index(at(4));
        }
        if bit(mask, 7) {
            s.text_style.font_families = vec![args.font_family.clone()];
        } else {
            // txt's ParagraphStyle font_family "" becomes the one family "".
            s.text_style.font_families = vec![String::new()];
        }
        if bit(mask, 8) {
            s.text_style.font_size = args.font_size as f32;
        }
        if bit(mask, 9) {
            s.text_style.height = args.height as f32;
            s.text_style.height_override = true;
        }
        if bit(mask, 6) {
            s.text_height_behavior = at(6);
        }
        if bit(mask, 5) {
            s.max_lines = usize::try_from(at(5)).unwrap_or(usize::MAX);
        }
        if bit(mask, 10) {
            s.strut = decode_strut(strut, &args.strut_font_families);
        }
        if bit(mask, 11) {
            s.ellipsis = args.ellipsis.clone();
        }
        if bit(mask, 12) {
            s.text_style.locale = args.locale.clone();
        }
        s
    }
}

/// `decodeStrut`.
fn decode_strut(data: &[u8], families: &[String]) -> StrutStyle {
    let mut s = StrutStyle::default();
    if data.len() < 4 {
        return s;
    }
    s.enabled = true;
    let ints: Vec<i32> = data
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let mask = ints[0];
    let mut n = 1;
    if bit(mask, 0) {
        s.font_style.weight = ints.get(n).copied().unwrap_or(400).clamp(1, 1000) as u16;
        n += 1;
    }
    if bit(mask, 1) {
        s.font_style.italic = font_style_index(ints.get(n).copied().unwrap_or(0));
        n += 1;
    }
    s.half_leading = bit(mask, 3);
    let floats: Vec<f32> = data[n * 4..]
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let mut f = 0;
    let mut next = || {
        let v = floats.get(f).copied().unwrap_or(0.0);
        f += 1;
        v
    };
    if bit(mask, 4) {
        s.font_size = next();
    }
    if bit(mask, 5) {
        s.height = next();
        s.height_override = true;
    }
    if bit(mask, 6) {
        s.leading = next();
    }
    s.force_height = bit(mask, 7);
    s.font_families = if bit(mask, 2) {
        families.to_vec()
    } else {
        vec![String::new()]
    };
    s
}

impl TextStyle {
    /// `ParagraphBuilder::pushStyle`: `encoded` is `Int32List(9)`, applied
    /// over `base`, the style on top of the stack.
    pub fn decode(base: &TextStyle, encoded: &[i32], args: &TextStyleArgs) -> TextStyle {
        let at = |i: usize| encoded.get(i).copied().unwrap_or(0);
        let mask = at(0);
        let mut s = base.clone();
        s.half_leading = bit(mask, 0);
        if bit(mask, 1) {
            s.color = at(1) as u32;
        }
        if bit(mask, 2) {
            s.decoration = at(2) as u32;
        }
        if bit(mask, 3) {
            s.decoration_color = at(3) as u32;
        }
        if bit(mask, 4) {
            s.decoration_style = match at(4) {
                1 => DecorationStyle::Double,
                2 => DecorationStyle::Dotted,
                3 => DecorationStyle::Dashed,
                4 => DecorationStyle::Wavy,
                _ => DecorationStyle::Solid,
            };
        }
        if bit(mask, 8) {
            s.decoration_thickness = args.decoration_thickness;
        }
        // The text baseline is decoded and dropped, as the engine drops it.
        if bit(mask, 5) {
            s.font_style.weight = at(5).clamp(1, 1000) as u16;
        }
        if bit(mask, 6) {
            s.font_style.italic = font_style_index(at(6));
        }
        if bit(mask, 10) {
            s.font_size = args.font_size as f32;
        }
        if bit(mask, 11) {
            s.letter_spacing = args.letter_spacing as f32;
        }
        if bit(mask, 12) {
            s.word_spacing = args.word_spacing as f32;
        }
        if bit(mask, 13) {
            s.height = args.height as f32;
            s.height_override = args.height != TEXT_HEIGHT_NONE;
        }
        if bit(mask, 14) {
            s.locale = args.locale.clone();
        }
        if bit(mask, 15) {
            s.background = args.background;
        }
        if bit(mask, 16) {
            s.foreground = args.foreground;
        }
        if bit(mask, 17) {
            s.shadows = args
                .shadows
                .chunks_exact(16)
                .map(|c| Shadow {
                    color: u32_le(c) ^ 0xFF00_0000,
                    offset: [
                        f32::from_le_bytes([c[4], c[5], c[6], c[7]]),
                        f32::from_le_bytes([c[8], c[9], c[10], c[11]]),
                    ],
                    blur_sigma: f64::from(f32::from_le_bytes([c[12], c[13], c[14], c[15]])),
                })
                .collect();
        }
        if bit(mask, 9) {
            s.font_families = args.font_families.clone();
        }
        if bit(mask, 18) {
            for c in args.font_features.chunks_exact(8) {
                let tag = [c[0], c[1], c[2], c[3]];
                let value = i32::from_le_bytes([c[4], c[5], c[6], c[7]]);
                // `SetFeature`: a later value for a tag replaces it.
                s.font_features.retain(|(t, _)| *t != tag);
                s.font_features.push((tag, value));
            }
        }
        if bit(mask, 19) {
            for c in args.font_variations.chunks_exact(8) {
                let tag = [c[0], c[1], c[2], c[3]];
                let value = f32::from_le_bytes([c[4], c[5], c[6], c[7]]);
                s.font_variations.retain(|(t, _)| *t != tag);
                s.font_variations.push((tag, value));
            }
        }
        s
    }
}

impl PlaceholderStyle {
    /// `ParagraphBuilder::addPlaceholder`'s arguments.
    pub fn new(
        width: f64,
        height: f64,
        alignment: u32,
        baseline_offset: f64,
        baseline: u32,
    ) -> Self {
        PlaceholderStyle {
            width: width as f32,
            height: height as f32,
            alignment: match alignment {
                1 => PlaceholderAlignment::AboveBaseline,
                2 => PlaceholderAlignment::BelowBaseline,
                3 => PlaceholderAlignment::Top,
                4 => PlaceholderAlignment::Bottom,
                5 => PlaceholderAlignment::Middle,
                _ => PlaceholderAlignment::Baseline,
            },
            baseline: if baseline == 1 {
                TextBaseline::Ideographic
            } else {
                TextBaseline::Alphabetic
            },
            baseline_offset: baseline_offset as f32,
        }
    }
}
