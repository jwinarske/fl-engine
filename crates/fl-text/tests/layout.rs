//! Layout in Roboto, read from the Flutter release's cache as the
//! `metrics` app reads it. The numbers are the stock engine's, from
//! apps/metrics/test/metrics.txt.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use fl_text::font::FontCollection;
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
