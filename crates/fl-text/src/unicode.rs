//! What SkParagraph asks of SkUnicode (SkUnicode_icu.cpp), from the Unicode
//! crates instead of ICU: per-code-unit flags, bidi regions and visual
//! order, word boundaries, and the UTF-8/UTF-16 index maps.
//!
//! The character classes are ICU's: `u_isspace`, `u_isWhitespace`,
//! `u_iscntrl` and the Ideographic property, for the ranges that occur in
//! text. Line breaks are UAX #14 (unicode-linebreak), graphemes and words
//! UAX #29 (unicode-segmentation); ICU tailors both slightly, which shows
//! only at the edges of those algorithms.

use unicode_bidi::{BidiInfo, Level};
use unicode_segmentation::UnicodeSegmentation;

/// `SkUnicode::CodeUnitFlags`.
pub(crate) mod flag {
    pub const WHITESPACE_BREAK: u16 = 0x1;
    pub const INTRA_WORD_BREAK: u16 = 0x2;
    pub const GRAPHEME_START: u16 = 0x4;
    pub const SOFT_LINE_BREAK_BEFORE: u16 = 0x8;
    pub const HARD_LINE_BREAK_BEFORE: u16 = 0x10;
    pub const CONTROL: u16 = 0x20;
    pub const TABULATION: u16 = 0x40;
    pub const GLYPH_CLUSTER_START: u16 = 0x80;
    pub const IDEOGRAPHIC: u16 = 0x100;
}

/// The space separators (Zs), and the line and paragraph separators.
fn is_separator(c: char) -> bool {
    matches!(
        c,
        '\u{20}' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

/// `u_isspace`: separators and the POSIX spaces.
pub(crate) fn is_space(c: char) -> bool {
    is_separator(c) || matches!(c, '\t' | '\n' | '\u{B}' | '\u{C}' | '\r')
}

/// `u_isWhitespace`: Java's, separators except the no-break ones, the
/// POSIX spaces and U+001C to U+001F.
pub(crate) fn is_whitespace(c: char) -> bool {
    (is_separator(c) && !matches!(c, '\u{A0}' | '\u{2007}' | '\u{202F}'))
        || matches!(
            c,
            '\t' | '\n' | '\u{B}' | '\u{C}' | '\r' | '\u{1C}'..='\u{1F}'
        )
}

/// `u_iscntrl`: Cc, Cf, and the line and paragraph separators.
pub(crate) fn is_control(c: char) -> bool {
    matches!(
        c,
        '\u{0}'..='\u{1F}'
            | '\u{7F}'..='\u{9F}'
            | '\u{AD}'
            | '\u{600}'..='\u{605}'
            | '\u{61C}'
            | '\u{6DD}'
            | '\u{70F}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

/// The Ideographic property.
pub(crate) fn is_ideographic(c: char) -> bool {
    matches!(
        c,
        '\u{3006}'
            | '\u{3007}'
            | '\u{3021}'..='\u{3029}'
            | '\u{3038}'..='\u{303A}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FA6D}'
            | '\u{FA70}'..='\u{FAD9}'
            | '\u{16FE4}'
            | '\u{17000}'..='\u{187F7}'
            | '\u{18800}'..='\u{18CD5}'
            | '\u{18D00}'..='\u{18D08}'
            | '\u{1B170}'..='\u{1B2FB}'
            | '\u{20000}'..='\u{2A6DF}'
            | '\u{2A700}'..='\u{2EBE0}'
            | '\u{2F800}'..='\u{2FA1D}'
            | '\u{30000}'..='\u{323AF}'
    )
}

/// The hard line breaks: line feed and the mandatory-break class.
pub(crate) fn is_hard_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\u{B}' | '\u{C}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// `computeCodeUnitFlags` for UTF-8 `text`, tabs replaced by spaces in
/// place (`setReplaceTabCharacters(true)`): one entry per byte and one past
/// the end.
pub(crate) fn code_unit_flags(text: &mut String) -> Vec<u16> {
    let mut flags = vec![0u16; text.len() + 1];
    for (pos, op) in unicode_linebreak::linebreaks(text) {
        let hard = op == unicode_linebreak::BreakOpportunity::Mandatory
            && text[..pos].chars().next_back().is_some_and(is_hard_break);
        flags[pos] |= if hard {
            flag::HARD_LINE_BREAK_BEFORE
        } else {
            flag::SOFT_LINE_BREAK_BEFORE
        };
    }
    for (pos, _) in text.grapheme_indices(true) {
        flags[pos] |= flag::GRAPHEME_START;
    }
    flags[text.len()] |= flag::GRAPHEME_START;
    let mut tabs = Vec::new();
    for (pos, c) in text.char_indices() {
        let mut c = c;
        if c == '\t' {
            flags[pos] |= flag::TABULATION;
            tabs.push(pos);
            c = ' ';
        }
        let mut f = 0;
        if is_space(c) {
            f |= flag::INTRA_WORD_BREAK;
        }
        if is_whitespace(c) {
            f |= flag::WHITESPACE_BREAK;
        }
        if is_control(c) {
            f |= flag::CONTROL;
        }
        if is_ideographic(c) {
            f |= flag::IDEOGRAPHIC;
        }
        for fl in &mut flags[pos..pos + c.len_utf8()] {
            *fl |= f;
        }
    }
    if !tabs.is_empty() {
        // A tab is one byte, as a space is: replaced in place.
        let mut bytes = std::mem::take(text).into_bytes();
        for pos in tabs {
            bytes[pos] = b' ';
        }
        *text = String::from_utf8(bytes).unwrap_or_default();
    }
    flags
}

/// A stretch of text at one bidi level, in UTF-8 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BidiRegion {
    pub start: usize,
    pub end: usize,
    pub level: u8,
}

/// `getBidiRegions`: the text's runs of one embedding level, the paragraph
/// at `rtl`'s level.
pub(crate) fn bidi_regions(text: &str, rtl: bool) -> Vec<BidiRegion> {
    if text.is_empty() {
        return Vec::new();
    }
    let level = if rtl { Level::rtl() } else { Level::ltr() };
    let info = BidiInfo::new(text, Some(level));
    let mut regions: Vec<BidiRegion> = Vec::new();
    for (pos, _) in text.char_indices() {
        let l = info.levels.get(pos).map_or(level.number(), |l| l.number());
        match regions.last_mut() {
            Some(r) if r.level == l => r.end = pos,
            _ => regions.push(BidiRegion {
                start: pos,
                end: pos,
                level: l,
            }),
        }
    }
    // Ends were left at each region's last char start; close them.
    let n = regions.len();
    for i in 0..n {
        regions[i].end = if i + 1 < n {
            regions[i + 1].start
        } else {
            text.len()
        };
    }
    regions
}

/// `reorderVisual`: the logical index of each run, left to right.
pub(crate) fn reorder_visual(levels: &[u8]) -> Vec<usize> {
    let levels: Vec<Level> = levels
        .iter()
        .map(|l| Level::new(*l).unwrap_or_else(|_| Level::ltr()))
        .collect();
    BidiInfo::reorder_visual(&levels)
}

/// `getWords`: the word boundaries, UAX #29, in UTF-8 bytes.
pub(crate) fn word_boundaries(text: &str) -> Vec<usize> {
    let mut v: Vec<usize> = text.split_word_bound_indices().map(|(i, _)| i).collect();
    v.push(text.len());
    v
}

/// The UTF-8 index of each UTF-16 index and one past the end, and the
/// UTF-16 index of each UTF-8 index and one past the end
/// (`extractUtfConversionMapping`).
pub(crate) fn utf_maps(text: &str) -> (Vec<usize>, Vec<usize>) {
    let mut utf8_of_16 = Vec::with_capacity(text.len() + 1);
    let mut utf16_of_8 = Vec::with_capacity(text.len() + 1);
    let mut u16i = 0;
    for (pos, c) in text.char_indices() {
        for _ in 0..c.len_utf16() {
            utf8_of_16.push(pos);
        }
        for _ in 0..c.len_utf8() {
            utf16_of_8.push(u16i);
        }
        u16i += c.len_utf16();
    }
    utf8_of_16.push(text.len());
    utf16_of_8.push(u16i);
    (utf8_of_16, utf16_of_8)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn spaces_break_softly_and_newlines_hard() {
        let mut t = String::from("ab cd\nef");
        let f = code_unit_flags(&mut t);
        assert!(f[2] & flag::WHITESPACE_BREAK != 0);
        assert!(f[3] & flag::SOFT_LINE_BREAK_BEFORE != 0);
        assert!(f[6] & flag::HARD_LINE_BREAK_BEFORE != 0);
        // The end is a soft break, as ICU reports it, not a hard one.
        assert!(f[8] & flag::SOFT_LINE_BREAK_BEFORE != 0);
        assert!(f[8] & flag::HARD_LINE_BREAK_BEFORE == 0);
        assert!(f[0] & flag::GRAPHEME_START != 0);
    }

    #[test]
    fn tabs_become_spaces() {
        let mut t = String::from("a\tb");
        let f = code_unit_flags(&mut t);
        assert_eq!(t, "a b");
        assert!(f[1] & flag::TABULATION != 0 && f[1] & flag::WHITESPACE_BREAK != 0);
    }

    #[test]
    fn no_break_space_is_space_but_not_whitespace() {
        assert!(is_space('\u{A0}') && !is_whitespace('\u{A0}'));
        assert!(is_whitespace(' ') && is_space(' '));
    }

    #[test]
    fn bidi_regions_split_by_level() {
        let r = bidi_regions("abc \u{5D0}\u{5D1}", false);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].level, r[1].level), (0, 1));
        assert_eq!(r[1].end, "abc \u{5D0}\u{5D1}".len());
    }

    #[test]
    fn utf16_maps_round_trip() {
        let (a, b) = utf_maps("a\u{1F600}b");
        assert_eq!(a, [0, 1, 1, 5, 6]);
        assert_eq!(b, [0, 1, 1, 1, 1, 3, 4]);
    }
}
