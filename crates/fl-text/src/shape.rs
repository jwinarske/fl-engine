//! Shaping one stretch of text in one typeface, as SkShaper's HarfBuzz
//! shaper does it for SkParagraph (`ShapeDontWrapOrReorder`): the font
//! scaled to its size in 16.16 fixed point, clusters monotone by character,
//! glyphs in visual order, positions accumulated from the advances and
//! offsets kept beside them.
//!
//! This is the one place shaping happens, so changing shapers is one file
//! (brief M3 §15).

use harfrust::{Buffer, ClusterLevel, Direction, Feature, Language, ShapeOptions, ShaperFont, Tag};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::font::Typeface;

/// One run as shaped: `glyphs.len() + 1` positions and offsets, the last
/// position the pen after the run; cluster indices into the shaped text, in
/// UTF-8 bytes, the last the run's end (LTR) or start (RTL).
#[derive(Debug, Clone, Default)]
pub(crate) struct Shaped {
    pub glyphs: Vec<u32>,
    pub positions: Vec<[f32; 2]>,
    pub offsets: Vec<[f32; 2]>,
    pub clusters: Vec<u32>,
    pub advance: f32,
}

/// A font's line metrics at a size, as `SkFont::getMetrics` gives them:
/// ascent negative, descent positive, the leading the line gap.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct FontMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub leading: f32,
    /// Below the baseline positive, and each only if the font states it,
    /// as Skia's `k*IsValid` flags say.
    pub underline_position: Option<f32>,
    pub underline_thickness: Option<f32>,
    /// Above the baseline negative.
    pub strikeout_position: Option<f32>,
    pub strikeout_thickness: Option<f32>,
    pub x_height: f32,
}

/// 16.16 fixed point, SkShaper's `skhb_position`.
fn fixed(v: f32) -> i32 {
    (v * 65536.0)
        .round()
        .clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

/// The longest text shaped in one call, in bytes (brief M3 §15): past it
/// text is shaped in pieces, split after a space where there is one.
pub(crate) const MAX_SHAPE_BYTES: usize = 32 * 1024;

/// The shaped runs kept (brief M3 §15): past it the cache starts over.
const CACHE_ENTRIES: usize = 4096;

/// What a shaping depends on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    face: u64,
    variations: Vec<([u8; 4], u32)>,
    size: u32,
    text: String,
    rtl: bool,
    locale: String,
    features: Vec<([u8; 4], u32, usize, usize)>,
}

static CACHE: Mutex<Option<HashMap<Key, Arc<Shaped>>>> = Mutex::new(None);

/// Forgets every shaped run, for when the faces they were shaped in go:
/// a run is keyed by its face's id, and a face loaded again is a new id,
/// so the old runs would only wait for the bound to clear them.
pub fn clear_shaped_runs() {
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// `text` (UTF-8) shaped in `tf` at `size`, right to left if `rtl`, with
/// `features` (tag, value, and the byte range within `text`). Cached by
/// all of those; a panic in the shaper is the text unshaped, so
/// unresolved.
pub(crate) fn shape(
    tf: &Typeface,
    size: f32,
    text: &str,
    rtl: bool,
    locale: &str,
    features: &[([u8; 4], u32, std::ops::Range<usize>)],
) -> Arc<Shaped> {
    let key = Key {
        face: tf.face.id,
        variations: tf
            .variations
            .iter()
            .map(|(t, v)| (*t, v.to_bits()))
            .collect(),
        size: size.to_bits(),
        text: text.to_owned(),
        rtl,
        locale: locale.to_owned(),
        features: features
            .iter()
            .map(|(t, v, r)| (*t, *v, r.start, r.end))
            .collect(),
    };
    if let Some(hit) = CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|c| c.get(&key))
    {
        return Arc::clone(hit);
    }
    let shaped = Arc::new(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            shape_pieces(tf, size, text, rtl, locale, features)
        }))
        .unwrap_or_else(|_| empty(text, rtl)),
    );
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let c = cache.get_or_insert_with(HashMap::new);
    if c.len() >= CACHE_ENTRIES {
        c.clear();
    }
    c.insert(key, Arc::clone(&shaped));
    shaped
}

/// Where `text` is split to be shaped in pieces of at most
/// [`MAX_SHAPE_BYTES`]: after the last space in each piece, else at the
/// last character boundary.
pub(crate) fn pieces(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    while text.len() - start > MAX_SHAPE_BYTES {
        let mut end = start + MAX_SHAPE_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        if let Some(sp) = text[start..end].rfind(' ') {
            if sp > 0 {
                end = start + sp + 1;
            }
        }
        out.push(start..end);
        start = end;
    }
    out.push(start..text.len());
    out
}

fn shape_pieces(
    tf: &Typeface,
    size: f32,
    text: &str,
    rtl: bool,
    locale: &str,
    features: &[([u8; 4], u32, std::ops::Range<usize>)],
) -> Shaped {
    let ranges = pieces(text);
    if ranges.len() == 1 {
        return shape_one(tf, size, text, rtl, locale, features);
    }
    let mut parts: Vec<(usize, Shaped)> = ranges
        .into_iter()
        .map(|r| {
            let f: Vec<_> = features
                .iter()
                .filter(|(_, _, fr)| fr.start < r.end && fr.end > r.start)
                .map(|(t, v, fr)| {
                    (
                        *t,
                        *v,
                        fr.start.max(r.start) - r.start..fr.end.min(r.end) - r.start,
                    )
                })
                .collect();
            (
                r.start,
                shape_one(tf, size, &text[r.clone()], rtl, locale, &f),
            )
        })
        .collect();
    if rtl {
        // Right to left, the later text is drawn first.
        parts.reverse();
    }
    let mut out = Shaped::default();
    let mut x = 0.0f32;
    for (start, p) in parts {
        for i in 0..p.glyphs.len() {
            out.glyphs.push(p.glyphs[i]);
            out.positions
                .push([p.positions[i][0] + x, p.positions[i][1]]);
            out.offsets.push(p.offsets[i]);
            out.clusters.push(p.clusters[i] + start as u32);
        }
        x += p.advance;
    }
    out.positions.push([x, 0.0]);
    out.offsets.push([0.0, 0.0]);
    out.clusters.push(if rtl { 0 } else { text.len() as u32 });
    out.advance = x;
    out
}

/// No glyphs: what a face that cannot shape `text` gives.
fn empty(text: &str, rtl: bool) -> Shaped {
    Shaped {
        glyphs: Vec::new(),
        positions: vec![[0.0, 0.0]],
        offsets: vec![[0.0, 0.0]],
        clusters: vec![if rtl { 0 } else { text.len() as u32 }],
        advance: 0.0,
    }
}

/// A feature as SkShaper hands it to HarfBuzz: global when it covers the
/// whole text, else its byte range, end exclusive. (harfrust's
/// `Feature::new` takes an exclusive end one short, so the struct is built
/// here.)
fn feature(tag: &[u8; 4], value: u32, r: &std::ops::Range<usize>, len: usize) -> Feature {
    let (start, end) = if r.start == 0 && r.end >= len {
        (Feature::GLOBAL_START, Feature::GLOBAL_END)
    } else {
        (
            u32::try_from(r.start).unwrap_or(u32::MAX),
            u32::try_from(r.end).unwrap_or(u32::MAX),
        )
    };
    Feature {
        tag: Tag::new(tag),
        value,
        start,
        end,
    }
}

fn shape_one(
    tf: &Typeface,
    size: f32,
    text: &str,
    rtl: bool,
    locale: &str,
    features: &[([u8; 4], u32, std::ops::Range<usize>)],
) -> Shaped {
    let mut buffer = Buffer::new();
    buffer.push_str(text);
    buffer.set_direction(if rtl {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    });
    if !locale.is_empty() {
        buffer.set_language(locale.parse::<Language>().ok());
    }
    buffer.guess_segment_properties();
    buffer.set_cluster_level(ClusterLevel::MonotoneCharacters);
    let scale = fixed(size);
    let font = ShaperFont::new(&tf.font).with_scale(scale);
    let features: Vec<Feature> = features
        .iter()
        .map(|(t, v, r)| feature(t, *v, r, text.len()))
        .collect();
    // harfrust holds each call to HarfBuzz's operation and length limits
    // (`max_ops`, `max_len`), on by default and not settable: a crafted
    // font stops shaping, it does not spin.
    if harfrust::shape(&font, &mut buffer, ShapeOptions::new().features(&features)).is_err() {
        // A face HarfBuzz refuses draws nothing; its text is unresolved.
        return empty(text, rtl);
    }
    let to_f = |v: i32| v as f32 / 65536.0;
    let mut out = Shaped::default();
    let mut x = 0.0f32;
    let mut y = 0.0f32;
    for (info, pos) in buffer.glyph_infos().iter().zip(buffer.glyph_positions()) {
        out.glyphs.push(info.glyph_id);
        out.positions.push([x, y]);
        out.offsets.push([to_f(pos.x_offset), -to_f(pos.y_offset)]);
        out.clusters.push(info.cluster);
        x += to_f(pos.x_advance);
        y -= to_f(pos.y_advance);
    }
    out.positions.push([x, y]);
    out.offsets.push([0.0, 0.0]);
    out.clusters.push(if rtl { 0 } else { text.len() as u32 });
    out.advance = x;
    out
}

impl FontMetrics {
    /// `tf`'s metrics at `size`: OS/2's typographic line when the font asks
    /// for it (`USE_TYPO_METRICS`), else hhea's, as Skia's scalers read
    /// them; hhea all zero falls to OS/2.
    pub(crate) fn of(tf: &Typeface, size: f32) -> FontMetrics {
        let m = tf.font.metrics();
        let upem = f32::from(m.units_per_em.max(1));
        let s = size / upem;
        let line = if m.use_typo_metrics {
            m.typo_line.or(m.hhea_line)
        } else {
            m.hhea_line
                .filter(|l| l.ascender.to_f32() != 0.0 || l.descender.to_f32() != 0.0)
                .or(m.typo_line)
        };
        let (ascent, descent, gap) = line
            .map(|l| {
                (
                    l.ascender.to_f32(),
                    l.descender.to_f32(),
                    l.line_gap.to_f32(),
                )
            })
            .unwrap_or((upem * 0.8, -upem * 0.2, 0.0));
        let style = tf.font.style_metrics();
        let under = style
            .underline
            .map(|d| (d.position.to_f32(), d.thickness.to_f32()));
        let strike = style
            .strike_through
            .map(|d| (d.position.to_f32(), d.thickness.to_f32()));
        FontMetrics {
            ascent: -ascent * s,
            descent: -descent * s,
            leading: gap * s,
            underline_position: under.map(|(p, _)| -p * s),
            underline_thickness: under.map(|(_, t)| t * s),
            strikeout_position: strike.map(|(p, _)| -p * s),
            strikeout_thickness: strike.map(|(_, t)| t * s),
            x_height: m.x_height.map_or(0.0, |v| v.to_f32()) * s,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn long_text_splits_after_a_space() {
        let text = "word ".repeat(20_000);
        let p = pieces(&text);
        assert!(p.len() > 1);
        assert_eq!(p.first().unwrap().start, 0);
        assert_eq!(p.last().unwrap().end, text.len());
        for w in p.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
        for r in &p {
            assert!(r.len() <= MAX_SHAPE_BYTES);
        }
        for r in &p[..p.len() - 1] {
            assert!(text[r.clone()].ends_with(' '));
        }
    }

    #[test]
    fn text_with_no_space_splits_on_a_character() {
        // Three bytes a character: the limit falls inside one.
        let text = "\u{4E2D}".repeat(MAX_SHAPE_BYTES);
        for r in pieces(&text) {
            assert!(r.len() <= MAX_SHAPE_BYTES);
            assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
        }
    }

    #[test]
    fn short_text_is_one_piece() {
        assert_eq!(pieces("hello"), vec![0..5]);
        assert_eq!(pieces(""), vec![0..0]);
    }

    /// The cache starts over past `CACHE_ENTRIES`, so changing text holds
    /// it bounded.
    #[test]
    #[ignore = "app lane: needs FLUTTER_ROOT"]
    fn the_shaped_run_cache_is_bounded() {
        let root = std::env::var("FLUTTER_ROOT").unwrap();
        let data = std::fs::read(format!(
            "{root}/bin/cache/artifacts/material_fonts/Roboto-Regular.ttf"
        ))
        .unwrap();
        let face = crate::font::Face::new(Arc::new(data), 0, "Roboto").unwrap();
        let tf = Typeface::new(&face, &[]);
        for i in 0..CACHE_ENTRIES + 10 {
            shape(&tf, 14.0, &format!("text {i}"), false, "", &[]);
            let n = CACHE.lock().unwrap().as_ref().map_or(0, HashMap::len);
            assert!(n <= CACHE_ENTRIES);
        }
    }
}
