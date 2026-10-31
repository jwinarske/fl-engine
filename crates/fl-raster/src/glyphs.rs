//! Glyphs on the raster thread (brief M3 §6, stage 5): rasterized by swash
//! at the size the device shows them, packed into emblema's atlas, and drawn
//! a run at a time with `draw_glyphs`.
//!
//! A frame is replayed twice: first to find and rasterize every glyph it
//! needs, then, with the atlas uploaded once, to draw. The atlas may repack
//! or grow while glyphs go in, and a run recorded before that would read
//! the old places.
//!
//! Glyphs too large for the atlas, and text whose paint has a shader or a
//! stroke, are drawn as their outlines, as Impeller draws them as paths.
//! emblema has no subpixel positioning, so a glyph is placed on a whole
//! device pixel: a recorded difference from Impeller's.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use emblema::{
    Atlas, Context, Coverage, Extent2D, GlyphKey, Image, PathBuilder, PixelFormat, PositionedGlyph,
    Vec2,
};
use fl_text::paint::GlyphRun;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Format, Verb};
use swash::FontRef;

use crate::RasterError;

/// The largest glyph rasterized into the atlas, in device pixels; above
/// it a glyph is its outline (Skia's `kSkSideTooBigForAtlas`).
pub(crate) const MAX_ATLAS_GLYPH: f32 = 256.0;

/// The atlas's first side, and the largest it grows to.
const ATLAS_START: u32 = 512;
const ATLAS_LIMIT: u32 = 4096;

/// Where a rasterized glyph's coverage sits relative to its origin, in
/// device pixels: left, top (up positive) and its size.
#[derive(Debug, Clone, Copy)]
struct Placement {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}

/// The glyph cache: the atlas, its device image, and each glyph's place.
/// `!Send`: it belongs to the raster thread, as the GPU image cache does.
pub struct Glyphs {
    atlas: Atlas,
    image: Option<(Image, u32)>,
    placed: HashMap<GlyphKey, Placement>,
    /// Glyphs that could not go in the atlas this frame, drawn as outlines.
    refused: std::collections::HashSet<GlyphKey>,
    scaler: RefCell<ScaleContext>,
    _raster_thread: PhantomData<*const ()>,
}

impl Default for Glyphs {
    fn default() -> Self {
        Glyphs {
            atlas: Atlas::with_limit(ATLAS_START, ATLAS_LIMIT),
            image: None,
            placed: HashMap::new(),
            refused: std::collections::HashSet::new(),
            scaler: RefCell::new(ScaleContext::new()),
            _raster_thread: PhantomData,
        }
    }
}

/// The atlas's identity for a face at its variation settings.
fn font_key(run: &GlyphRun) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    run.typeface.face.id.hash(&mut h);
    for (tag, v) in run.typeface.variations.iter() {
        tag.hash(&mut h);
        v.to_bits().hash(&mut h);
    }
    h.finish()
}

/// The device size a run's glyphs show at, under `m`'s scale: the larger
/// axis, as Impeller scales text frames.
fn device_size(run: &GlyphRun, m: &emblema::Affine2) -> f32 {
    run.font_size * m.matrix2.x_axis.length().max(m.matrix2.y_axis.length())
}

/// Whether a run is drawn from the atlas, rather than as outlines.
fn atlas_route(run: &GlyphRun, transform: Option<&emblema::Affine2>) -> bool {
    transform.is_some_and(|m| {
        let px = device_size(run, m);
        px.is_finite() && px > 0.0 && px <= MAX_ATLAS_GLYPH
    })
}

impl Glyphs {
    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// A frame begins: glyphs it does not use again may be evicted.
    pub(crate) fn begin_frame(&mut self) {
        self.atlas.begin_frame();
        self.refused.clear();
    }

    /// Rasterizes and inserts what `run` needs drawn under `transform`.
    pub(crate) fn prepare(&mut self, transform: emblema::Transform2D, run: &GlyphRun) {
        let m = transform.to_affine();
        if !atlas_route(run, m.as_ref()) {
            return;
        }
        let Some(m) = m else {
            return;
        };
        let px = device_size(run, &m).round().max(1.0);
        let font = font_key(run);
        let (data, index) = run.typeface.face.data();
        let Some(fref) = FontRef::from_index(data.as_slice(), index as usize) else {
            return;
        };
        let mut ctx = self.scaler.borrow_mut();
        let vars: Vec<([u8; 4], f32)> = run.typeface.variations.iter().copied().collect();
        let mut scaler = ctx
            .builder(fref)
            .size(px)
            .hint(false)
            .variations(vars.iter())
            .build();
        for &glyph in &run.glyphs {
            let key = GlyphKey {
                font,
                glyph,
                size: px as u16,
            };
            if self.atlas.get(key).is_some() {
                // Asked for again this frame, which keeps it from eviction.
                let _ = self.atlas.insert(
                    key,
                    &Coverage {
                        width: 0,
                        height: 0,
                        texels: Vec::new(),
                    },
                );
                continue;
            }
            let Some(img) = Render::new(&[Source::Outline])
                .format(Format::Alpha)
                .render(&mut scaler, glyph)
            else {
                self.refused.insert(key);
                continue;
            };
            let p = img.placement;
            let coverage = Coverage {
                width: p.width,
                height: p.height,
                texels: img.data,
            };
            match self.atlas.insert(key, &coverage) {
                Ok(_) => {
                    self.placed.insert(
                        key,
                        Placement {
                            left: p.left,
                            top: p.top,
                            width: p.width,
                            height: p.height,
                        },
                    );
                }
                // Full past its limit, or larger than it: this glyph is
                // drawn as its outline this frame; text never goes missing.
                Err(_) => {
                    self.refused.insert(key);
                }
            }
        }
    }

    /// Puts the atlas on the device if anything went in: once a frame.
    pub(crate) fn upload(&mut self, ctx: &mut Context) -> Result<(), RasterError> {
        let size = self.atlas.size();
        let stale = self.image.as_ref().is_none_or(|(_, s)| *s != size);
        if stale {
            if let Some((old, _)) = self.image.take() {
                ctx.destroy_image(old);
            }
            let image = ctx.create_image(Extent2D::new(size, size), PixelFormat::R8Unorm)?;
            self.image = Some((image, size));
        }
        if stale || self.atlas.is_dirty() {
            if let Some((image, _)) = self.image.as_mut() {
                ctx.write_image(image, self.atlas.texels())?;
            }
            self.atlas.mark_clean();
        }
        Ok(())
    }

    /// The atlas's device image, once uploaded.
    pub(crate) fn image(&self) -> Option<&Image> {
        self.image.as_ref().map(|(i, _)| i)
    }

    /// Frees the device image; before the context goes.
    pub(crate) fn clear(&mut self, ctx: &mut Context) {
        if let Some((image, _)) = self.image.take() {
            ctx.destroy_image(image);
        }
    }

    /// The run's glyphs as atlas quads, in the canvas's space, each on a
    /// whole device pixel; `None` if the run is drawn as outlines.
    pub(crate) fn positioned(
        &self,
        transform: emblema::Transform2D,
        run: &GlyphRun,
        origin: [f32; 2],
    ) -> Option<Vec<PositionedGlyph>> {
        let m = transform.to_affine();
        if !atlas_route(run, m.as_ref()) {
            return None;
        }
        let m = m?;
        let inv = m.inverse();
        let px = device_size(run, &m).round().max(1.0);
        let font = font_key(run);
        let (sx, sy) = (m.matrix2.x_axis.length(), m.matrix2.y_axis.length());
        let mut out = Vec::with_capacity(run.glyphs.len());
        for (glyph, pos) in run.glyphs.iter().zip(&run.positions) {
            let key = GlyphKey {
                font,
                glyph: *glyph,
                size: px as u16,
            };
            if self.refused.contains(&key) {
                return None;
            }
            let pl = self.placed.get(&key)?;
            self.atlas.get(key)?;
            if pl.width == 0 || pl.height == 0 {
                continue;
            }
            let at = m.transform_point2(Vec2::new(origin[0] + pos[0], origin[1] + pos[1]));
            let tl = Vec2::new(at.x.round() + pl.left as f32, at.y.round() - pl.top as f32);
            let user = inv.transform_point2(tl);
            out.push(PositionedGlyph {
                key,
                position: [user.x, user.y],
                size: [pl.width as f32 / sx, pl.height as f32 / sy],
            });
        }
        Some(out)
    }

    /// The run's glyph outlines as one path in the canvas's space.
    pub(crate) fn outlines(&self, run: &GlyphRun, origin: [f32; 2]) -> Option<emblema::Path> {
        let (data, index) = run.typeface.face.data();
        let fref = FontRef::from_index(data.as_slice(), index as usize)?;
        let mut ctx = self.scaler.borrow_mut();
        let vars: Vec<([u8; 4], f32)> = run.typeface.variations.iter().copied().collect();
        let mut scaler = ctx
            .builder(fref)
            .size(run.font_size)
            .hint(false)
            .variations(vars.iter())
            .build();
        let mut b = PathBuilder::new();
        for (glyph, pos) in run.glyphs.iter().zip(&run.positions) {
            let Some(outline) = scaler.scale_outline(*glyph) else {
                continue;
            };
            let (ox, oy) = (origin[0] + pos[0], origin[1] + pos[1]);
            // Font space is y up; the canvas is y down.
            let pt = |i: usize| {
                let p = outline.points()[i];
                Vec2::new(ox + p.x, oy - p.y)
            };
            let mut i = 0;
            for verb in outline.verbs() {
                match verb {
                    Verb::MoveTo => {
                        b.move_to(pt(i));
                        i += 1;
                    }
                    Verb::LineTo => {
                        b.line_to(pt(i));
                        i += 1;
                    }
                    Verb::QuadTo => {
                        b.quad_to(pt(i), pt(i + 1));
                        i += 2;
                    }
                    Verb::CurveTo => {
                        b.cubic_to(pt(i), pt(i + 1), pt(i + 2));
                        i += 3;
                    }
                    Verb::Close => {
                        b.close();
                    }
                }
            }
        }
        Some(b.build())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use emblema::Transform2D;
    use fl_text::font::FontCollection;
    use fl_text::paint::PaintOp;
    use fl_text::style::ParagraphStyle;
    use fl_text::ParagraphBuilder;
    use std::sync::Arc;

    /// The glyph run of `text` in Roboto at `size`.
    fn run(text: &str, size: f32) -> Arc<GlyphRun> {
        let root = std::env::var("FLUTTER_ROOT")
            .expect("FLUTTER_ROOT must name the pinned Flutter release");
        let data = std::fs::read(format!(
            "{root}/bin/cache/artifacts/material_fonts/Roboto-Regular.ttf"
        ))
        .unwrap();
        let fonts = FontCollection::without_system_fonts();
        assert!(fonts.add_font(data, "Roboto"));
        let mut ps = ParagraphStyle::default();
        ps.text_style.font_families = vec!["Roboto".into()];
        let mut b = ParagraphBuilder::new(ps, Arc::new(fonts));
        let mut ts = b.peek_style();
        ts.font_size = size;
        b.push_style(ts);
        b.add_text(text);
        let mut p = b.build();
        p.layout(10_000.0);
        p.paint_ops(0.0, 0.0)
            .into_iter()
            .find_map(|op| match op {
                PaintOp::Glyphs { run, .. } => Some(run),
                _ => None,
            })
            .unwrap()
    }

    fn scale(s: f32) -> Transform2D {
        Transform2D::from(emblema::Affine2::from_scale(Vec2::splat(s)))
    }

    #[test]
    #[ignore = "app lane: needs FLUTTER_ROOT"]
    fn glyphs_are_rasterized_at_the_device_size_on_whole_pixels() {
        let r = run("Hi", 14.0);
        let mut g = Glyphs::default();
        g.begin_frame();
        g.prepare(scale(2.0), &r);
        let q = g.positioned(scale(2.0), &r, [10.3, 20.7]).unwrap();
        assert_eq!(q.len(), 2);
        for p in &q {
            assert_eq!(p.key.size, 28);
            // Whole device pixels: the canvas position times the scale.
            for v in p.position {
                assert!(
                    (v * 2.0 - (v * 2.0).round()).abs() < 1e-3,
                    "{:?}",
                    p.position
                );
            }
        }
        assert!(g.atlas().is_dirty());
    }

    #[test]
    #[ignore = "app lane: needs FLUTTER_ROOT"]
    fn a_glyph_past_the_atlas_is_its_outline() {
        let r = run("A", 200.0);
        let mut g = Glyphs::default();
        g.begin_frame();
        // 400 device pixels: past MAX_ATLAS_GLYPH.
        g.prepare(scale(2.0), &r);
        assert!(g.positioned(scale(2.0), &r, [0.0, 0.0]).is_none());
        assert!(g.atlas().is_empty());
        let path = g.outlines(&r, [0.0, 0.0]).unwrap();
        assert!(!path.verbs().is_empty());
    }

    #[test]
    #[ignore = "app lane: needs FLUTTER_ROOT"]
    fn an_unprepared_run_is_drawn_as_outlines() {
        // A run the first replay did not see never reads a missing atlas
        // place; it goes to outlines instead of disappearing.
        let r = run("Hi", 14.0);
        let g = Glyphs::default();
        assert!(g.positioned(scale(1.0), &r, [0.0, 0.0]).is_none());
    }
}
