//! What a laid out paragraph paints: SkParagraph's `TextLine::paint` and
//! `Decorations`, as the operations Flutter's `DisplayListParagraphPainter`
//! records with Impeller on: rects for backgrounds, glyph runs for text and
//! their shadows, filled rects, dashed lines and a wavy path for
//! decorations, inside saves, translations and clips.
//!
//! Positions are in the paragraph's coordinates plus the paint offset; the
//! glyphs themselves are drawn by the raster side.

use std::sync::Arc;

use crate::font::Typeface;
use crate::line::{
    iterate_visual_runs, measure_text_inside_one_run, Adjustment, ClipContext, TextLine,
};
use crate::paragraph::{Paragraph, Range, Run, EMPTY_INDEX};
use crate::shape::FontMetrics;
use crate::style::{decoration, DecorationStyle, PaintId, TextStyle};

/// A paint a text operation draws with: a color, or one of the paints the
/// paragraph's styles named (foreground, background), by index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintRef {
    /// `0xAARRGGBB`.
    Color(u32),
    Id(PaintId),
}

/// Glyphs in one face at one size, positioned from an origin.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRun {
    pub typeface: Typeface,
    pub font_size: f32,
    pub glyphs: Vec<u16>,
    /// Each glyph's origin, relative to the run's.
    pub positions: Vec<[f32; 2]>,
}

/// One drawing operation.
#[derive(Debug, Clone)]
pub enum PaintOp {
    Save,
    Restore,
    Translate([f32; 2]),
    /// Left, top, right, bottom; intersected, not antialiased.
    ClipRect([f32; 4]),
    /// A background: the rect filled with a style's background paint.
    Rect {
        rect: [f32; 4],
        paint: PaintId,
    },
    /// Text.
    Glyphs {
        run: Arc<GlyphRun>,
        origin: [f32; 2],
        paint: PaintRef,
    },
    /// A text shadow: the glyphs in `color`, blurred by `blur_sigma`.
    Shadow {
        run: Arc<GlyphRun>,
        origin: [f32; 2],
        color: u32,
        blur_sigma: f32,
    },
    /// A decoration drawn as a rect, antialiased.
    FilledRect {
        rect: [f32; 4],
        color: u32,
    },
    /// A decoration line, stroked `width` wide, dashed `on`, `off` if set
    /// (`DrawDashedLine`).
    Line {
        from: [f32; 2],
        to: [f32; 2],
        color: u32,
        width: f32,
        dash: Option<[f32; 2]>,
    },
    /// A wavy decoration: from `start`, quadratic segments (control,
    /// end), stroked `width` wide.
    Wave {
        start: [f32; 2],
        quads: Vec<([f32; 2], [f32; 2])>,
        color: u32,
        width: f32,
    },
}

/// The style attribute a pass over a run splits it by
/// (`TextStyle::matchOneAttribute`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleType {
    Foreground,
    Background,
    Shadow,
    Decorations,
}

fn match_one(a: &TextStyle, b: &TextStyle, t: StyleType) -> bool {
    match t {
        StyleType::Foreground => match (a.foreground, b.foreground) {
            (None, None) => a.color == b.color,
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
        StyleType::Background => match (a.background, b.background) {
            (None, None) => true,
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
        StyleType::Shadow => a.shadows == b.shadows,
        StyleType::Decorations => {
            a.decoration == b.decoration
                && a.decoration_color == b.decoration_color
                && a.decoration_style == b.decoration_style
                && a.decoration_thickness == b.decoration_thickness
        }
    }
}

fn offset(r: [f32; 4], dx: f32, dy: f32) -> [f32; 4] {
    [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy]
}

/// `iterateThroughSingleRunByStyles`: the run's text in the line split
/// where the attribute `t` changes; the visitor gets each stretch, its
/// style and its context. Returns the run's width in the line.
#[allow(clippy::too_many_arguments)]
fn by_styles(
    p: &Paragraph,
    line: &TextLine,
    run: &Run,
    run_offset: f32,
    text_range: Range,
    t: StyleType,
    visitor: &mut dyn FnMut(Range, &TextStyle, &ClipContext),
) -> f32 {
    let correct = |r: Range, text_offset: f32| -> ClipContext {
        let mut c = measure_text_inside_one_run(
            p,
            line,
            r,
            run,
            run_offset,
            text_offset,
            false,
            Adjustment::GlyphCluster,
        );
        if t == StyleType::Decorations {
            c.clip[1] = line.sizes.run_top(run, false);
            c.clip[3] = c.clip[1] + run.calculate_height(false, false);
        }
        c
    };
    let blocks =
        &p.blocks[line.blocks.start.min(p.blocks.len())..line.blocks.end.min(p.blocks.len())];
    if run.ellipsis {
        let ctx = correct(run.text_range, 0.0);
        let test = Range::new(
            run.cluster_start,
            run.cluster_start + run.text_range.width(),
        );
        for b in blocks {
            let i = b.range.intersected(test);
            if i.start != EMPTY_INDEX && i.width() > 0 {
                visitor(test, &b.style, &ctx);
                return run.advance()[0];
            }
        }
        return run.advance()[0];
    }
    let mut start = EMPTY_INDEX;
    let mut size = 0usize;
    let mut prev: Option<&TextStyle> = None;
    let mut text_offset_in_run = 0.0f32;
    let n = blocks.len();
    let mut index = 0;
    while index <= n {
        let mut intersect = Range::new(0, 0);
        let mut style: Option<&TextStyle> = None;
        if index < n {
            let b = if run.left_to_right() {
                &blocks[index]
            } else {
                &blocks[n - index - 1]
            };
            intersect = b.range.intersected(text_range);
            if intersect.start == EMPTY_INDEX || intersect.width() == 0 {
                if start == EMPTY_INDEX {
                    index += 1;
                    continue;
                }
                intersect = Range::new(start, start + size);
                index = n;
            } else {
                style = Some(&b.style);
                match prev {
                    Some(ps) if start != EMPTY_INDEX && match_one(&b.style, ps, t) => {
                        size += intersect.width();
                        start = start.min(intersect.start);
                        index += 1;
                        continue;
                    }
                    _ if start == EMPTY_INDEX => {
                        prev = Some(&b.style);
                        size = intersect.width();
                        start = intersect.start;
                        index += 1;
                        continue;
                    }
                    _ => {}
                }
            }
        } else if prev.is_none() {
            break;
        }
        let stretch = Range::new(start, start + size);
        let ctx = correct(stretch, text_offset_in_run);
        text_offset_in_run += ctx.clip[2] - ctx.clip[0];
        if ctx.clip[3] - ctx.clip[1] != 0.0 {
            if let Some(ps) = prev {
                visitor(stretch, ps, &ctx);
            }
        }
        prev = style;
        start = intersect.start;
        size = intersect.width();
        index += 1;
    }
    text_offset_in_run
}

/// `Run::copyTo`: glyphs `pos` to `pos + size`, each at its position plus
/// its offset and justification shift.
fn glyph_run(run: &Run, pos: usize, size: usize) -> Option<Arc<GlyphRun>> {
    let tf = run.typeface.clone()?;
    let end = (pos + size).min(run.size());
    let glyphs = run.glyphs[pos..end]
        .iter()
        .map(|g| u16::try_from(*g).unwrap_or(0))
        .collect();
    let positions = (pos..end)
        .map(|i| {
            let mut x = run.positions[i][0] + run.offsets[i][0];
            if let Some(s) = run.justification_shifts.get(i) {
                x += s[0];
            }
            [x, run.positions[i][1] + run.offsets[i][1]]
        })
        .collect();
    Some(Arc::new(GlyphRun {
        typeface: tf,
        font_size: run.font_size,
        glyphs,
        positions,
    }))
}

/// `extendHeight`.
fn extend_height(line: &TextLine, ctx: &ClipContext) -> [f32; 4] {
    let mut r = ctx.clip;
    r[3] += (line.max_run_metrics.height() - line.height()).max(0.0);
    r
}

/// The baseline glyphs sit on, rounded as Skia rounds it.
fn corrected_baseline(line: &TextLine) -> f32 {
    (line.baseline() + 0.5).floor()
}

impl Paragraph {
    /// What `paint(x, y)` draws, in order.
    pub fn paint_ops(&self, x: f32, y: f32) -> Vec<PaintOp> {
        let mut ops = Vec::new();
        for li in 0..self.lines.len() {
            paint_line(self, li, x, y, &mut ops);
        }
        ops
    }
}

/// `TextLine::paint`.
fn paint_line(p: &Paragraph, li: usize, x: f32, y: f32, ops: &mut Vec<PaintOp>) {
    let line = &p.lines[li];
    let blocks =
        &p.blocks[line.blocks.start.min(p.blocks.len())..line.blocks.end.min(p.blocks.len())];
    let has_background = blocks.iter().any(|b| b.style.background.is_some());
    let has_shadows = blocks.iter().any(|b| !b.style.shadows.is_empty());
    let has_decorations = blocks.iter().any(|b| b.style.decoration != 0);
    let off = line.offset();
    let pass = |t: StyleType, f: &mut dyn FnMut(&Run, &ClipContext, &TextStyle)| {
        iterate_visual_runs(p, line, false, |_ri, run, run_offset, text_range| {
            let w = by_styles(
                p,
                line,
                run,
                run_offset,
                text_range,
                t,
                &mut |_r, style, ctx| f(run, ctx, style),
            );
            (w, true)
        });
    };
    if has_background {
        pass(StyleType::Background, &mut |_run, ctx, style| {
            if let Some(id) = style.background {
                ops.push(PaintOp::Rect {
                    rect: offset(ctx.clip, off[0] + x, off[1] + y),
                    paint: id,
                });
            }
        });
    }
    if has_shadows {
        let baseline = corrected_baseline(line);
        pass(StyleType::Shadow, &mut |run, ctx, style| {
            for s in &style.shadows {
                // `hasShadow`: an offset or a blur.
                if s.offset == [0.0, 0.0] && s.blur_sigma == 0.0 {
                    continue;
                }
                let Some(g) = glyph_run(run, ctx.pos, ctx.size) else {
                    continue;
                };
                if ctx.clipping_needed {
                    ops.push(PaintOp::Save);
                    ops.push(PaintOp::ClipRect(offset(
                        extend_height(line, ctx),
                        off[0] + x,
                        off[1] + y,
                    )));
                }
                ops.push(PaintOp::Shadow {
                    run: g,
                    origin: [
                        x + off[0] + s.offset[0] + ctx.text_shift,
                        y + off[1] + s.offset[1] + baseline,
                    ],
                    color: s.color,
                    blur_sigma: s.blur_sigma as f32,
                });
                if ctx.clipping_needed {
                    ops.push(PaintOp::Restore);
                }
            }
        });
    }
    text_blobs(p, line, x, y, ops);
    if has_decorations {
        pass(StyleType::Decorations, &mut |run, ctx, style| {
            ops.push(PaintOp::Save);
            ops.push(PaintOp::Translate([x + off[0], y + off[1]]));
            decorations(style, run, ctx, ops);
            ops.push(PaintOp::Restore);
        });
    }
}

/// `ensureTextBlobCachePopulated` and the records' `paint`.
fn text_blobs(p: &Paragraph, line: &TextLine, x: f32, y: f32, ops: &mut Vec<PaintOp>) {
    let off = line.offset();
    let baseline = corrected_baseline(line);
    let mut blob = |run: &Run, style: &TextStyle, ctx: &ClipContext| {
        if run.is_placeholder() {
            return;
        }
        let Some(g) = glyph_run(run, ctx.pos, ctx.size) else {
            return;
        };
        let paint = match style.foreground {
            Some(id) => PaintRef::Id(id),
            None => PaintRef::Color(style.color),
        };
        let origin = [x + off[0] + ctx.text_shift, y + off[1] + baseline];
        if ctx.clipping_needed {
            ops.push(PaintOp::Save);
            ops.push(PaintOp::ClipRect(offset(
                extend_height(line, ctx),
                off[0] + x,
                off[1] + y,
            )));
        }
        ops.push(PaintOp::Glyphs {
            run: g,
            origin,
            paint,
        });
        if ctx.clipping_needed {
            ops.push(PaintOp::Restore);
        }
    };
    let blocks =
        &p.blocks[line.blocks.start.min(p.blocks.len())..line.blocks.end.min(p.blocks.len())];
    // One block, one run: the line's glyphs in one record.
    if blocks.len() == 1 && line.runs_in_visual_order.len() == 1 && line.ellipsis.is_none() {
        let run = &p.runs[line.runs_in_visual_order[0]];
        if !run.is_placeholder() {
            if line.clusters.width() == 0 {
                return;
            }
            let (s, e) = (
                &p.clusters[line.clusters.start],
                &p.clusters[line.clusters.end - 1],
            );
            let (gs, ge) = if run.left_to_right() {
                (s.start, if e.is_hard_break { e.start } else { e.end })
            } else {
                (e.start, if s.is_hard_break { s.start } else { s.end })
            };
            let top = line.sizes.run_top(run, line.ascent_typographic);
            let ctx = ClipContext {
                pos: gs,
                size: ge.saturating_sub(gs),
                text_shift: -run.position_x(gs),
                clip: [
                    0.0,
                    top,
                    line.advance[0],
                    top + run.calculate_height(line.ascent_typographic, line.descent_typographic),
                ],
                excluded_trailing_spaces: 0.0,
                clipping_needed: false,
            };
            blob(run, &blocks[0].style, &ctx);
            return;
        }
    }
    iterate_visual_runs(p, line, false, |_ri, run, run_offset, text_range| {
        if run.is_placeholder() {
            return (run.advance()[0], true);
        }
        let w = by_styles(
            p,
            line,
            run,
            run_offset,
            text_range,
            StyleType::Foreground,
            &mut |_r, style, ctx| blob(run, style, ctx),
        );
        (w, true)
    });
}

/// The spacing of a double decoration's second line.
const DOUBLE_SPACING: f32 = 3.0;

/// `Decorations::paint`, in the line's coordinates.
fn decorations(style: &TextStyle, run: &Run, ctx: &ClipContext, ops: &mut Vec<PaintOp>) {
    if style.decoration == 0 {
        return;
    }
    let Some(tf) = run.typeface.as_ref() else {
        return;
    };
    let m = FontMetrics::of(tf, style.font_size);
    // `calculateThickness`.
    let mut thickness = style.font_size / 14.0;
    if let Some(t) = m.underline_thickness.filter(|t| *t > 0.0) {
        thickness = t;
    }
    if style.decoration == decoration::LINE_THROUGH {
        if let Some(t) = m.strikeout_thickness.filter(|t| *t > 0.0) {
            thickness = t;
        }
    }
    thickness *= style.decoration_thickness as f32;
    // `calculatePaint`.
    let scale = style.font_size / 14.0;
    let dash = match style.decoration_style {
        DecorationStyle::Dotted => Some([1.0 * scale, 1.5 * scale]),
        DecorationStyle::Dashed => Some([4.0 * scale, 2.0 * scale]),
        _ => None,
    };
    let color = if style.decoration_color == 0 {
        style.color
    } else {
        style.decoration_color
    };
    for d in [
        decoration::UNDERLINE,
        decoration::OVERLINE,
        decoration::LINE_THROUGH,
    ] {
        if style.decoration & d == 0 {
            continue;
        }
        // `calculatePosition`.
        let ascent = if d == decoration::OVERLINE {
            run.correct_ascent() - run.ascent()
        } else {
            run.correct_ascent()
        };
        let position = match d {
            decoration::UNDERLINE => {
                m.underline_position
                    .filter(|p| *p > 0.0)
                    .unwrap_or(thickness)
                    - ascent
            }
            decoration::OVERLINE => -ascent,
            _ => m.strikeout_position.unwrap_or(m.x_height / -2.0) - ascent,
        };
        let width = ctx.clip[2] - ctx.clip[0];
        let x = ctx.clip[0];
        let y = ctx.clip[1] + position;
        let r = thickness * 0.5;
        match style.decoration_style {
            DecorationStyle::Wavy => ops.push(wave(x, y, width, thickness, color)),
            DecorationStyle::Double => {
                ops.push(PaintOp::FilledRect {
                    rect: [x, y - r, x + width, y + r],
                    color,
                });
                let b = y + DOUBLE_SPACING;
                ops.push(PaintOp::FilledRect {
                    rect: [x, b - r, x + width, b + r],
                    color,
                });
            }
            DecorationStyle::Dashed | DecorationStyle::Dotted => ops.push(PaintOp::Line {
                from: [x, y],
                to: [x + width, y],
                color,
                width: thickness,
                dash,
            }),
            DecorationStyle::Solid => ops.push(PaintOp::FilledRect {
                rect: [x, y - r, x + width, y + r],
                color,
            }),
        }
    }
}

/// `calculateWaves`: quarter waves of the thickness, the last one cut to
/// fit, from `(x, y)`.
fn wave(x: f32, y: f32, width: f32, thickness: f32, color: u32) -> PaintOp {
    let q = thickness;
    let mut quads = Vec::new();
    let mut cur = [x, y];
    let mut count = 0usize;
    let mut x_start = 0.0f32;
    while x_start + q * 2.0 < width {
        let dy = if count % 2 != 0 { q } else { -q };
        let ctrl = [cur[0] + q, cur[1] + dy];
        let end = [cur[0] + q * 2.0, cur[1]];
        quads.push((ctrl, end));
        cur = end;
        x_start += q * 2.0;
        count += 1;
    }
    let remaining = width - x_start;
    if remaining > 0.0 {
        let sign = if count % 2 == 0 { -1.0 } else { 1.0 };
        let (x1, y1) = (
            f64::from(remaining) / 2.0,
            f64::from(remaining) / 2.0 * sign,
        );
        let x2 = f64::from(remaining);
        let y2 = (f64::from(remaining)
            - f64::from(remaining) * f64::from(remaining) / (f64::from(q) * 2.0))
            * sign;
        quads.push((
            [cur[0] + x1 as f32, cur[1] + y1 as f32],
            [cur[0] + x2 as f32, cur[1] + y2 as f32],
        ));
    }
    PaintOp::Wave {
        start: [x, y],
        quads,
        color,
        width: thickness,
    }
}
