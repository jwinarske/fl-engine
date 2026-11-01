//! Glyphs on the raster thread (brief M3 §6, stage 5): rasterized by swash
//! at the size the device shows them, packed into emblema's atlas, and drawn
//! a run at a time with `draw_glyphs`.
//!
//! A frame is replayed twice: first to find and rasterize every glyph it
//! needs, then, with the atlas uploaded once, to draw. The atlas may repack
//! or grow while glyphs go in, and a run recorded before that would read
//! the old places.
//!
//! The atlas is the cache, kept from frame to frame. A frame that needs
//! more glyphs than it holds at its limit puts the rest on overflow pages,
//! atlases of that frame only, each drawn from its own image: the frame is
//! drawn in more than one pass, not dropped. Only past the last page is a
//! run drawn as its outline.
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

/// Overflow pages a frame may use beside the cache: with it, four 4096
/// atlases, 64 MiB of coverage at their largest, freed with the frame
/// that needed them.
const MAX_OVERFLOW: usize = 3;

/// An atlas and its device image.
struct Page {
    atlas: Atlas,
    image: Option<(Image, u32)>,
}

impl Page {
    fn new() -> Page {
        Page {
            atlas: Atlas::with_limit(ATLAS_START, ATLAS_LIMIT),
            image: None,
        }
    }

    /// Puts the atlas on the device if anything went in; whether it wrote.
    fn upload(&mut self, ctx: &mut Context) -> Result<bool, RasterError> {
        let size = self.atlas.size();
        let stale = self.image.as_ref().is_none_or(|(_, s)| *s != size);
        if stale {
            if let Some((old, _)) = self.image.take() {
                ctx.destroy_image(old);
            }
            let image = ctx.create_image(Extent2D::new(size, size), PixelFormat::R8Unorm)?;
            self.image = Some((image, size));
        }
        let mut wrote = false;
        if stale || self.atlas.is_dirty() {
            if let Some((image, _)) = self.image.as_mut() {
                ctx.write_image(image, self.atlas.texels())?;
                wrote = true;
            }
            self.atlas.mark_clean();
        }
        Ok(wrote)
    }

    fn clear(&mut self, ctx: &mut Context) {
        if let Some((image, _)) = self.image.take() {
            ctx.destroy_image(image);
        }
    }
}

/// Where a rasterized glyph's coverage sits relative to its origin, in
/// device pixels: left, top (up positive) and its size.
#[derive(Debug, Clone, Copy)]
struct Placement {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}

/// The glyph cache: the atlas, its device image, this frame's overflow
/// pages, and each glyph's place. `!Send`: it belongs to the raster thread,
/// as the GPU image cache does.
pub struct Glyphs {
    cache: Page,
    /// This frame's pages past the cache, in the order they filled.
    overflow: Vec<Page>,
    /// How many of `overflow` this frame uses; the rest are freed at upload.
    overflow_used: usize,
    placed: HashMap<GlyphKey, Placement>,
    /// Glyphs no page had room for this frame, drawn as outlines.
    refused: std::collections::HashSet<GlyphKey>,
    scaler: RefCell<ScaleContext>,
    /// Atlas writes to the device, for the soak lane.
    uploads: u64,
    _raster_thread: PhantomData<*const ()>,
}

impl Default for Glyphs {
    fn default() -> Self {
        Glyphs {
            cache: Page::new(),
            overflow: Vec::new(),
            overflow_used: 0,
            placed: HashMap::new(),
            refused: std::collections::HashSet::new(),
            scaler: RefCell::new(ScaleContext::new()),
            uploads: 0,
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
    /// The cache's atlas.
    pub fn atlas(&self) -> &Atlas {
        &self.cache.atlas
    }

    /// Overflow pages the last frame used.
    pub fn overflow_pages(&self) -> usize {
        self.overflow_used
    }

    /// The page `key` is on this frame: 0 the cache, then the overflow
    /// pages in order.
    fn page_of(&self, key: GlyphKey) -> Option<usize> {
        if self.cache.atlas.get(key).is_some() {
            return Some(0);
        }
        self.overflow[..self.overflow_used]
            .iter()
            .position(|p| p.atlas.get(key).is_some())
            .map(|i| i + 1)
    }

    /// Page `i`'s atlas and device image.
    fn page(&self, i: usize) -> Option<&Page> {
        match i {
            0 => Some(&self.cache),
            _ => self.overflow.get(i - 1).filter(|_| i <= self.overflow_used),
        }
    }

    /// The pages' device images, the cache first: slot `i` is page `i`.
    pub(crate) fn images(&self) -> Vec<&Image> {
        (0..=self.overflow_used)
            .map_while(|i| self.page(i)?.image.as_ref().map(|(img, _)| img))
            .collect()
    }

    /// Page `i`'s atlas, for drawing from.
    pub(crate) fn page_atlas(&self, i: usize) -> Option<&Atlas> {
        self.page(i).map(|p| &p.atlas)
    }

    /// How many times the atlas has been written to the device.
    pub fn uploads(&self) -> u64 {
        self.uploads
    }

    /// Placements kept, those of glyphs evicted since included until the
    /// next prune.
    pub fn placements(&self) -> usize {
        self.placed.len()
    }

    /// A frame begins: glyphs it does not use again may be evicted.
    pub(crate) fn begin_frame(&mut self) {
        self.cache.atlas.begin_frame();
        // Last frame's overflow is not kept: the cache is what persists.
        // Their device images are, until upload knows whether this frame
        // needs as many.
        for page in &mut self.overflow {
            page.atlas = Atlas::with_limit(ATLAS_START, ATLAS_LIMIT);
        }
        self.overflow_used = 0;
        self.refused.clear();
    }

    /// Puts `coverage` on the first page with room, opening an overflow
    /// page if none has; false past the last. The pages apart from `self`,
    /// as the scaler is borrowed beside them.
    fn insert(
        cache: &mut Page,
        overflow: &mut Vec<Page>,
        used: &mut usize,
        key: GlyphKey,
        coverage: &Coverage,
    ) -> bool {
        if cache.atlas.insert(key, coverage).is_ok() {
            return true;
        }
        for page in &mut overflow[..*used] {
            if page.atlas.insert(key, coverage).is_ok() {
                return true;
            }
        }
        if *used == MAX_OVERFLOW {
            return false;
        }
        if *used == overflow.len() {
            overflow.push(Page::new());
        }
        *used += 1;
        overflow[*used - 1].atlas.insert(key, coverage).is_ok()
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
            if self.cache.atlas.get(key).is_some() {
                // Asked for again this frame, which keeps it from eviction.
                let _ = self.cache.atlas.insert(
                    key,
                    &Coverage {
                        width: 0,
                        height: 0,
                        texels: Vec::new(),
                    },
                );
                continue;
            }
            if self.page_of(key).is_some() || self.refused.contains(&key) {
                // On an overflow page already, or refused, this frame.
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
            if Self::insert(
                &mut self.cache,
                &mut self.overflow,
                &mut self.overflow_used,
                key,
                &coverage,
            ) {
                self.placed.insert(
                    key,
                    Placement {
                        left: p.left,
                        top: p.top,
                        width: p.width,
                        height: p.height,
                    },
                );
            } else {
                // Past the last page: this glyph's run is drawn as its
                // outline this frame; text never goes missing.
                self.refused.insert(key);
            }
        }
    }

    /// Puts the cache and this frame's overflow pages on the device, each
    /// written if anything went in: once a frame. Overflow pages this frame
    /// did not need are freed.
    pub(crate) fn upload(&mut self, ctx: &mut Context) -> Result<(), RasterError> {
        self.uploads += u64::from(self.cache.upload(ctx)?);
        for page in &mut self.overflow[..self.overflow_used] {
            self.uploads += u64::from(page.upload(ctx)?);
        }
        for mut page in self.overflow.drain(self.overflow_used..) {
            page.clear(ctx);
        }
        // The atlas evicts; the placements of what it evicted go too, once
        // they outnumber the glyphs on the pages, so text that keeps
        // changing keeps this bounded by them.
        let on_pages: usize =
            self.cache.atlas.len() + self.overflow.iter().map(|p| p.atlas.len()).sum::<usize>();
        if self.placed.len() > 2 * on_pages + 1024 {
            let (cache, overflow) = (&self.cache, &self.overflow);
            self.placed.retain(|k, _| {
                cache.atlas.get(*k).is_some() || overflow.iter().any(|p| p.atlas.get(*k).is_some())
            });
        }
        Ok(())
    }

    /// Frees the device images; before the context goes.
    pub(crate) fn clear(&mut self, ctx: &mut Context) {
        self.cache.clear(ctx);
        for mut page in self.overflow.drain(..) {
            page.clear(ctx);
        }
        self.overflow_used = 0;
    }

    /// The run's glyphs as atlas quads, in the canvas's space, each on a
    /// whole device pixel, by the page each is on; `None` if the run is
    /// drawn as outlines.
    pub(crate) fn positioned(
        &self,
        transform: emblema::Transform2D,
        run: &GlyphRun,
        origin: [f32; 2],
    ) -> Option<Vec<(usize, Vec<PositionedGlyph>)>> {
        let m = transform.to_affine();
        if !atlas_route(run, m.as_ref()) {
            return None;
        }
        let m = m?;
        let inv = m.inverse();
        let px = device_size(run, &m).round().max(1.0);
        let font = font_key(run);
        let (sx, sy) = (m.matrix2.x_axis.length(), m.matrix2.y_axis.length());
        let mut out: Vec<(usize, Vec<PositionedGlyph>)> = Vec::new();
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
            let page = self.page_of(key)?;
            if pl.width == 0 || pl.height == 0 {
                continue;
            }
            let at = m.transform_point2(Vec2::new(origin[0] + pos[0], origin[1] + pos[1]));
            let tl = Vec2::new(at.x.round() + pl.left as f32, at.y.round() - pl.top as f32);
            let user = inv.transform_point2(tl);
            let quad = PositionedGlyph {
                key,
                position: [user.x, user.y],
                size: [pl.width as f32 / sx, pl.height as f32 / sy],
            };
            match out.iter_mut().find(|(p, _)| *p == page) {
                Some((_, quads)) => quads.push(quad),
                None => out.push((page, vec![quad])),
            }
        }
        // The cache's glyphs first; most runs are on it alone.
        out.sort_by_key(|(p, _)| *p);
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
        let pages = g.positioned(scale(2.0), &r, [10.3, 20.7]).unwrap();
        assert_eq!(pages.len(), 1, "two glyphs, on the cache alone");
        let (page, q) = &pages[0];
        assert_eq!(*page, 0);
        assert_eq!(q.len(), 2);
        for p in q {
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

    /// `n` distinct glyphs of Roboto at 250 device pixels, one run.
    fn large_glyphs(n: u16) -> Arc<GlyphRun> {
        let mut r = (*run("A", 250.0)).clone();
        r.glyphs = (1..=n).collect();
        r.positions = (0..n).map(|i| [f32::from(i) * 300.0, 0.0]).collect();
        Arc::new(r)
    }

    /// The atlas's side limit, crossed (brief M3 §15, atlas full): one
    /// frame of 1,000 distinct glyphs at 250 device pixels, more than a
    /// 4096 atlas holds. The cache grows to its limit and no further; the rest
    /// go on an overflow page, and the run is drawn from both, every
    /// glyph from an atlas. The next frame keeps the cache alone.
    #[test]
    #[ignore = "app lane: needs FLUTTER_ROOT"]
    fn a_frame_past_the_atlas_limit_is_drawn_from_overflow_pages() {
        let r = large_glyphs(1000);
        let mut g = Glyphs::default();
        g.begin_frame();
        g.prepare(scale(1.0), &r);
        assert_eq!(g.atlas().size(), ATLAS_LIMIT);
        assert!(g.refused.is_empty(), "{} refused", g.refused.len());
        assert!(g.overflow_pages() >= 1);
        let pages = g.positioned(scale(1.0), &r, [0.0, 0.0]).unwrap();
        assert!(
            pages.len() >= 2 && pages[0].0 == 0,
            "{:?}",
            pages.iter().map(|p| p.0).collect::<Vec<_>>()
        );
        let drawn: usize = pages.iter().map(|(_, q)| q.len()).sum();
        let blank = r
            .glyphs
            .iter()
            .filter(|gl| {
                g.placed
                    .iter()
                    .any(|(k, p)| k.glyph == **gl && (p.width == 0 || p.height == 0))
            })
            .count();
        assert_eq!(drawn + blank, r.glyphs.len(), "a glyph went missing");
        assert!(
            g.placements()
                <= g.atlas().len()
                    + g.overflow.iter().map(|p| p.atlas.len()).sum::<usize>()
                    + blank
        );

        g.begin_frame();
        assert_eq!(g.overflow_pages(), 0, "the overflow outlived its frame");
        let small = run("Hi", 14.0);
        g.prepare(scale(1.0), &small);
        assert_eq!(g.overflow_pages(), 0);
    }

    /// Past the last overflow page, a run with a glyph no page could take
    /// is drawn whole as outlines: text never goes missing. The same 1,250
    /// glyphs at three sizes, three keys each, more than four pages hold.
    #[test]
    #[ignore = "app lane: needs FLUTTER_ROOT"]
    fn a_frame_past_every_page_draws_the_rest_as_outlines() {
        let r = large_glyphs(1250);
        let mut g = Glyphs::default();
        g.begin_frame();
        let scales = [1.0, 0.98, 0.96];
        for s in scales {
            g.prepare(scale(s), &r);
        }
        assert_eq!(g.overflow_pages(), MAX_OVERFLOW);
        assert!(!g.refused.is_empty());
        let last = scale(scales[2]);
        assert!(g.positioned(last, &r, [0.0, 0.0]).is_none());
        assert!(g.outlines(&r, [0.0, 0.0]).is_some());
        // The first size went in whole, and is drawn from the pages.
        assert!(g.positioned(scale(scales[0]), &r, [0.0, 0.0]).is_some());
    }

    /// A frame of `run` from the origin in opaque black on transparent, as
    /// the raster thread draws it; with the glyph cache it ended with.
    fn frame(run: Arc<GlyphRun>) -> (Vec<u8>, crate::Offscreen) {
        use fl_ui::{Draw, LayerOp, Paint, Picture, Scene};
        let picture = Picture {
            draws: vec![Draw::Glyphs {
                run,
                origin: [0.0, 0.0],
                paint: Paint::default(),
            }],
        };
        let scene = Scene {
            ops: vec![LayerOp::AddPicture {
                dx: 0.0,
                dy: 0.0,
                picture: Arc::new(picture),
            }],
        };
        let mut target = crate::Offscreen::new(300, 300).unwrap();
        let pixels = target.render(&scene).unwrap();
        (pixels, target)
    }

    /// The second pass on the device: a glyph the cache had no room for is
    /// drawn from its overflow page, its own image in its own slot, exactly
    /// as the same glyph drawn alone from the cache. The other 999 are
    /// placed far outside the target; they only fill the cache.
    #[test]
    #[ignore = "app lane: needs FLUTTER_ROOT and a Vulkan driver"]
    fn a_glyph_on_an_overflow_page_draws_as_it_does_from_the_cache() {
        let mut crowd = (*large_glyphs(1000)).clone();
        let last = crowd.glyphs.len() - 1;
        for p in &mut crowd.positions[..last] {
            *p = [-100_000.0, 0.0];
        }
        crowd.positions[last] = [20.0, 260.0];
        let mut alone = crowd.clone();
        alone.glyphs = vec![crowd.glyphs[last]];
        alone.positions = vec![[20.0, 260.0]];

        let (crowded, target) = frame(Arc::new(crowd.clone()));
        let key = GlyphKey {
            font: font_key(&crowd),
            glyph: crowd.glyphs[last],
            size: 250,
        };
        assert!(
            target.glyphs.page_of(key).is_some_and(|p| p > 0),
            "the glyph is not on an overflow page"
        );
        let (reference, target) = frame(Arc::new(alone));
        assert_eq!(target.glyphs.page_of(key), Some(0));
        assert!(reference.chunks(4).any(|p| p[3] > 0), "nothing drawn");
        assert!(crowded == reference, "the overflow page drew differently");
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
