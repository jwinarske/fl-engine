//! Layout in Roboto, read from the Flutter release's cache as the
//! `metrics` app reads it. The numbers are the stock engine's, from
//! apps/metrics/test/metrics.txt.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use fl_text::font::FontCollection;
use fl_text::paint::PaintOp;
use fl_text::style::{ParagraphStyle, TextStyle};
use fl_text::{Paragraph, ParagraphBuilder, RectHeightStyle, RectWidthStyle};

fn roboto(face: &str) -> Vec<u8> {
    let root =
        std::env::var("FLUTTER_ROOT").expect("FLUTTER_ROOT must name the pinned Flutter release");
    std::fs::read(format!("{root}/bin/cache/artifacts/material_fonts/{face}")).unwrap()
}

fn fonts() -> Arc<FontCollection> {
    let f = FontCollection::without_system_fonts();
    assert!(f.add_font(roboto("Roboto-Regular.ttf"), "Roboto"));
    Arc::new(f)
}

fn paragraph(fonts: Arc<FontCollection>, text: &str, width: f32) -> Paragraph {
    let mut ps = ParagraphStyle::default();
    ps.text_style.font_families = vec!["Roboto".into()];
    let mut b = ParagraphBuilder::new(ps, fonts);
    let mut ts: TextStyle = b.peek_style();
    ts.font_size = 14.0;
    b.push_style(ts);
    b.add_text(text);
    let mut p = b.build();
    p.layout(width);
    p
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.0005
}

#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn plain_matches_the_stock_engine() {
    let p = paragraph(fonts(), "Hello world", 400.0);
    assert!(
        near(p.height(), 16.0)
            && near(p.longest_line(), 70.178)
            && near(p.min_intrinsic_width(), 34.535)
    );
    assert!(near(p.alphabetic_baseline(), 12.988) && near(p.ideographic_baseline(), 16.406));
    let m = &p.line_metrics()[0];
    assert!(
        m.hard_break && (m.ascent - 12.988).abs() < 0.0005 && (m.baseline - 12.582).abs() < 0.0005
    );
    let b = p.rects_for_range(2, 9, RectHeightStyle::Tight, RectWidthStyle::Tight);
    assert_eq!(b.len(), 1);
    assert!(near(b[0].rect[0], 17.397) && near(b[0].rect[1], -0.406) && near(b[0].rect[2], 58.885));
}

#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn wraps_where_the_stock_engine_wraps() {
    let p = paragraph(
        fonts(),
        "The quick brown fox jumps over the lazy dog, and then it sleeps.",
        120.0,
    );
    let widths: Vec<f64> = p.line_metrics().iter().map(|m| m.width).collect();
    assert_eq!(widths.len(), 4);
    for (w, want) in widths.iter().zip([102.594, 114.871, 112.116, 55.645]) {
        assert!((w - want).abs() < 0.0005, "{widths:?}");
    }
}

/// Past `MAX_SHAPE_BYTES` text is shaped in pieces split after spaces, so
/// it measures what it would whole.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn text_past_the_shaping_limit_is_shaped_in_pieces() {
    let f = fonts();
    let one = paragraph(Arc::clone(&f), "word ", f32::INFINITY).max_intrinsic_width();
    let n = 20_000;
    let p = paragraph(f, &"word ".repeat(n), 300.0);
    let whole = p.max_intrinsic_width();
    assert!(
        ((whole / n as f32) - one).abs() < 0.001,
        "{whole} for {n} of {one}"
    );
    assert!(p.line_count() > 1000);
}

/// A font cut short is no font, or a face that shapes to nothing: never a
/// panic out of layout.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn a_truncated_font_does_not_panic() {
    let data = roboto("Roboto-Regular.ttf");
    for len in [0, 12, 100, 1000, data.len() / 2, data.len() - 1] {
        let f = FontCollection::without_system_fonts();
        let _ = f.add_font(data[..len].to_vec(), "Cut");
        let mut ps = ParagraphStyle::default();
        ps.text_style.font_families = vec!["Cut".into()];
        let mut b = ParagraphBuilder::new(ps, Arc::new(f));
        b.add_text("Hello world");
        let mut p = b.build();
        p.layout(100.0);
        let _ = p.rects_for_range(0, 11, RectHeightStyle::Tight, RectWidthStyle::Tight);
    }
}

/// A character no font has is counted unresolved and drawn with the first
/// face's missing glyph, as SkParagraph draws it.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn a_character_no_font_has_is_unresolved() {
    let p = paragraph(fonts(), "a\u{4E2D}b", 400.0);
    assert_eq!(p.unresolved_glyphs(), 1);
    assert_eq!(p.line_count(), 1);
    assert!(p.max_intrinsic_width() > 0.0);
}

/// Roboto with its GSUB replaced by `gsub`: the table directory kept in
/// order, every table copied, offsets four-byte aligned. Checksums are left
/// as they were; nothing here reads them.
fn with_gsub(font: &[u8], gsub: &[u8]) -> Vec<u8> {
    let be16 = |b: &[u8], at: usize| u16::from_be_bytes([b[at], b[at + 1]]);
    let be32 = |b: &[u8], at: usize| u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
    let n = usize::from(be16(font, 4));
    let mut out = font[..12 + 16 * n].to_vec();
    let mut at = out.len();
    for i in 0..n {
        let rec = 12 + 16 * i;
        let tag = &font[rec..rec + 4];
        let data = if tag == b"GSUB" {
            gsub
        } else {
            let (off, len) = (be32(font, rec + 8) as usize, be32(font, rec + 12) as usize);
            &font[off..off + len]
        };
        out[rec + 8..rec + 12].copy_from_slice(&(at as u32).to_be_bytes());
        out[rec + 12..rec + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        at = out.len();
    }
    assert!(out.windows(4).any(|w| w == b"GSUB"), "no GSUB to replace");
    out
}

/// A GSUB whose work grows as `WIDTH` to the power `DEPTH` without the
/// shaper's operation limit, and never nests deeper than `DEPTH`, under
/// HarfBuzz's nesting limit of 64, which would end it first. Lookup 0 is
/// on by default (`ccmp`). Lookups 0 to `DEPTH - 1` are contextual
/// (format 3): each matches `WIDTH` of `glyph` and applies the next lookup
/// at every one of them. The last substitutes `glyph` for itself.
fn looping_gsub(glyph: u16) -> Vec<u8> {
    const WIDTH: u16 = 16;
    const DEPTH: u16 = 8;
    fn put(g: &mut Vec<u8>, v: &[u16]) {
        v.iter().for_each(|x| g.extend_from_slice(&x.to_be_bytes()));
    }
    let mut g = Vec::new();
    // Header: version 1.0, then the three lists' offsets.
    put(&mut g, &[1, 0, 10, 30, 44]);
    // ScriptList at 10: DFLT, its script at 8, whose default LangSys at 4
    // lists feature 0.
    put(&mut g, &[1]);
    g.extend_from_slice(b"DFLT");
    put(&mut g, &[8, 4, 0, 0, 0xFFFF, 1, 0]);
    // FeatureList at 30: ccmp, at 8, with lookup 0.
    put(&mut g, &[1]);
    g.extend_from_slice(b"ccmp");
    put(&mut g, &[8, 0, 1, 0]);
    // LookupList at 44: the lookups' offsets, then the lookups.
    let contextual = 8 + 6 + 6 * WIDTH + 6;
    let count = DEPTH + 1;
    put(&mut g, &[count]);
    for i in 0..count {
        put(&mut g, &[2 + 2 * count + i * contextual]);
    }
    for i in 0..DEPTH {
        // Type 5, no flags, one subtable at 8. Format 3: WIDTH glyphs and
        // WIDTH records, every coverage at the end of the subtable, every
        // record (j, lookup i + 1). Coverage: format 1, the one glyph.
        let coverage = 6 + 6 * WIDTH;
        put(&mut g, &[5, 0, 1, 8, 3, WIDTH, WIDTH]);
        (0..WIDTH).for_each(|_| put(&mut g, &[coverage]));
        (0..WIDTH).for_each(|j| put(&mut g, &[j, i + 1]));
        put(&mut g, &[1, 1, glyph]);
    }
    // The last: type 1, one subtable at 8, format 2, coverage at 8, one
    // substitute, the glyph itself.
    put(&mut g, &[1, 0, 1, 8, 2, 8, 1, glyph, 1, 1, glyph]);
    g
}

/// harfrust's operation limit (brief M3 §15, shaping cost): a crafted font
/// whose lookups fan out 16 ways 8 levels deep, 4 billion applications at
/// each position, shapes and returns. The limit is what ends it: 8 levels
/// are well inside the nesting limit.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn a_font_whose_lookups_loop_still_shapes() {
    let roboto = roboto("Roboto-Regular.ttf");
    let glyph = {
        use skrifa::MetadataProvider;
        let font = skrifa::FontRef::new(&roboto).unwrap();
        font.charmap().map('a').unwrap().to_u32() as u16
    };
    let looping = with_gsub(&roboto, &looping_gsub(glyph));
    let text = "a".repeat(200);
    let plain = paragraph(fonts(), &text, f32::INFINITY).max_intrinsic_width();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let f = FontCollection::without_system_fonts();
        assert!(f.add_font(looping, "Roboto"));
        let p = paragraph(Arc::new(f), &text, f32::INFINITY);
        let _ = tx.send(p.max_intrinsic_width());
    });
    // Locally 20 ms, against under 1 ms for Roboto as it is: the budget,
    // 4,096 operations a character, spent.
    let width = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("shaping a font whose lookups loop did not return within 60 s");
    // Every lookup substitutes the glyph for itself: what comes out is
    // Roboto's.
    assert!(near(width, plain), "{width} against {plain}");
}

/// Text in three scripts one face covers is three runs, a script each, as
/// SkShaper's script iterator splits it; the spaces join the run before.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn mixed_scripts_are_shaped_a_run_each() {
    let text = "abc \u{416}\u{414} \u{3B1}\u{3B2}";
    let p = paragraph(fonts(), text, 400.0);
    assert_eq!(p.unresolved_glyphs(), 0);
    let runs = p
        .paint_ops(0.0, 0.0)
        .iter()
        .filter(|op| matches!(op, PaintOp::Glyphs { .. }))
        .count();
    assert_eq!(runs, 3);
    // The same width as each script's text laid out alone.
    let alone: f32 = ["abc ", "\u{416}\u{414} ", "\u{3B1}\u{3B2}"]
        .iter()
        .map(|t| paragraph(fonts(), t, 400.0).max_intrinsic_width())
        .sum();
    assert!(near(p.max_intrinsic_width(), alone));
}
