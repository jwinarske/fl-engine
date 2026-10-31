//! The paragraph: SkParagraph's `ParagraphImpl`, `ParagraphBuilderImpl`,
//! `Run`, `Cluster` and `OneLineShaper`, ported to keep its rules (brief M3
//! D1). Text is held as UTF-8, as SkParagraph holds it; the dart:ui facing
//! queries convert to and from UTF-16.
//!
//! Layout runs as SkParagraph's does: the whole text shaped into one
//! endless line (runs, then clusters), broken into lines
//! (`wrapper.rs`), and each line formatted (`line.rs`).

use std::collections::HashSet;
use std::sync::Arc;

use crate::font::{Face, FontCollection, Typeface};
use crate::line::TextLine;
use crate::shape::{self, FontMetrics, Shaped};
use crate::style::{
    ParagraphStyle, PlaceholderAlignment, PlaceholderStyle, TextBaseline, TextStyle,
};
use crate::unicode::{self, flag};

/// A range of UTF-8 bytes, or of clusters, glyphs or blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Range {
    pub start: usize,
    pub end: usize,
}

pub(crate) const EMPTY_INDEX: usize = usize::MAX;
pub(crate) const EMPTY_RANGE: Range = Range {
    start: EMPTY_INDEX,
    end: EMPTY_INDEX,
};

impl Range {
    pub fn new(start: usize, end: usize) -> Range {
        Range { start, end }
    }

    pub fn width(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// `operator*`: the overlap, or the empty range at `usize::MAX`.
    pub fn intersect(&self, other: Range) -> Range {
        if *self == other {
            return *self;
        }
        let s = self.start.max(other.start);
        let e = self.end.min(other.end);
        if e > s {
            Range::new(s, e)
        } else {
            EMPTY_RANGE
        }
    }

    /// `intersected`: the overlap, empty at its start when they only touch.
    pub fn intersected(&self, other: Range) -> Range {
        if *self == other {
            return *self;
        }
        let s = self.start.max(other.start);
        let e = self.end.min(other.end);
        if e >= s {
            Range::new(s, e)
        } else {
            EMPTY_RANGE
        }
    }

    /// `SkRange::intersection`, for cluster ranges.
    pub fn intersection(&self, other: Range) -> Range {
        Range::new(self.start.max(other.start), self.end.min(other.end))
    }

    pub fn intersects(&self, other: Range) -> bool {
        self.start.max(other.start) <= self.end.min(other.end)
    }
}

/// A font feature over a range of text: tag, value, range.
pub(crate) type FeatureSpan = ([u8; 4], u32, Range);

/// A styled stretch of text.
#[derive(Debug, Clone)]
pub(crate) struct Block {
    pub range: Range,
    pub style: TextStyle,
}

/// A placeholder, and the text and blocks before it.
#[derive(Debug, Clone)]
pub(crate) struct Placeholder {
    pub range: Range,
    pub style: PlaceholderStyle,
    pub text_style: TextStyle,
    pub text_before: Range,
}

/// `InternalLineMetrics`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LineMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub leading: f32,
    pub raw_ascent: f32,
    pub raw_descent: f32,
    pub raw_leading: f32,
    pub force_strut: bool,
}

impl LineMetrics {
    pub fn clean_with(force_strut: bool) -> LineMetrics {
        LineMetrics {
            ascent: f32::MAX,
            descent: -f32::MAX,
            leading: 0.0,
            raw_ascent: f32::MAX,
            raw_descent: -f32::MAX,
            raw_leading: 0.0,
            force_strut,
        }
    }

    pub fn new(a: f32, d: f32, l: f32) -> LineMetrics {
        LineMetrics {
            ascent: a,
            descent: d,
            leading: l,
            raw_ascent: a,
            raw_descent: d,
            raw_leading: l,
            force_strut: false,
        }
    }

    pub fn from_font(m: &FontMetrics, force_strut: bool) -> LineMetrics {
        LineMetrics {
            force_strut,
            ..LineMetrics::new(m.ascent, m.descent, m.leading)
        }
    }

    pub fn add_run(&mut self, run: &Run) {
        if self.force_strut {
            return;
        }
        self.ascent = self.ascent.min(run.correct_ascent());
        self.descent = self.descent.max(run.correct_descent());
        self.leading = self.leading.max(run.correct_leading);
        self.raw_ascent = self.raw_ascent.min(run.ascent());
        self.raw_descent = self.raw_descent.max(run.descent());
        self.raw_leading = self.raw_leading.max(run.leading());
    }

    pub fn add(&mut self, o: &LineMetrics) {
        self.ascent = self.ascent.min(o.ascent);
        self.descent = self.descent.max(o.descent);
        self.leading = self.leading.max(o.leading);
        self.raw_ascent = self.raw_ascent.min(o.raw_ascent);
        self.raw_descent = self.raw_descent.max(o.raw_descent);
        self.raw_leading = self.raw_leading.max(o.raw_leading);
    }

    pub fn clean(&mut self) {
        let f = self.force_strut;
        *self = LineMetrics::clean_with(f);
    }

    pub fn is_clean(&self) -> bool {
        self.ascent == f32::MAX
            && self.descent == -f32::MAX
            && self.leading == 0.0
            && self.raw_ascent == f32::MAX
            && self.raw_descent == -f32::MAX
            && self.raw_leading == 0.0
    }

    pub fn delta(&self) -> f32 {
        self.height() - self.ideographic_baseline()
    }

    /// `updateLineMetrics`: the strut's metrics onto a line's.
    pub fn update_line(&self, m: &mut LineMetrics) {
        if m.force_strut {
            m.ascent = self.ascent;
            m.descent = self.descent;
            m.leading = self.leading;
            m.raw_ascent = self.raw_ascent;
            m.raw_descent = self.raw_descent;
            m.raw_leading = self.raw_leading;
        } else {
            m.ascent = m.ascent.min(self.ascent - self.leading / 2.0);
            m.descent = m.descent.max(self.descent + self.leading / 2.0);
            m.raw_ascent = m.raw_ascent.min(self.raw_ascent - self.raw_leading / 2.0);
            m.raw_descent = m.raw_descent.max(self.raw_descent + self.raw_leading / 2.0);
        }
    }

    pub fn run_top(&self, run: &Run, typographic: bool) -> f32 {
        self.leading / 2.0 - self.ascent
            + if typographic {
                run.ascent()
            } else {
                run.correct_ascent()
            }
            + self.delta()
    }

    /// Rounded, as SkParagraph rounds a line's height.
    pub fn height(&self) -> f32 {
        (f64::from(self.descent) - f64::from(self.ascent) + f64::from(self.leading)).round() as f32
    }

    pub fn alphabetic_baseline(&self) -> f32 {
        self.leading / 2.0 - self.ascent
    }

    pub fn ideographic_baseline(&self) -> f32 {
        self.descent - self.ascent + self.leading
    }

    pub fn delta_baselines(&self) -> f32 {
        self.leading / 2.0 + self.descent
    }

    pub fn baseline(&self) -> f32 {
        self.leading / 2.0 - self.ascent
    }
}

/// A run of glyphs in one typeface and direction.
#[derive(Debug, Clone)]
pub(crate) struct Run {
    pub text_range: Range,
    pub cluster_range: Range,
    pub typeface: Option<Typeface>,
    pub font_size: f32,
    pub font_metrics: FontMetrics,
    pub placeholder: Option<usize>,
    pub advance: [f32; 2],
    pub offset: [f32; 2],
    pub cluster_start: usize,
    /// The run's text within what was shaped: start and length.
    pub utf8_range: (usize, usize),
    pub glyphs: Vec<u32>,
    pub positions: Vec<[f32; 2]>,
    pub offsets: Vec<[f32; 2]>,
    pub cluster_indexes: Vec<usize>,
    pub bidi_level: u8,
    pub height_multiplier: f32,
    pub use_half_leading: bool,
    pub baseline_shift: f32,
    pub correct_ascent: f32,
    pub correct_descent: f32,
    pub correct_leading: f32,
    pub ellipsis: bool,
    /// Per glyph: the shift, and the previous cluster's shift.
    pub justification_shifts: Vec<[f32; 2]>,
}

impl Run {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        typeface: Option<Typeface>,
        font_size: f32,
        bidi_level: u8,
        advance: [f32; 2],
        glyph_count: usize,
        utf8_range: (usize, usize),
        first_char: usize,
        height_multiplier: f32,
        use_half_leading: bool,
        baseline_shift: f32,
        offset_x: f32,
    ) -> Run {
        let font_metrics = typeface
            .as_ref()
            .map(|t| FontMetrics::of(t, font_size))
            .unwrap_or_default();
        let ltr = bidi_level % 2 == 0;
        let mut r = Run {
            text_range: Range::new(
                first_char + utf8_range.0,
                first_char + utf8_range.0 + utf8_range.1,
            ),
            cluster_range: EMPTY_RANGE,
            typeface,
            font_size,
            font_metrics,
            placeholder: None,
            advance,
            offset: [offset_x, 0.0],
            cluster_start: first_char,
            utf8_range,
            glyphs: vec![0; glyph_count],
            positions: vec![[0.0, 0.0]; glyph_count + 1],
            offsets: vec![[0.0, 0.0]; glyph_count + 1],
            cluster_indexes: vec![0; glyph_count + 1],
            bidi_level,
            height_multiplier,
            use_half_leading,
            baseline_shift,
            correct_ascent: 0.0,
            correct_descent: 0.0,
            correct_leading: 0.0,
            ellipsis: false,
            justification_shifts: Vec::new(),
        };
        r.calculate_metrics();
        r.positions[glyph_count] = [offset_x + advance[0], advance[1]];
        r.cluster_indexes[glyph_count] = if ltr {
            utf8_range.0 + utf8_range.1
        } else {
            utf8_range.0
        };
        r
    }

    pub fn calculate_metrics(&mut self) {
        let m = &self.font_metrics;
        self.correct_ascent = m.ascent - m.leading * 0.5;
        self.correct_descent = m.descent + m.leading * 0.5;
        self.correct_leading = 0.0;
        if self.height_multiplier.abs() <= f32::EPSILON * 4.0 {
            return;
        }
        let run_height = self.height_multiplier * self.font_size;
        let intrinsic = self.correct_descent - self.correct_ascent;
        if self.use_half_leading {
            let extra = (run_height - intrinsic) / 2.0;
            self.correct_ascent -= extra;
            self.correct_descent += extra;
        } else if intrinsic != 0.0 {
            let multiplier = run_height / intrinsic;
            self.correct_ascent *= multiplier;
            self.correct_descent *= multiplier;
        }
        self.correct_ascent += self.baseline_shift;
        self.correct_descent += self.baseline_shift;
    }

    pub fn size(&self) -> usize {
        self.glyphs.len()
    }

    pub fn left_to_right(&self) -> bool {
        self.bidi_level % 2 == 0
    }

    pub fn is_placeholder(&self) -> bool {
        self.placeholder.is_some()
    }

    pub fn ascent(&self) -> f32 {
        self.font_metrics.ascent + self.baseline_shift
    }

    pub fn descent(&self) -> f32 {
        self.font_metrics.descent + self.baseline_shift
    }

    pub fn leading(&self) -> f32 {
        self.font_metrics.leading
    }

    pub fn correct_ascent(&self) -> f32 {
        self.correct_ascent + self.baseline_shift
    }

    pub fn correct_descent(&self) -> f32 {
        self.correct_descent + self.baseline_shift
    }

    /// The advance, its height the font's line.
    pub fn advance(&self) -> [f32; 2] {
        let m = &self.font_metrics;
        [self.advance[0], m.descent - m.ascent + m.leading]
    }

    pub fn pos_x(&self, i: usize) -> f32 {
        self.positions[i][0]
    }

    pub fn position_x(&self, i: usize) -> f32 {
        self.pos_x(i) + self.justification_shifts.get(i).map_or(0.0, |s| s[1])
    }

    pub fn calculate_width(&self, start: usize, end: usize) -> f32 {
        let correction = if end > start && !self.justification_shifts.is_empty() {
            self.justification_shifts[end - 1][0] - self.justification_shifts[start][1]
        } else {
            0.0
        };
        self.pos_x(end) - self.pos_x(start) + correction
    }

    pub fn calculate_height(&self, ascent_typographic: bool, descent_typographic: bool) -> f32 {
        let a = if ascent_typographic {
            self.ascent()
        } else {
            self.correct_ascent()
        };
        let d = if descent_typographic {
            self.descent()
        } else {
            self.correct_descent()
        };
        d - a
    }

    pub fn cluster_index(&self, pos: usize) -> usize {
        self.cluster_indexes[pos]
    }

    pub fn global_cluster_index(&self, pos: usize) -> usize {
        self.cluster_start + self.cluster_indexes[pos]
    }

    pub fn is_resolved(&self) -> bool {
        self.glyphs.iter().all(|g| *g != 0)
    }

    /// `addLetterSpacesEvenly` for the whole run.
    pub fn add_letter_spaces_evenly(&mut self, space: f32) -> f32 {
        let mut shift = 0.0;
        let n = self.size();
        for i in 0..n {
            self.positions[i][0] += shift;
            shift += space;
        }
        self.positions[n][0] += shift;
        self.advance[0] += shift;
        shift
    }

    /// `updateMetrics`: a placeholder's metrics from its style and the line.
    pub fn update_placeholder_metrics(&mut self, style: &PlaceholderStyle, line: &mut LineMetrics) {
        let adjust = match style.baseline {
            TextBaseline::Alphabetic => 0.0,
            TextBaseline::Ideographic => line.delta_baselines() / 2.0,
        };
        let (h, off) = (style.height, style.baseline_offset);
        let m = &mut self.font_metrics;
        m.leading = 0.0;
        match style.alignment {
            PlaceholderAlignment::Baseline => {
                m.ascent = adjust - off;
                m.descent = adjust + h - off;
            }
            PlaceholderAlignment::AboveBaseline => {
                m.ascent = adjust - h;
                m.descent = adjust;
            }
            PlaceholderAlignment::BelowBaseline => {
                m.ascent = adjust;
                m.descent = adjust + h;
            }
            PlaceholderAlignment::Top => {
                m.descent = h + m.ascent;
            }
            PlaceholderAlignment::Bottom => {
                m.ascent = m.descent - h;
            }
            PlaceholderAlignment::Middle => {
                let mid = (-m.descent - m.ascent) / 2.0;
                m.descent = h / 2.0 - mid;
                m.ascent = -h / 2.0 - mid;
            }
        }
        self.calculate_metrics();
        line.add_run(self);
    }

    /// The cursive scripts letter spacing is not added to.
    pub fn is_cursive_script(&self, text: &str) -> bool {
        text.get(self.text_range.start..self.text_range.end)
            .and_then(|t| t.chars().find(|c| !c.is_whitespace()))
            .is_some_and(|c| {
                matches!(c,
                    '\u{600}'..='\u{6FF}' | '\u{750}'..='\u{77F}' | '\u{8A0}'..='\u{8FF}'
                    | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}'
                    | '\u{700}'..='\u{74F}' | '\u{840}'..='\u{85F}' | '\u{1800}'..='\u{18AF}'
                    | '\u{7C0}'..='\u{7FF}' | '\u{A840}'..='\u{A87F}' | '\u{10D00}'..='\u{10D3F}')
            })
    }
}

/// A cluster: the glyphs that cover one or more code units.
#[derive(Debug, Clone)]
pub(crate) struct Cluster {
    pub run_index: usize,
    pub text_range: Range,
    pub start: usize,
    pub end: usize,
    pub width: f32,
    pub half_letter_spacing: f32,
    pub is_whitespace_break: bool,
    pub is_intra_word_break: bool,
    pub is_hard_break: bool,
    pub is_ideographic: bool,
}

impl Cluster {
    pub fn starts_in(&self, r: Range) -> bool {
        self.text_range.start >= r.start && self.text_range.start < r.end
    }

    pub fn size_to_char(&self, ch: usize) -> f32 {
        if ch < self.text_range.start || ch >= self.text_range.end {
            return 0.0;
        }
        let shift = ch - self.text_range.start;
        (f64::from(self.width) * shift as f64 / self.text_range.width() as f64) as f32
    }

    pub fn size_from_char(&self, ch: usize) -> f32 {
        if ch < self.text_range.start || ch >= self.text_range.end {
            return 0.0;
        }
        let shift = self.text_range.end - ch - 1;
        (f64::from(self.width) * shift as f64 / self.text_range.width() as f64) as f32
    }
}

/// What `layout` has done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum State {
    Unknown,
    Indexed,
    Shaped,
    LineBroken,
    Formatted,
}

/// A paragraph: built once, laid out at a width, then measured, queried
/// and painted.
#[derive(Debug)]
pub struct Paragraph {
    pub(crate) fonts: Arc<FontCollection>,
    pub(crate) style: ParagraphStyle,
    pub(crate) text: String,
    pub(crate) blocks: Vec<Block>,
    pub(crate) placeholders: Vec<Placeholder>,
    state: State,
    pub(crate) flags: Vec<u16>,
    bidi: Vec<unicode::BidiRegion>,
    pub(crate) runs: Vec<Run>,
    pub(crate) clusters: Vec<Cluster>,
    pub(crate) cluster_of: Vec<usize>,
    pub(crate) lines: Vec<TextLine>,
    utf8_of_16: Vec<usize>,
    pub(crate) utf16_of_8: Vec<usize>,
    words: Vec<usize>,
    pub(crate) empty_metrics: LineMetrics,
    pub(crate) strut_metrics: LineMetrics,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) max_intrinsic_width: f32,
    pub(crate) min_intrinsic_width: f32,
    pub(crate) longest_line: f32,
    pub(crate) alphabetic_baseline: f32,
    pub(crate) ideographic_baseline: f32,
    pub(crate) exceeded_max_lines: bool,
    pub(crate) max_width_with_trailing_spaces: f32,
    trailing_spaces: usize,
    has_line_breaks: bool,
    has_whitespaces_inside: bool,
    old_width: f32,
    /// Code points no font had.
    pub(crate) unresolved: HashSet<char>,
    /// Code points drawn in a system fallback face, and its family.
    pub(crate) fell_back: Vec<(char, String)>,
}

/// Builds a [`Paragraph`]: `ParagraphBuilderImpl`.
#[derive(Debug)]
pub struct ParagraphBuilder {
    fonts: Arc<FontCollection>,
    style: ParagraphStyle,
    styles: Vec<TextStyle>,
    text: String,
    blocks: Vec<Block>,
    placeholders: Vec<Placeholder>,
}

impl ParagraphBuilder {
    pub fn new(style: ParagraphStyle, fonts: Arc<FontCollection>) -> ParagraphBuilder {
        let mut b = ParagraphBuilder {
            fonts,
            style,
            styles: Vec::new(),
            text: String::new(),
            blocks: Vec::new(),
            placeholders: Vec::new(),
        };
        b.start_block();
        b
    }

    fn peek(&self) -> &TextStyle {
        self.styles.last().unwrap_or(&self.style.text_style)
    }

    /// The style on top of the stack, which `pushStyle`'s encoding is
    /// applied over.
    pub fn peek_style(&self) -> TextStyle {
        self.peek().clone()
    }

    pub fn push_style(&mut self, style: TextStyle) {
        let same = self
            .blocks
            .last()
            .is_some_and(|b| b.range.end == self.text.len() && b.style == style);
        self.styles.push(style);
        if !same {
            self.start_block();
        }
    }

    pub fn pop(&mut self) {
        self.styles.pop();
        self.start_block();
    }

    pub fn add_text(&mut self, text: &str) {
        self.text.push_str(text);
    }

    pub fn add_placeholder(&mut self, style: PlaceholderStyle) {
        self.add_placeholder_inner(style, false);
    }

    fn add_placeholder_inner(&mut self, style: PlaceholderStyle, last: bool) {
        if !self.text.is_empty() && !last {
            self.end_run_if_needed();
        }
        let text_before = Range::new(
            self.placeholders.last().map_or(0, |p| p.range.end),
            self.text.len(),
        );
        let start = self.text.len();
        let top = self.peek().clone();
        if !last {
            self.push_style(top.for_placeholder());
            self.add_text("\u{FFFC}");
            self.pop();
        }
        let end = self.text.len();
        self.placeholders.push(Placeholder {
            range: Range::new(start, end),
            style,
            text_style: top,
            text_before,
        });
    }

    fn end_run_if_needed(&mut self) {
        let len = self.text.len();
        if let Some(last) = self.blocks.last_mut() {
            if last.range.start == len {
                self.blocks.pop();
            } else {
                last.range.end = len;
            }
        }
    }

    fn start_block(&mut self) {
        self.end_run_if_needed();
        let len = self.text.len();
        let style = self.peek().clone();
        self.blocks.push(Block {
            range: Range::new(len, len),
            style,
        });
    }

    pub fn build(mut self) -> Paragraph {
        if !self.text.is_empty() {
            self.end_run_if_needed();
        }
        self.add_placeholder_inner(PlaceholderStyle::default(), true);
        Paragraph::new(
            self.text,
            self.style,
            self.blocks,
            self.placeholders,
            self.fonts,
        )
    }
}

impl Paragraph {
    fn new(
        text: String,
        style: ParagraphStyle,
        blocks: Vec<Block>,
        placeholders: Vec<Placeholder>,
        fonts: Arc<FontCollection>,
    ) -> Paragraph {
        let (utf8_of_16, utf16_of_8) = unicode::utf_maps(&text);
        Paragraph {
            fonts,
            style,
            text,
            blocks,
            placeholders,
            state: State::Unknown,
            flags: Vec::new(),
            bidi: Vec::new(),
            runs: Vec::new(),
            clusters: Vec::new(),
            cluster_of: Vec::new(),
            lines: Vec::new(),
            utf8_of_16,
            utf16_of_8,
            words: Vec::new(),
            empty_metrics: LineMetrics::clean_with(false),
            strut_metrics: LineMetrics::clean_with(false),
            width: 0.0,
            height: 0.0,
            max_intrinsic_width: 0.0,
            min_intrinsic_width: 0.0,
            longest_line: 0.0,
            alphabetic_baseline: 0.0,
            ideographic_baseline: 0.0,
            exceeded_max_lines: false,
            max_width_with_trailing_spaces: 0.0,
            trailing_spaces: 0,
            has_line_breaks: false,
            has_whitespaces_inside: false,
            old_width: 0.0,
            unresolved: HashSet::new(),
            fell_back: Vec::new(),
        }
    }

    pub(crate) fn has_flag(&self, i: usize, f: u16) -> bool {
        self.flags.get(i).is_some_and(|x| x & f != 0)
    }

    pub(crate) fn utf16_index(&self, utf8: usize) -> usize {
        self.utf16_of_8.get(utf8).copied().unwrap_or(0)
    }

    pub(crate) fn utf8_index(&self, utf16: usize) -> Option<usize> {
        self.utf8_of_16.get(utf16).copied()
    }

    pub(crate) fn utf16_len(&self) -> usize {
        self.utf8_of_16.len()
    }

    pub(crate) fn cluster_index(&self, utf8: usize) -> usize {
        self.cluster_of.get(utf8).copied().unwrap_or(EMPTY_INDEX)
    }

    pub(crate) fn is_soft_break(&self, c: &Cluster) -> bool {
        self.has_flag(c.text_range.end, flag::SOFT_LINE_BREAK_BEFORE)
    }

    pub(crate) fn trimmed_width(&self, c: &Cluster, pos: usize) -> f32 {
        let run = &self.runs[c.run_index];
        (run.position_x(pos) - run.position_x(c.start)).min(c.width)
    }

    pub(crate) fn strut_enabled(&self) -> bool {
        self.style.strut.enabled
    }

    /// `layout(width)`.
    pub fn layout(&mut self, raw_width: f32) {
        let floor_width = raw_width;
        if (!raw_width.is_finite() || self.longest_line <= floor_width)
            && self.state >= State::LineBroken
            && self.lines.len() == 1
            && self.lines[0].ellipsis.is_none()
        {
            self.width = floor_width;
            self.state = State::Shaped;
        } else if self.state >= State::LineBroken && self.old_width != floor_width {
            self.state = State::Shaped;
        }
        if self.state < State::Shaped {
            if self.state < State::Indexed {
                self.compute_code_unit_properties();
                self.state = State::Indexed;
            }
            self.runs.clear();
            self.clusters.clear();
            self.cluster_of = vec![EMPTY_INDEX; self.text.len() + 1];
            if !self.shape_text_into_endless_line() {
                self.reset_context();
                self.resolve_strut();
                self.compute_empty_metrics();
                self.lines.clear();
                self.width = floor_width;
                self.height = self.empty_metrics.height();
                if self.style.strut_force_height() {
                    self.height = self.strut_metrics.height();
                }
                self.alphabetic_baseline = self.empty_metrics.alphabetic_baseline();
                self.ideographic_baseline = self.empty_metrics.ideographic_baseline();
                self.longest_line = f32::MIN_POSITIVE - f32::MAX;
                self.min_intrinsic_width = 0.0;
                self.max_intrinsic_width = 0.0;
                self.old_width = floor_width;
                return;
            }
            self.state = State::Shaped;
        }
        if self.state == State::Shaped {
            self.reset_context();
            self.resolve_strut();
            self.compute_empty_metrics();
            self.lines.clear();
            self.break_shaped_text_into_lines(floor_width);
            self.state = State::LineBroken;
        }
        if self.state == State::LineBroken {
            for run in &mut self.runs {
                run.justification_shifts.clear();
            }
            self.format_lines(self.width);
            self.state = State::Formatted;
        }
        self.old_width = floor_width;
        if self.style.max_lines == 1 || (self.style.unlimited_lines() && self.style.ellipsized()) {
            self.min_intrinsic_width = self.max_intrinsic_width;
        }
        if self.max_intrinsic_width < self.min_intrinsic_width {
            self.max_intrinsic_width = self.min_intrinsic_width;
        }
    }

    fn reset_context(&mut self) {
        self.alphabetic_baseline = 0.0;
        self.height = 0.0;
        self.width = 0.0;
        self.ideographic_baseline = 0.0;
        self.max_intrinsic_width = 0.0;
        self.min_intrinsic_width = 0.0;
        self.longest_line = 0.0;
        self.max_width_with_trailing_spaces = 0.0;
        self.exceeded_max_lines = false;
    }

    fn compute_code_unit_properties(&mut self) {
        self.bidi = unicode::bidi_regions(
            &self.text,
            self.style.text_direction == crate::style::TextDirection::Rtl,
        );
        self.flags = unicode::code_unit_flags(&mut self.text);
        let n = self.text.len();
        self.trailing_spaces = n;
        let mut first_ws = EMPTY_INDEX;
        for i in 0..self.flags.len() {
            let f = self.flags[i];
            if f & flag::WHITESPACE_BREAK != 0 {
                if self.trailing_spaces == n {
                    self.trailing_spaces = i;
                }
                if first_ws == EMPTY_INDEX {
                    first_ws = i;
                }
            } else {
                self.trailing_spaces = n;
            }
            if f & flag::HARD_LINE_BREAK_BEFORE != 0 {
                self.has_line_breaks = true;
            }
        }
        if first_ws < self.trailing_spaces {
            self.has_whitespaces_inside = true;
        }
    }

    /// `shapeTextIntoEndlessLine`.
    fn shape_text_into_endless_line(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }
        self.unresolved.clear();
        self.fell_back.clear();
        let mut advance_x = 0.0f32;
        let mut bidi_index = 0;
        let placeholders = self.placeholders.clone();
        for (pi, ph) in placeholders.iter().enumerate() {
            if ph.text_before.width() > 0 {
                while bidi_index < self.bidi.len() {
                    let region = self.bidi[bidi_index];
                    let start = region.start.max(ph.text_before.start);
                    let end = region.end.min(ph.text_before.end);
                    let range = Range::new(start, end);
                    if end > start {
                        self.shape_region(range, region.level, &mut advance_x);
                    }
                    if end == region.end {
                        bidi_index += 1;
                    } else {
                        break;
                    }
                }
            }
            if ph.range.width() == 0 {
                continue;
            }
            let tf = self
                .fonts
                .find(&ph.text_style.font_families, ph.text_style.font_style)
                .first()
                .map(|f| Typeface::new(f, &ph.text_style.variations()));
            let level = self.bidi.get(bidi_index).map_or(2, |r| r.level);
            let mut run = Run::new(
                tf,
                ph.text_style.font_size,
                level,
                [ph.style.width, ph.style.height],
                1,
                (0, ph.range.width()),
                ph.range.start,
                0.0,
                false,
                0.0,
                advance_x,
            );
            run.positions[0] = [advance_x, 0.0];
            run.offsets[0] = [0.0, 0.0];
            run.cluster_indexes[0] = 0;
            run.placeholder = Some(pi);
            self.runs.push(run);
            advance_x += ph.style.width;
        }
        self.apply_spacing_and_build_cluster_table();
        true
    }

    /// The blocks overlapping `range`.
    pub(crate) fn find_all_blocks(&self, range: Range) -> Range {
        let mut begin = EMPTY_INDEX;
        let mut end = EMPTY_INDEX;
        for (i, b) in self.blocks.iter().enumerate() {
            if b.range.end <= range.start {
                continue;
            }
            if b.range.start >= range.end {
                break;
            }
            if begin == EMPTY_INDEX {
                begin = i;
            }
            end = i;
        }
        if begin == EMPTY_INDEX || end == EMPTY_INDEX {
            return EMPTY_RANGE;
        }
        Range::new(begin, end + 1)
    }

    /// One bidi region's text, shaped style block by style block
    /// (`iterateThroughFontStyles`), blocks that shape alike combined.
    fn shape_region(&mut self, range: Range, level: u8, advance_x: &mut f32) {
        let blocks = self.find_all_blocks(range);
        if blocks.start == EMPTY_INDEX {
            return;
        }
        let mut combined: Option<(Range, TextStyle, Vec<FeatureSpan>)> = None;
        let mut groups = Vec::new();
        for b in &self.blocks[blocks.start..blocks.end] {
            let br = Range::new(b.range.start.max(range.start), b.range.end.min(range.end));
            if br.is_empty() {
                continue;
            }
            let features = block_features(b, br);
            match &mut combined {
                Some((r, s, f)) if b.style.same_font(s) && r.end == br.start => {
                    r.end = br.end;
                    f.extend(features);
                }
                _ => {
                    if let Some(c) = combined.take() {
                        groups.push(c);
                    }
                    combined = Some((br, b.style.clone(), features));
                }
            }
        }
        groups.extend(combined);
        for (r, style, features) in groups {
            self.shape_block(r, &style, &features, level, advance_x);
        }
    }

    /// `OneLineShaper::shape` for one block: the block's families in
    /// order, then a fallback per code point still unresolved, then what
    /// is left drawn with the first face's `.notdef`.
    fn shape_block(
        &mut self,
        range: Range,
        style: &TextStyle,
        features: &[FeatureSpan],
        level: u8,
        advance_x: &mut f32,
    ) {
        let rtl = level % 2 == 1;
        let variations = style.variations();
        let faces = self.fonts.find(&style.font_families, style.font_style);
        let mut tried: Vec<u64> = Vec::new();
        let mut unresolved: Vec<Range> = vec![range];
        let mut resolved: Vec<Piece> = Vec::new();
        let mut first: Option<Piece> = None;
        let attempt = |p: &Paragraph,
                       face: &Arc<Face>,
                       unresolved: &mut Vec<Range>,
                       resolved: &mut Vec<Piece>,
                       first: &mut Option<Piece>| {
            let tf = Typeface::new(face, &variations);
            let mut still = Vec::new();
            for r in unresolved.drain(..) {
                let text = &p.text[r.start..r.end];
                let feats: Vec<([u8; 4], u32, std::ops::Range<usize>)> = features
                    .iter()
                    .filter(|(_, _, fr)| fr.intersects(r))
                    .map(|(t, v, fr)| {
                        let s = fr.start.max(r.start) - r.start;
                        let e = fr.end.min(r.end) - r.start;
                        (*t, *v, s..e)
                    })
                    .collect();
                let shaped = shape::shape(&tf, style.font_size, text, rtl, &style.locale, &feats);
                if first.is_none() {
                    *first = Some(Piece {
                        tf: tf.clone(),
                        shaped: Arc::clone(&shaped),
                        text: r,
                        glyphs: Range::new(0, shaped.glyphs.len()),
                        shaped_start: r.start,
                    });
                }
                for (ok, text_r, glyphs) in p.sort_out_glyphs(&shaped, r, rtl) {
                    if ok {
                        resolved.push(Piece {
                            tf: tf.clone(),
                            shaped: Arc::clone(&shaped),
                            text: text_r,
                            glyphs,
                            shaped_start: r.start,
                        });
                    } else {
                        still.push(text_r);
                    }
                }
            }
            *unresolved = still;
        };
        for face in &faces {
            if unresolved.is_empty() {
                break;
            }
            tried.push(face.id);
            attempt(self, face, &mut unresolved, &mut resolved, &mut first);
        }
        // Fallback, a code point at a time.
        let mut hopeless: Vec<Range> = Vec::new();
        while let Some(front) = unresolved.first().copied() {
            let mut progressed = false;
            let chars: Vec<char> = self.text[front.start..front.end].chars().collect();
            let mut seen = HashSet::new();
            for c in chars {
                if !seen.insert(c) {
                    continue;
                }
                let Some(face) = self.fonts.fallback(c, style.font_style) else {
                    continue;
                };
                if tried.contains(&face.id) {
                    continue;
                }
                tried.push(face.id);
                let before = resolved.len();
                attempt(self, &face, &mut unresolved, &mut resolved, &mut first);
                if resolved.len() > before {
                    self.fell_back.push((c, face.family.clone()));
                    progressed = true;
                    break;
                }
            }
            if !progressed {
                if let Some(f) = unresolved.first().copied() {
                    unresolved.remove(0);
                    hopeless.push(f);
                }
            }
        }
        // Text no face has: the first shaping's glyphs, `.notdef` and all.
        for h in hopeless {
            for c in self.text[h.start..h.end].chars() {
                self.unresolved.insert(c);
            }
            if let Some(f) = &first {
                if let Some(glyphs) = f.glyphs_for(h, rtl) {
                    resolved.push(Piece {
                        text: h,
                        glyphs,
                        ..f.clone()
                    });
                }
            } else if let Some(face) = self.fonts.find(&[], style.font_style).first() {
                let tf = Typeface::new(face, &variations);
                let shaped = shape::shape(
                    &tf,
                    style.font_size,
                    &self.text[h.start..h.end],
                    rtl,
                    &style.locale,
                    &[],
                );
                let n = shaped.glyphs.len();
                resolved.push(Piece {
                    tf,
                    shaped,
                    text: h,
                    glyphs: Range::new(0, n),
                    shaped_start: h.start,
                });
            }
        }
        resolved.sort_by_key(|p| p.text.start);
        for piece in resolved {
            self.add_piece(piece, style, level, advance_x);
        }
    }

    /// `sortOutGlyphs` and `clusteredText`: the shaped text split into
    /// resolved and unresolved stretches, a grapheme at a time. Each is
    /// whether resolved, its text (absolute) and its glyphs.
    fn sort_out_glyphs(&self, s: &Shaped, r: Range, rtl: bool) -> Vec<(bool, Range, Range)> {
        let n = s.glyphs.len();
        if n == 0 {
            return vec![(true, r, Range::new(0, 0))];
        }
        // Per glyph: whether its grapheme is resolved.
        let mut ok = vec![true; n];
        let mut grapheme_start = EMPTY_INDEX;
        let mut grapheme_ok = true;
        let mut grapheme_glyphs: Vec<usize> = Vec::new();
        let flush = |g: &[usize], good: bool, ok: &mut [bool]| {
            for i in g {
                ok[*i] = good;
            }
        };
        for i in 0..n {
            let ci = r.start + s.clusters[i] as usize;
            let gi = self.previous_grapheme(ci);
            if gi != grapheme_start {
                flush(&grapheme_glyphs, grapheme_ok, &mut ok);
                grapheme_glyphs.clear();
                grapheme_start = gi;
                grapheme_ok = s.glyphs[i] != 0 || self.has_flag(ci, flag::CONTROL);
            } else if s.glyphs[i] == 0 {
                grapheme_ok = false;
            }
            grapheme_glyphs.push(i);
        }
        flush(&grapheme_glyphs, grapheme_ok, &mut ok);
        // Maximal stretches of glyphs alike.
        let mut out = Vec::new();
        let mut i = 0;
        while i < n {
            let mut j = i;
            while j < n && ok[j] == ok[i] {
                j += 1;
            }
            let text = if !rtl {
                let s0 = r.start + s.clusters[i] as usize;
                let e0 = if j < n {
                    r.start + s.clusters[j] as usize
                } else {
                    r.end
                };
                Range::new(s0, e0)
            } else {
                let s0 = r.start + s.clusters[j - 1] as usize;
                let e0 = if i > 0 {
                    r.start + s.clusters[i - 1] as usize
                } else {
                    r.end
                };
                Range::new(s0, e0)
            };
            out.push((ok[i], text, Range::new(i, j)));
            i = j;
        }
        out
    }

    pub(crate) fn previous_grapheme(&self, mut utf8: usize) -> usize {
        while utf8 > 0 && !self.has_flag(utf8, flag::GRAPHEME_START) {
            utf8 -= 1;
        }
        utf8
    }

    pub(crate) fn next_grapheme(&self, mut utf8: usize) -> usize {
        while utf8 < self.text.len() && !self.has_flag(utf8, flag::GRAPHEME_START) {
            utf8 += 1;
        }
        utf8
    }

    pub(crate) fn next_glyph_cluster(&self, mut utf8: usize) -> usize {
        while utf8 < self.text.len() && !self.has_flag(utf8, flag::GLYPH_CLUSTER_START) {
            utf8 += 1;
        }
        utf8
    }

    pub(crate) fn previous_glyph_cluster(&self, mut utf8: usize) -> usize {
        while utf8 > 0 && !self.has_flag(utf8, flag::GLYPH_CLUSTER_START) {
            utf8 -= 1;
        }
        utf8
    }

    /// `OneLineShaper::finish`: a resolved piece as a run.
    fn add_piece(&mut self, p: Piece, style: &TextStyle, level: u8, advance_x: &mut f32) {
        let s = &p.shaped;
        let g = p.glyphs;
        let run_advance = s.positions[g.end][0] - s.positions[g.start][0];
        let mut run = Run::new(
            Some(p.tf.clone()),
            style.font_size,
            level,
            [run_advance, 0.0],
            g.width(),
            (p.text.start - p.shaped_start, p.text.width()),
            p.shaped_start,
            style.height_multiplier(),
            style.half_leading,
            0.0,
            *advance_x,
        );
        let zero = s.positions[g.start][0];
        for i in g.start..=g.end {
            let k = i - g.start;
            if i < g.end {
                run.glyphs[k] = s.glyphs[i];
                run.cluster_indexes[k] = s.clusters[i] as usize;
            }
            run.positions[k] = [s.positions[i][0] - zero + *advance_x, s.positions[i][1]];
            run.offsets[k] = s.offsets[i];
        }
        // The end cluster of a piece is its own text's end (or start).
        run.cluster_indexes[g.width()] = if level % 2 == 0 {
            p.text.end - p.shaped_start
        } else {
            p.text.start - p.shaped_start
        };
        *advance_x += run_advance;
        self.runs.push(run);
    }

    /// `applySpacingAndBuildClusterTable`.
    fn apply_spacing_and_build_cluster_table(&mut self) {
        let mut letter_spacing_styles = 0;
        let mut has_word_spacing = false;
        for b in &self.blocks {
            if b.range.width() > 0 {
                if b.style.letter_spacing.abs() > f32::EPSILON {
                    letter_spacing_styles += 1;
                }
                if b.style.word_spacing.abs() > f32::EPSILON {
                    has_word_spacing = true;
                }
            }
        }
        if letter_spacing_styles == 0 && !has_word_spacing {
            self.build_cluster_table();
            return;
        }
        if letter_spacing_styles == 1
            && !has_word_spacing
            && self.blocks.len() == 1
            && self.blocks[0].range.width() == self.text.len()
            && self.runs.len() == 1
        {
            let ls = self.blocks[0].style.letter_spacing;
            if !self.runs[0].is_cursive_script(&self.text) {
                self.runs[0].add_letter_spaces_evenly(ls);
            }
            self.build_cluster_table();
            for c in &mut self.clusters {
                c.half_letter_spacing = ls / 2.0;
            }
            return;
        }
        self.build_cluster_table();
        let mut shift = 0.0f32;
        let mut so_far_ws_only = true;
        let mut word_spacing_pending = false;
        let mut last_space: Option<usize> = None;
        for ri in 0..self.runs.len() {
            if self.runs[ri].is_placeholder() {
                continue;
            }
            let cr = self.runs[ri].cluster_range;
            let ltr = self.runs[ri].left_to_right();
            let cursive = self.runs[ri].is_cursive_script(&self.text);
            for k in 0..cr.width() {
                let ci = if ltr { cr.start + k } else { cr.end - k - 1 };
                self.shift_cluster_glyphs(ri, ci, shift);
                let bi = self
                    .blocks
                    .iter()
                    .position(|b| self.clusters[ci].starts_in(b.range))
                    .unwrap_or(0);
                let ws = self.blocks[bi].style.word_spacing;
                let ls = self.blocks[bi].style.letter_spacing;
                let is_ws = self.clusters[ci].is_whitespace_break;
                if ws != 0.0 {
                    if is_ws && self.is_soft_break(&self.clusters[ci]) {
                        if !so_far_ws_only {
                            last_space = Some(ci);
                            word_spacing_pending = true;
                        }
                    } else if word_spacing_pending {
                        if let Some(ls_ci) = last_space {
                            let lr = self.clusters[ls_ci].run_index;
                            self.runs[lr].advance[0] += ws;
                            self.clusters[ls_ci].width += ws;
                            if lr != ri {
                                let end = self.clusters[ls_ci].end;
                                self.runs[lr].positions[end][0] += ws;
                            }
                        }
                        self.shift_cluster_glyphs(ri, ci, ws);
                        shift += ws;
                        word_spacing_pending = false;
                    }
                }
                if ls != 0.0 && !cursive {
                    shift += self.add_letter_spaces_to_cluster(ri, ci, ls);
                }
                if so_far_ws_only && !is_ws {
                    so_far_ws_only = false;
                }
            }
        }
    }

    /// `Run::shift(cluster, offset)`.
    fn shift_cluster_glyphs(&mut self, ri: usize, ci: usize, offset: f32) {
        if offset == 0.0 {
            return;
        }
        let (s, e) = (self.clusters[ci].start, self.clusters[ci].end);
        let run = &mut self.runs[ri];
        for i in s..e {
            run.positions[i][0] += offset;
        }
        if run.size() == e {
            run.positions[e][0] += offset;
        }
    }

    /// `Run::addLetterSpacesEvenly(space, cluster)`.
    fn add_letter_spaces_to_cluster(&mut self, ri: usize, ci: usize, space: f32) -> f32 {
        let (s, e) = (self.clusters[ci].start, self.clusters[ci].end);
        let run = &mut self.runs[ri];
        let mut shift = 0.0;
        for i in s..e {
            run.positions[i][0] += shift;
            shift += space;
        }
        if run.size() == e {
            run.positions[e][0] += shift;
        }
        run.advance[0] += shift;
        let c = &mut self.clusters[ci];
        c.width += shift;
        c.half_letter_spacing = space / 2.0;
        shift
    }

    /// `buildClusterTable`.
    fn build_cluster_table(&mut self) {
        let n = self.text.len();
        for run in &self.runs {
            self.flags[run.text_range.start] |= flag::GRAPHEME_START | flag::GLYPH_CLUSTER_START;
        }
        if let Some(last) = self.runs.last() {
            self.flags[last.text_range.end] |= flag::GRAPHEME_START | flag::GLYPH_CLUSTER_START;
        }
        for ri in 0..self.runs.len() {
            let run_start = self.clusters.len();
            if self.runs[ri].is_placeholder() {
                let tr = self.runs[ri].text_range;
                for i in tr.start..tr.end {
                    self.cluster_of[i] = self.clusters.len();
                }
                let adv = self.runs[ri].advance();
                let c = self.make_cluster(ri, 0, 1, tr, adv[0]);
                self.clusters.push(c);
                self.flags[tr.start] |= flag::SOFT_LINE_BREAK_BEFORE;
                self.flags[tr.end] |= flag::SOFT_LINE_BREAK_BEFORE;
            } else {
                let pieces = self.clusters_in_text_order(ri);
                for (gs, ge, cs, ce, w) in pieces {
                    for i in cs..ce {
                        self.cluster_of[i] = self.clusters.len();
                    }
                    let c = self.make_cluster(ri, gs, ge, Range::new(cs, ce), w);
                    self.clusters.push(c);
                    self.flags[cs] |= flag::GLYPH_CLUSTER_START;
                }
            }
            let start = self.runs[ri].text_range.start;
            self.flags[start] |= flag::GLYPH_CLUSTER_START;
            self.runs[ri].cluster_range = Range::new(run_start, self.clusters.len());
            self.max_intrinsic_width += self.runs[ri].advance()[0];
        }
        // The empty cluster at the end, as `Cluster`'s constructor makes it:
        // none of no code units is not whitespace, so it is a whitespace and
        // an intra-word break, which is what lets a line's trailing-space
        // trim step over it.
        self.cluster_of[n] = self.clusters.len();
        self.clusters.push(Cluster {
            run_index: EMPTY_INDEX,
            text_range: Range::new(n, n),
            start: 0,
            end: 0,
            width: 0.0,
            half_letter_spacing: 0.0,
            is_whitespace_break: true,
            is_intra_word_break: true,
            is_hard_break: self.has_flag(n, flag::HARD_LINE_BREAK_BEFORE),
            is_ideographic: false,
        });
    }

    /// `iterateThroughClustersInTextOrder`: glyph start, end, text start,
    /// end and width of each cluster of run `ri`.
    fn clusters_in_text_order(&self, ri: usize) -> Vec<(usize, usize, usize, usize, f32)> {
        let run = &self.runs[ri];
        let mut out = Vec::new();
        let n = run.size();
        if run.left_to_right() {
            let mut start = 0;
            let mut cluster = run.cluster_index(0);
            for glyph in 1..=n {
                let next = run.cluster_index(glyph);
                if next <= cluster {
                    continue;
                }
                out.push((
                    start,
                    glyph,
                    run.cluster_start + cluster,
                    run.cluster_start + next,
                    run.calculate_width(start, glyph),
                ));
                start = glyph;
                cluster = next;
            }
        } else {
            let mut glyph = n;
            let mut cluster = run.utf8_range.0;
            for start in (0..n).rev() {
                let next = if start == 0 {
                    run.utf8_range.0 + run.utf8_range.1
                } else {
                    run.cluster_index(start - 1)
                };
                if next <= cluster {
                    continue;
                }
                out.push((
                    start,
                    glyph,
                    run.cluster_start + cluster,
                    run.cluster_start + next,
                    run.calculate_width(start, glyph),
                ));
                glyph = start;
                cluster = next;
            }
        }
        out
    }

    fn make_cluster(&self, ri: usize, start: usize, end: usize, tr: Range, width: f32) -> Cluster {
        let mut ws = 0;
        let mut intra = 0;
        let mut ideographic = false;
        let text = self.text.as_bytes();
        if tr.width() == 1 && text[tr.start] <= 0x7F {
            if matches!(text[tr.start], b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') {
                ws += 1;
            }
        } else {
            for i in tr.start..tr.end {
                if self.has_flag(i, flag::WHITESPACE_BREAK) {
                    ws += 1;
                }
                if self.has_flag(i, flag::INTRA_WORD_BREAK) {
                    intra += 1;
                }
                if self.has_flag(i, flag::IDEOGRAPHIC) {
                    ideographic = true;
                }
            }
        }
        Cluster {
            run_index: ri,
            text_range: tr,
            start,
            end,
            width,
            half_letter_spacing: 0.0,
            is_whitespace_break: ws == tr.width(),
            is_intra_word_break: intra == tr.width(),
            is_hard_break: self.has_flag(tr.end, flag::HARD_LINE_BREAK_BEFORE),
            is_ideographic: ideographic,
        }
    }

    /// `resolveStrut`.
    fn resolve_strut(&mut self) {
        let strut = &self.style.strut;
        if !strut.enabled || strut.font_size < 0.0 {
            return;
        }
        let Some(face) = self
            .fonts
            .find(&strut.font_families, strut.font_style)
            .into_iter()
            .next()
        else {
            return;
        };
        let tf = Typeface::new(&face, &[]);
        let m = FontMetrics::of(&tf, strut.font_size);
        let leading = if strut.leading < 0.0 {
            0.0
        } else {
            strut.leading * strut.font_size
        };
        self.strut_metrics = if strut.height_override {
            let (a, d) = if strut.half_leading {
                let occupied = m.descent - m.ascent;
                let flexible = (strut.height * strut.font_size - occupied) / 2.0;
                (m.ascent - flexible, m.descent + flexible)
            } else {
                let mh = m.descent - m.ascent + m.leading;
                let mult = if mh == 0.0 {
                    strut.height
                } else {
                    strut.height * strut.font_size / mh
                };
                (m.ascent * mult, m.descent * mult)
            };
            LineMetrics {
                ascent: a,
                descent: d,
                leading,
                raw_ascent: m.ascent,
                raw_descent: m.descent,
                raw_leading: m.leading,
                force_strut: false,
            }
        } else {
            LineMetrics::new(m.ascent, m.descent, leading)
        };
        self.strut_metrics.force_strut = strut.force_height;
    }

    /// `computeEmptyMetrics`.
    fn compute_empty_metrics(&mut self) {
        let empty = self.runs.is_empty();
        let mut ts = self.style.text_style.clone();
        if empty {
            if let Some(b) = self.blocks.last() {
                ts = b.style.clone();
            }
        }
        let m = self
            .fonts
            .find(&ts.font_families, ts.font_style)
            .first()
            .map(|f| FontMetrics::of(&Typeface::new(f, &ts.variations()), ts.font_size))
            .unwrap_or_default();
        let force = self.style.strut.force_height;
        self.empty_metrics = LineMetrics::from_font(&m, force);
        if !force && ts.height_override {
            let intrinsic = self.empty_metrics.height();
            let strut_height = ts.height * ts.font_size;
            let e = &mut self.empty_metrics;
            if self.style.strut.half_leading {
                e.leading += strut_height - intrinsic;
            } else if intrinsic != 0.0 {
                let mult = strut_height / intrinsic;
                e.ascent *= mult;
                e.descent *= mult;
                e.leading *= mult;
            }
        }
        if empty {
            let hb = self.style.text_height_behavior;
            let e = &mut self.empty_metrics;
            if hb & crate::style::height_behavior::DISABLE_FIRST_ASCENT != 0 {
                e.ascent = e.raw_ascent;
            }
            if hb & crate::style::height_behavior::DISABLE_LAST_DESCENT != 0 {
                e.descent = e.raw_descent;
            }
        }
        if self.style.strut.enabled {
            let s = self.strut_metrics;
            s.update_line(&mut self.empty_metrics);
        }
    }

    /// `breakShapedTextIntoLines`.
    fn break_shaped_text_into_lines(&mut self, max_width: f32) {
        if !self.has_line_breaks
            && !self.has_whitespaces_inside
            && self.placeholders.len() == 1
            && self.runs.len() == 1
            && self.runs[0].advance[0] <= max_width
        {
            let run = self.runs[0].clone();
            let mut advance = run.advance();
            let text_range = Range::new(0, self.text.len());
            let excluding = Range::new(0, self.trailing_spaces);
            let mut metrics = LineMetrics::clean_with(self.style.strut_force_height());
            metrics.add_run(&run);
            let hb = self.style.text_height_behavior;
            if hb & crate::style::height_behavior::DISABLE_FIRST_ASCENT != 0 {
                metrics.ascent = metrics.raw_ascent;
            }
            if hb & crate::style::height_behavior::DISABLE_LAST_DESCENT != 0 {
                metrics.descent = metrics.raw_descent;
            }
            if self.strut_enabled() {
                self.strut_metrics.update_line(&mut metrics);
            }
            let mut trailing = self.clusters.len();
            loop {
                trailing -= 1;
                let c = &self.clusters[trailing];
                if !c.is_whitespace_break {
                    trailing += 1;
                    break;
                }
                advance[0] -= c.width;
                if trailing == 0 {
                    break;
                }
            }
            advance[1] = metrics.height();
            let clusters = Range::new(0, trailing);
            let ghosts = Range::new(0, self.clusters.len() - 1);
            let line = TextLine::from_spec(
                self,
                crate::wrapper::LineSpec {
                    text_excluding_spaces: excluding,
                    text: text_range,
                    text_with_newlines: text_range,
                    clusters,
                    clusters_with_ghosts: ghosts,
                    width_with_spaces: run.advance[0],
                    offset: [0.0, 0.0],
                    advance,
                    metrics,
                    ellipsis: false,
                },
            );
            self.lines.push(line);
            self.longest_line = if advance[0].abs() <= f32::EPSILON {
                run.advance[0]
            } else {
                advance[0]
            };
            self.height = advance[1];
            self.width = max_width;
            self.max_intrinsic_width = run.advance[0];
            self.min_intrinsic_width = advance[0];
            self.alphabetic_baseline = self.lines[0].sizes.alphabetic_baseline();
            self.ideographic_baseline = self.lines[0].sizes.ideographic_baseline();
            self.exceeded_max_lines = false;
            return;
        }
        crate::wrapper::break_text_into_lines(self, max_width);
        self.width = max_width;
        self.alphabetic_baseline = self
            .lines
            .first()
            .map_or(self.empty_metrics.alphabetic_baseline(), |l| {
                l.sizes.alphabetic_baseline()
            });
        self.ideographic_baseline = self
            .lines
            .first()
            .map_or(self.empty_metrics.ideographic_baseline(), |l| {
                l.sizes.ideographic_baseline()
            });
    }

    /// `formatLines`.
    fn format_lines(&mut self, max_width: f32) {
        let align = self.style.effective_align();
        let left = align == crate::style::TextAlign::Left
            || (align == crate::style::TextAlign::Justify
                && self.style.text_direction == crate::style::TextDirection::Ltr);
        if !max_width.is_finite() && !left {
            self.lines.clear();
            return;
        }
        for i in 0..self.lines.len() {
            crate::line::format(self, i, align, max_width);
        }
    }

    /// `getWords`: the word boundaries in UTF-16, as ICU gives them.
    pub(crate) fn words(&mut self) -> &[usize] {
        if self.words.is_empty() {
            self.words = unicode::word_boundaries(&self.text)
                .into_iter()
                .map(|i| self.utf16_index(i))
                .collect();
        }
        &self.words
    }
}

/// A resolved piece of a shaping: some of its glyphs, and their text.
#[derive(Clone)]
struct Piece {
    tf: Typeface,
    shaped: Arc<Shaped>,
    text: Range,
    glyphs: Range,
    /// Where the shaped text started.
    shaped_start: usize,
}

impl Piece {
    /// The glyphs of this piece's shaping covering `text`.
    fn glyphs_for(&self, text: Range, rtl: bool) -> Option<Range> {
        let s = &self.shaped;
        let n = s.glyphs.len();
        let inside = |i: usize| {
            let c = self.shaped_start + s.clusters[i] as usize;
            c >= text.start && c < text.end
        };
        let first = (0..n).find(|i| inside(*i))?;
        let last = (0..n).rev().find(|i| inside(*i))?;
        let _ = rtl;
        Some(Range::new(first, last + 1))
    }
}

/// A block's font features over `range`, and `liga` off with letter
/// spacing (iterateThroughFontStyles).
fn block_features(b: &Block, range: Range) -> Vec<FeatureSpan> {
    let mut f: Vec<FeatureSpan> = b
        .style
        .font_features
        .iter()
        .map(|(t, v)| (*t, u32::try_from(*v).unwrap_or(0), range))
        .collect();
    if b.style.letter_spacing > 0.0 {
        f.push((*b"liga", 0, range));
    }
    f
}
