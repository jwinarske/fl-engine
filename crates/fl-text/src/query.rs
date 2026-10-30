//! What dart:ui asks of a laid out paragraph: its measures, and the
//! queries `ParagraphImpl` answers, in UTF-16 as Flutter's engine passes
//! them.

use crate::line::{
    self, LineMetricsInfo, PositionWithAffinity, RectHeightStyle, RectWidthStyle, TextBox,
};
use crate::paragraph::{Paragraph, Range, EMPTY_INDEX};

/// `GlyphInfo`: the grapheme's bounds (left, top, right, bottom), its
/// UTF-16 range, and whether left to right.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphInfo {
    pub rect: [f32; 4],
    pub start: usize,
    pub end: usize,
    pub ltr: bool,
}

impl Paragraph {
    /// The text's length in bytes.
    pub fn text_len(&self) -> usize {
        self.text.len()
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn longest_line(&self) -> f32 {
        self.longest_line
    }

    pub fn min_intrinsic_width(&self) -> f32 {
        self.min_intrinsic_width
    }

    pub fn max_intrinsic_width(&self) -> f32 {
        self.max_intrinsic_width
    }

    pub fn alphabetic_baseline(&self) -> f32 {
        self.alphabetic_baseline
    }

    pub fn ideographic_baseline(&self) -> f32 {
        self.ideographic_baseline
    }

    pub fn did_exceed_max_lines(&self) -> bool {
        self.exceeded_max_lines
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The code points no font had, for the missing-glyph report.
    pub fn unresolved_glyphs(&self) -> usize {
        if self.text.is_empty() {
            0
        } else {
            self.unresolved.len()
        }
    }

    /// `getRectsForRange`, the range in UTF-16.
    pub fn rects_for_range(
        &self,
        start: usize,
        end: usize,
        hs: RectHeightStyle,
        ws: RectWidthStyle,
    ) -> Vec<TextBox> {
        let mut out = Vec::new();
        if self.text.is_empty() {
            if start == 0 && end > 0 {
                out.push(TextBox {
                    rect: [0.0, 0.0, 0.0, self.height],
                    direction: self.style.text_direction,
                });
            }
            return out;
        }
        let n16 = self.utf16_len();
        if start >= end || start > n16 || end == 0 {
            return out;
        }
        let mut text = Range::new(self.text.len(), self.text.len());
        if start < n16 {
            let mut utf8 = self.utf8_index(start).unwrap_or(0);
            if start > 0 && self.utf8_index(start - 1) == Some(utf8) {
                utf8 = self.utf8_index(start + 1).unwrap_or(utf8);
            }
            text.start = self.next_grapheme(utf8);
        }
        if end < n16 {
            text.end = self.previous_grapheme(self.utf8_index(end).unwrap_or(0));
        }
        for li in 0..self.lines.len() {
            let lt = self.lines[li].text_with_newlines;
            let intersect = lt.intersect(text);
            if (intersect.start == EMPTY_INDEX || intersect.is_empty()) && lt.start != text.start {
                continue;
            }
            line::rects_for_range(self, li, intersect, hs, ws, &mut out);
        }
        out
    }

    /// `getRectsForPlaceholders`.
    pub fn rects_for_placeholders(&self) -> Vec<TextBox> {
        let mut out = Vec::new();
        if self.text.is_empty() || self.placeholders.len() == 1 {
            return out;
        }
        for li in 0..self.lines.len() {
            line::rects_for_placeholders(self, li, &mut out);
        }
        out
    }

    /// `getGlyphPositionAtCoordinate`.
    pub fn position_at(&self, dx: f32, dy: f32) -> PositionWithAffinity {
        let none = PositionWithAffinity {
            position: 0,
            upstream: false,
        };
        if self.text.is_empty() {
            return none;
        }
        let last = self.lines.len().saturating_sub(1);
        for li in 0..self.lines.len() {
            let l = &self.lines[li];
            if dy >= l.offset()[1] + l.height() && li != last {
                continue;
            }
            return line::glyph_position_at(self, li, dx);
        }
        none
    }

    /// `getWordBoundary`, in UTF-16.
    pub fn word_boundary(&mut self, offset: usize) -> (usize, usize) {
        let mut start = 0;
        let mut end = 0;
        for &w in self.words() {
            if w <= offset {
                start = w;
                end = w;
            } else {
                end = w;
                break;
            }
        }
        (start, end)
    }

    /// `getLineNumberAt`, `utf8` in bytes.
    fn line_number_at_utf8(&self, utf8: usize) -> Option<usize> {
        if utf8 >= self.text.len() || self.lines.is_empty() {
            return None;
        }
        let mut lo = 0usize;
        let mut hi = self.lines.len() - 1;
        if self.lines[hi].text_with_newlines.end <= utf8 {
            return None;
        }
        while hi > lo {
            let mid = (hi + lo) / 2;
            let r = self.lines[mid].text_with_newlines;
            if utf8 < r.start {
                hi = mid.checked_sub(1)?;
            } else if r.end <= utf8 {
                lo = mid + 1;
            } else {
                return Some(mid);
            }
        }
        Some(lo)
    }

    /// `getLineNumberAtUTF16Offset`.
    pub fn line_number_at(&self, utf16: usize) -> Option<usize> {
        self.line_number_at_utf8(self.utf8_index(utf16)?)
    }

    /// `getLineMetrics`, all lines.
    pub fn line_metrics(&self) -> Vec<LineMetricsInfo> {
        (0..self.lines.len())
            .map(|li| line::metrics(self, li))
            .collect()
    }

    /// `getLineMetricsAt`.
    pub fn line_metrics_at(&self, li: usize) -> Option<LineMetricsInfo> {
        (li < self.lines.len()).then(|| line::metrics(self, li))
    }

    /// dart:ui's `getLineBoundary`: the first line whose start to end
    /// holds the offset, else (-1, -1).
    pub fn line_boundary(&self, utf16: usize) -> (i64, i64) {
        for m in self.line_metrics() {
            if utf16 >= m.start_index && utf16 <= m.end_index {
                return (m.start_index as i64, m.end_index as i64);
            }
        }
        (-1, -1)
    }

    /// `getGlyphInfoAtUTF16Offset`.
    pub fn glyph_info_at(&self, utf16: usize) -> Option<GlyphInfo> {
        let utf8 = self.utf8_index(utf16)?;
        let li = self.line_number_at_utf8(utf8)?;
        let start = self.previous_grapheme(utf8);
        let end = self.next_grapheme(utf8 + 1);
        let mut boxes = Vec::new();
        line::rects_for_range(
            self,
            li,
            Range::new(start, end),
            RectHeightStyle::Tight,
            RectWidthStyle::Tight,
            &mut boxes,
        );
        let b = boxes.first()?;
        Some(GlyphInfo {
            rect: b.rect,
            start: self.utf16_index(start),
            end: self.utf16_index(end),
            ltr: b.direction == crate::style::TextDirection::Ltr,
        })
    }

    /// `getClosestUTF16GlyphInfoAt`.
    pub fn closest_glyph_info(&self, dx: f32, dy: f32) -> Option<GlyphInfo> {
        let p = self.position_at(dx, dy);
        let utf16 = if p.upstream {
            p.position.checked_sub(1)?
        } else {
            p.position
        };
        self.glyph_info_at(utf16)
    }
}
