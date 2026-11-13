//! What SkParagraph asks of SkUnicode (SkUnicode_icu.cpp), from the Unicode
//! crates instead of ICU: per-code-unit flags, bidi regions and visual
//! order, word boundaries, and the UTF-8/UTF-16 index maps.
//!
//! The character classes are ICU's: `u_isspace`, `u_isWhitespace`,
//! `u_iscntrl` and the Ideographic property, for the ranges that occur in
//! text. Line breaks are UAX #14 (unicode-linebreak), graphemes and words
//! UAX #29 (unicode-segmentation), each with the tailoring Flutter's ICU
//! data carries from Chromium: lines as CSS `line-break: normal`, words
//! broken at full stops between letters. Not yet: ICU's dictionaries for
//! Thai, Lao, Khmer and Burmese, which break words and lines inside a run
//! of those scripts.

use std::borrow::Cow;

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
    for (pos, op) in unicode_linebreak::linebreaks(&as_line_normal(text)) {
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

/// `getWords`: the word boundaries, UAX #29 as Chromium's ICU tailors
/// it ([`as_chromium_words`]), in UTF-8 bytes.
pub(crate) fn word_boundaries(text: &str) -> Vec<usize> {
    let mut v: Vec<usize> = as_chromium_words(text)
        .split_word_bound_indices()
        .map(|(i, _)| i)
        .collect();
    v.push(text.len());
    v
}

/// `text` with each character in `swap` replaced by its stand-in: one of
/// the class the tailoring gives it and of the same UTF-8 length, so every
/// boundary falls at the same byte. Borrowed when nothing is swapped.
fn swapped(text: &str, swap: impl Fn(char) -> Option<char>) -> Cow<'_, str> {
    if !text.chars().any(|c| swap(c).is_some()) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .map(|c| match swap(c) {
                Some(s) if s.len_utf8() == c.len_utf8() => s,
                _ => c,
            })
            .collect(),
    )
}

/// The line-break classes of ICU's root line iterator as Flutter ships it,
/// `line_normal` (Chromium's line_normal.patch): CSS `line-break: normal`,
/// where the conditional Japanese starters (small kana, the prolonged
/// sound mark) are ideographs, so a line may start with one. UAX #14 as
/// written makes them nonstarters, as `line-break: strict` does. Each is
/// three bytes, as U+4E00 is.
fn as_line_normal(text: &str) -> Cow<'_, str> {
    swapped(text, |c| {
        (unicode_linebreak::break_property(c as u32)
            == unicode_linebreak::BreakClass::ConditionalJapaneseStarter)
            .then_some('\u{4E00}')
    })
}

/// The word-break classes of ICU's word iterator as Flutter ships it
/// (Chromium's wordbrk.patch): the full stops are MidNum, not MidNumLet,
/// so a hostname or "e.g." breaks at its dots while "3.14" holds, and the
/// colons are not MidLetter. The stand-ins: the commas, MidNum, and
/// characters of no word-break class.
fn as_chromium_words(text: &str) -> Cow<'_, str> {
    swapped(text, |c| match c {
        '.' => Some(','),
        '\u{FF0E}' => Some('\u{FF0C}'),
        ':' => Some('!'),
        '\u{FE55}' => Some('\u{FE56}'),
        '\u{FF1A}' => Some('\u{FF01}'),
        _ => None,
    })
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

/// The script runs of `text`, as SkShaper's HarfBuzz script iterator
/// (`SkUnicodeHbScriptRunIterator`) gives them: a run takes the script of
/// its first character of a real script, Common and Inherited characters
/// joining the run they are in; a run of only those is Common. Each is
/// its byte range and ISO 15924 tag.
pub(crate) fn script_runs(text: &str) -> Vec<(std::ops::Range<usize>, [u8; 4])> {
    use harfrust::Script;
    let script = |c: char| harfrust::unicode::script(c as u32);
    let tag = |s: Script| s.tag().to_be_bytes();
    let mut out = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        let mut current = script(c);
        let mut end = start + c.len_utf8();
        while let Some(&(i, c)) = chars.peek() {
            let s = script(c);
            if s != current {
                if current == Script::INHERITED || current == Script::COMMON {
                    current = s;
                } else if s != Script::INHERITED && s != Script::COMMON {
                    break;
                }
            }
            end = i + c.len_utf8();
            chars.next();
        }
        if current == Script::INHERITED {
            current = Script::COMMON;
        }
        out.push((start..end, tag(current)));
    }
    out
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
    fn script_runs_split_where_skshaper_splits() {
        fn tags(t: &str) -> Vec<(&str, String)> {
            script_runs(t)
                .into_iter()
                .map(|(r, s)| (&t[r], std::str::from_utf8(&s).unwrap().to_owned()))
                .collect()
        }
        // Common text joins the run it is in; a run takes its first real
        // script.
        assert_eq!(
            tags("(abc) 123 \u{65E5}\u{672C}!"),
            [
                ("(abc) 123 ", "Latn".to_owned()),
                ("\u{65E5}\u{672C}!", "Hani".to_owned()),
            ]
        );
        // Only Common, or a mark alone, is Common.
        assert_eq!(tags("12 34"), [("12 34", "Zyyy".to_owned())]);
        assert_eq!(tags("\u{301}"), [("\u{301}", "Zyyy".to_owned())]);
        // A mark stays with its base; Kana and Han are runs apart, as the
        // iterator has no Japanese merging.
        assert_eq!(
            tags("e\u{301}\u{30AB}\u{6F22}"),
            [
                ("e\u{301}", "Latn".to_owned()),
                ("\u{30AB}", "Kana".to_owned()),
                ("\u{6F22}", "Hani".to_owned()),
            ]
        );
        assert!(script_runs("").is_empty());
    }

    #[test]
    fn words_break_at_full_stops_between_letters_as_chromium_tailors() {
        let words = |t: &str| {
            let b = word_boundaries(t);
            b.windows(2)
                .map(|w| t[w[0]..w[1]].to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(words("e.g."), ["e", ".", "g", "."]);
        assert_eq!(words("example.com"), ["example", ".", "com"]);
        // Between digits a full stop is MidNum, so a number holds.
        assert_eq!(words("3.14"), ["3.14"]);
        assert_eq!(words("a:b"), ["a", ":", "b"]);
        // Other MidNumLet and MidLetter characters still join.
        assert_eq!(words("don't"), ["don't"]);
    }

    #[test]
    fn lines_may_start_with_small_kana_as_css_normal_allows() {
        // \u{30C3} (small tsu) and \u{30FC} (prolonged sound mark) are
        // conditional Japanese starters.
        for t in ["\u{30AB}\u{30C3}\u{30C8}", "\u{30CA}\u{30FC}"] {
            let mut s = String::from(t);
            let f = code_unit_flags(&mut s);
            assert!(f[3] & flag::SOFT_LINE_BREAK_BEFORE != 0, "{t}");
        }
    }

    #[test]
    fn utf16_maps_round_trip() {
        let (a, b) = utf_maps("a\u{1F600}b");
        assert_eq!(a, [0, 1, 1, 5, 6]);
        assert_eq!(b, [0, 1, 1, 1, 1, 3, 4]);
    }
}
