//! Fuzzing font loading (brief M3 §15): Roboto and MaterialIcons from the
//! Flutter release's cache, mutated, given to `loadFontFromList`'s path,
//! then a paragraph laid out, queried and painted in each. Fonts are parsed
//! and runs shaped under `catch_unwind` inside fl-text, so no panic may
//! reach here. Deterministic and pure Rust, as fl-image's; the sanitizer
//! lanes run it under ASan.
//!
//!     FL_FUZZ_ITERS=5000 cargo test -p fl-text --test fuzz -- --ignored
#![allow(clippy::unwrap_used, clippy::panic)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use fl_text::font::FontCollection;
use fl_text::style::{ParagraphStyle, TextStyle};
use fl_text::{ParagraphBuilder, RectHeightStyle, RectWidthStyle};

/// xorshift64*.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// `seed` changed one to eight ways, half of them in its first 4 KiB,
/// where the table directory and the small tables are.
fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut v = seed.to_vec();
    for _ in 0..1 + rng.below(8) {
        let span = if rng.below(2) == 0 {
            v.len().min(4096)
        } else {
            v.len()
        };
        let at = rng.below(span);
        match rng.below(4) {
            0 => v[at] ^= 1 << rng.below(8),
            1 => v[at] = rng.next() as u8,
            2 => {
                let b = [0x00, 0x01, 0xff][rng.below(3)];
                for x in v.iter_mut().skip(at).take(1 + rng.below(4)) {
                    *x = b;
                }
            }
            _ => v.truncate(at.max(12)),
        }
    }
    v
}

/// Latin with ligatures and kerning pairs, Arabic, a combining mark, an
/// emoji and an icon's code point: every shaping path a font can reach.
const TEXT: &str = "office AVAWAY fi ffl 0123 \u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064A}\u{0629} e\u{0301} \u{1F600} \u{E87C}\nsecond line";

fn lay_out(font: Vec<u8>) {
    let fonts = FontCollection::without_system_fonts();
    if !fonts.add_font(font, "Fuzz") {
        return;
    }
    let mut ps = ParagraphStyle::default();
    ps.text_style.font_families = vec!["Fuzz".into()];
    let mut b = ParagraphBuilder::new(ps, Arc::new(fonts));
    let mut ts: TextStyle = b.peek_style();
    ts.font_size = 17.0;
    b.push_style(ts);
    b.add_text(TEXT);
    let mut p = b.build();
    p.layout(150.0);
    let _ = p.line_metrics();
    let _ = p.rects_for_range(0, 40, RectHeightStyle::Max, RectWidthStyle::Max);
    let _ = p.position_at(60.0, 20.0);
    let _ = p.paint_ops(0.0, 0.0);
}

#[test]
#[ignore = "fuzz: needs FLUTTER_ROOT; FL_FUZZ_ITERS=5000 cargo test -p fl-text --test fuzz -- --ignored"]
fn font_loading_answers_every_mutation() {
    let root =
        std::env::var("FLUTTER_ROOT").expect("FLUTTER_ROOT must name the pinned Flutter release");
    let dir = format!("{root}/bin/cache/artifacts/material_fonts");
    let seeds: Vec<Vec<u8>> = ["Roboto-Regular.ttf", "MaterialIcons-Regular.otf"]
        .iter()
        .map(|f| std::fs::read(format!("{dir}/{f}")).unwrap())
        .collect();
    let iters: usize = std::env::var("FL_FUZZ_ITERS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(500);
    // fl-text's own catch_unwind would print each caught panic.
    std::panic::set_hook(Box::new(|_| {}));
    let mut rng = Rng(0x5eed_f0e7_0000_0001);
    let mut escaped = Vec::new();
    for i in 0..iters {
        let input = mutate(&mut rng, &seeds[i % seeds.len()]);
        if catch_unwind(AssertUnwindSafe(|| lay_out(input.clone()))).is_err() {
            escaped.push((i, input));
        }
    }
    let _ = std::panic::take_hook();
    println!("{iters} fonts, {} panics past fl-text", escaped.len());
    if let Some((i, input)) = escaped.first() {
        let out = std::env::temp_dir().join(format!("fl-text-fuzz-{i}.bin"));
        let _ = std::fs::write(&out, input);
        panic!(
            "{} panics reached the caller, the first at input {i}, written to {}",
            escaped.len(),
            out.display()
        );
    }
}
