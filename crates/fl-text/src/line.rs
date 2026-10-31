//! A laid out line: SkParagraph's `TextLine`. Lines live in the paragraph
//! and refer to its runs and clusters by index, so what needs both is a
//! function of the paragraph and a line index.

use crate::font::Typeface;
use crate::paragraph::{LineMetrics, Paragraph, Range, Run, EMPTY_INDEX};
use crate::shape;
use crate::style::{TextAlign, TextDirection};
use crate::unicode;
use crate::wrapper::LineSpec;

#[derive(Debug, Clone)]
pub(crate) struct TextLine {
    pub blocks: Range,
    pub text_excluding_spaces: Range,
    pub text: Range,
    pub text_with_newlines: Range,
    pub clusters: Range,
    pub ghosts: Range,
    pub runs_in_visual_order: Vec<usize>,
    pub advance: [f32; 2],
    pub offset: [f32; 2],
    pub shift: f32,
    pub width_with_spaces: f32,
    pub ellipsis: Option<Run>,
    pub sizes: LineMetrics,
    pub max_run_metrics: LineMetrics,
    pub ascent_typographic: bool,
    pub descent_typographic: bool,
}

/// `RectHeightStyle`, dart:ui's `BoxHeightStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RectHeightStyle {
    Tight,
    Max,
    IncludeLineSpacingMiddle,
    IncludeLineSpacingTop,
    IncludeLineSpacingBottom,
    Strut,
}

impl RectHeightStyle {
    pub fn from_index(i: u32) -> RectHeightStyle {
        match i {
            1 => RectHeightStyle::Max,
            2 => RectHeightStyle::IncludeLineSpacingMiddle,
            3 => RectHeightStyle::IncludeLineSpacingTop,
            4 => RectHeightStyle::IncludeLineSpacingBottom,
            5 => RectHeightStyle::Strut,
            _ => RectHeightStyle::Tight,
        }
    }
}

/// `RectWidthStyle`, dart:ui's `BoxWidthStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RectWidthStyle {
    Tight,
    Max,
}

/// A box of text, its rectangle (left, top, right, bottom) and direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBox {
    pub rect: [f32; 4],
    pub direction: TextDirection,
}

/// A text position and its affinity (downstream is false).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionWithAffinity {
    pub position: usize,
    pub upstream: bool,
}

/// `skt::LineMetrics`, the UTF-16 indices and the measures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMetricsInfo {
    pub start_index: usize,
    pub end_index: usize,
    pub end_excluding_whitespace: usize,
    pub end_including_newline: usize,
    pub hard_break: bool,
    pub ascent: f64,
    pub descent: f64,
    pub unscaled_ascent: f64,
    pub height: f64,
    pub width: f64,
    pub left: f64,
    pub baseline: f64,
    pub line_number: usize,
}

/// `ClipContext`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ClipContext {
    pub pos: usize,
    pub size: usize,
    pub text_shift: f32,
    /// left, top, right, bottom.
    pub clip: [f32; 4],
    pub excluded_trailing_spaces: f32,
    pub clipping_needed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Adjustment {
    GlyphCluster,
    GraphemeGluster,
}

fn width(r: &[f32; 4]) -> f32 {
    r[2] - r[0]
}

fn height(r: &[f32; 4]) -> f32 {
    r[3] - r[1]
}

fn nearly_zero(v: f32) -> bool {
    v.abs() <= 1.0 / 4096.0
}

fn nearly_equal(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1.0 / 4096.0 || (a == b)
}

/// `compareRound`, the rounding hack off.
fn compare_round(a: f32, b: f32) -> i32 {
    let base = a.abs().max(b.abs());
    let diff = (a - b).abs();
    if nearly_zero(base) || diff / base < 0.001 {
        return 0;
    }
    if a < b {
        -1
    } else {
        1
    }
}

impl TextLine {
    /// `TextLine::TextLine` from the wrapper's line.
    pub(crate) fn from_spec(p: &Paragraph, s: LineSpec) -> TextLine {
        let blocks = p.find_all_blocks(s.text_excluding_spaces);
        let mut line = TextLine {
            blocks,
            text_excluding_spaces: s.text_excluding_spaces,
            text: s.text,
            text_with_newlines: s.text_with_newlines,
            clusters: s.clusters,
            ghosts: s.clusters_with_ghosts,
            runs_in_visual_order: Vec::new(),
            advance: s.advance,
            offset: s.offset,
            shift: 0.0,
            width_with_spaces: s.width_with_spaces,
            ellipsis: None,
            sizes: s.metrics,
            max_run_metrics: LineMetrics::clean_with(false),
            ascent_typographic: false,
            descent_typographic: false,
        };
        if line.ghosts.width() == 0 {
            return line;
        }
        let (Some(sc), Some(ec)) = (
            p.clusters.get(line.ghosts.start),
            p.clusters.get(line.ghosts.end - 1),
        ) else {
            return line;
        };
        let (first, last) = (sc.run_index, ec.run_index);
        if first == EMPTY_INDEX || last == EMPTY_INDEX || last < first {
            return line;
        }
        let mut levels = Vec::with_capacity(last - first + 1);
        let mut placeholders = Vec::new();
        for ri in first..=last {
            let run = &p.runs[ri];
            levels.push(run.bidi_level);
            line.max_run_metrics.add(&LineMetrics::new(
                run.correct_ascent(),
                run.correct_descent(),
                run.font_metrics.leading,
            ));
            if run.is_placeholder() {
                placeholders.push(ri);
            }
        }
        let mut ph = placeholders.into_iter();
        for index in unicode::reorder_visual(&levels) {
            let ri = first + index;
            if p.runs[ri].is_placeholder() {
                if let Some(x) = ph.next() {
                    line.runs_in_visual_order.push(x);
                }
            } else {
                line.runs_in_visual_order.push(ri);
            }
        }
        for ci in line.ghosts.start..line.ghosts.end {
            let c = &p.clusters[ci];
            let Some(run) = p.runs.get(c.run_index) else {
                continue;
            };
            if !run.is_placeholder() && !run.is_cursive_script(&p.text) {
                line.shift += c.half_letter_spacing;
                break;
            }
        }
        line
    }

    pub fn width(&self) -> f32 {
        self.advance[0] + self.ellipsis.as_ref().map_or(0.0, |e| e.advance[0])
    }

    pub fn height(&self) -> f32 {
        self.advance[1]
    }

    pub fn spaces_width(&self) -> f32 {
        self.width_with_spaces - self.width()
    }

    pub fn offset(&self) -> [f32; 2] {
        [self.offset[0] + self.shift, self.offset[1]]
    }

    pub fn baseline(&self) -> f32 {
        self.sizes.baseline()
    }

    fn ends_with_hard_line_break(&self, p: &Paragraph) -> bool {
        (self.ghosts.width() > 0 && p.clusters[self.ghosts.end - 1].is_hard_break)
            || self.ellipsis.is_some()
            || self.ghosts.end == p.clusters.len() - 1
    }
}

/// `format`.
pub(crate) fn format(p: &mut Paragraph, li: usize, align: TextAlign, max_width: f32) {
    let delta = max_width - p.lines[li].width();
    if delta <= 0.0 {
        return;
    }
    match align {
        TextAlign::Justify => {
            if !p.lines[li].ends_with_hard_line_break(p) {
                justify(p, li, max_width);
            } else if p.style.text_direction == TextDirection::Rtl {
                p.lines[li].shift = delta;
            }
        }
        TextAlign::Right => p.lines[li].shift = delta,
        TextAlign::Center => p.lines[li].shift = delta / 2.0,
        _ => {}
    }
}

/// `iterateThroughClustersInGlyphsOrder`, forward: each cluster, its
/// index and whether it is a ghost.
fn clusters_in_glyph_order(
    p: &Paragraph,
    li: usize,
    include_ghosts: bool,
) -> Vec<(usize, usize, bool)> {
    let line = &p.lines[li];
    let mut out = Vec::new();
    let mut index = 0;
    for &ri in &line.runs_in_visual_order {
        let run = &p.runs[ri];
        let trimmed = line.clusters.intersection(run.cluster_range);
        let trailed = line.ghosts.intersection(run.cluster_range);
        if trailed.end < trailed.start {
            continue;
        }
        let trimmed_end = trimmed.end.max(trimmed.start);
        let order: Box<dyn Iterator<Item = usize>> = if run.left_to_right() {
            Box::new(trailed.start..trailed.end)
        } else {
            Box::new((trailed.start..trailed.end).rev())
        };
        for ci in order {
            let ghost = ci >= trimmed_end;
            if !include_ghosts && ghost {
                continue;
            }
            out.push((ci, index, ghost));
            index += 1;
        }
    }
    out
}

/// `justify`.
fn justify(p: &mut Paragraph, li: usize, max_width: f32) {
    let mut patches: i32 = 0;
    let mut text_len = 0.0f32;
    let mut whitespace_len = 0.0f32;
    let mut patch = false;
    let mut leading = false;
    for (ci, index, _) in clusters_in_glyph_order(p, li, false) {
        let c = &p.clusters[ci];
        if c.is_whitespace_break {
            if index == 0 {
                leading = true;
            } else if !patch && !leading {
                patches += 1;
            }
            patch = !leading;
            whitespace_len += c.width;
        } else if c.is_ideographic {
            if !patch && index != 0 {
                patches += 1;
            }
            patch = true;
            leading = false;
            patches += 1;
        } else {
            patch = false;
            leading = false;
        }
        text_len += c.width;
    }
    if patch {
        patches -= 1;
    }
    if patches <= 0 {
        if p.style.text_direction == TextDirection::Rtl {
            p.lines[li].shift = max_width - text_len;
        }
        return;
    }
    let step = (max_width - text_len + whitespace_len) / patches as f32;
    let mut shift = 0.0f32;
    let mut prev_shift = 0.0f32;
    let ghost_shift = max_width - p.lines[li].advance[0];
    patch = false;
    leading = false;
    for (ci, index, ghost) in clusters_in_glyph_order(p, li, true) {
        let (ws, ideo, w, ri) = {
            let c = &p.clusters[ci];
            (
                c.is_whitespace_break,
                c.is_ideographic,
                c.width,
                c.run_index,
            )
        };
        if ghost {
            if p.runs[ri].left_to_right() {
                shift_cluster(p, ci, ghost_shift, ghost_shift);
            }
            continue;
        }
        if ws {
            if index == 0 {
                leading = true;
            } else if !patch && !leading {
                shift += step;
                patch = true;
            }
            shift -= w;
        } else if ideo {
            if !patch && index != 0 {
                shift += step;
            }
            patch = false;
            leading = false;
        } else {
            patch = false;
            leading = false;
        }
        shift_cluster(p, ci, shift, prev_shift);
        prev_shift = shift;
        if !ws && ideo {
            shift += step;
            patch = true;
        }
    }
    let line = &mut p.lines[li];
    line.width_with_spaces += ghost_shift;
    line.advance[0] = max_width;
}

fn shift_cluster(p: &mut Paragraph, ci: usize, shift: f32, prev: f32) {
    let (s, mut e, ri) = {
        let c = &p.clusters[ci];
        (c.start, c.end, c.run_index)
    };
    let Some(run) = p.runs.get_mut(ri) else {
        return;
    };
    if e == run.size() {
        e += 1;
    }
    if run.justification_shifts.is_empty() {
        run.justification_shifts = vec![[0.0, 0.0]; run.size() + 1];
    }
    for pos in s..e.min(run.justification_shifts.len()) {
        run.justification_shifts[pos] = [shift, prev];
    }
}

/// `createEllipsis`: clusters dropped from the end until the ellipsis
/// fits.
pub(crate) fn create_ellipsis(p: &mut Paragraph, li: usize, max_width: f32) {
    let ellipsis_text = p.style.ellipsis.clone();
    let mut width = p.lines[li].advance[0];
    let mut last_run = EMPTY_INDEX;
    let mut ellipsis_run: Option<Run> = None;
    let mut found = None;
    let ghosts = p.lines[li].ghosts;
    let mut ci = ghosts.end;
    while ci > ghosts.start {
        let c = p.clusters[ci - 1].clone();
        if last_run != c.run_index {
            let run = shape_ellipsis(p, li, &ellipsis_text, ci - 1);
            if run.advance[0] > max_width {
                last_run = EMPTY_INDEX;
                ellipsis_run = Some(run);
                ci -= 1;
                continue;
            }
            last_run = c.run_index;
            ellipsis_run = Some(run);
        }
        let adv = ellipsis_run.as_ref().map_or(0.0, |r| r.advance[0]);
        if width + adv > max_width {
            width -= c.width;
            ci -= 1;
            continue;
        }
        found = Some((ci, c.text_range));
        break;
    }
    let line = &mut p.lines[li];
    match (found, ellipsis_run) {
        (Some((ci, tr)), Some(mut run)) => {
            line.advance[0] = width;
            run.cluster_start = tr.start;
            line.ellipsis = Some(run);
            line.clusters.end = ci;
            line.ghosts.end = ci;
            line.text.end = tr.end;
            line.text_with_newlines.end = tr.end;
            line.text_excluding_spaces.end = tr.end;
        }
        _ => {
            line.clusters.end = line.clusters.start;
            line.ghosts.end = line.clusters.start;
            line.text.end = line.text.start;
            line.text_with_newlines.end = line.text_with_newlines.start;
            line.text_excluding_spaces.end = line.text_excluding_spaces.start;
            line.advance[0] = 0.0;
        }
    }
}

/// `shapeEllipsis`: in the cluster's run's face, else the style's
/// faces, else a fallback for its first character.
fn shape_ellipsis(p: &Paragraph, li: usize, ellipsis: &str, ci: usize) -> Run {
    let c = &p.clusters[ci];
    let run = &p.runs[c.run_index];
    let line = &p.lines[li];
    let mut style = &p.style.text_style;
    for b in &p.blocks[line.blocks.start.min(p.blocks.len())..line.blocks.end.min(p.blocks.len())] {
        if (run.left_to_right() && c.text_range.end <= b.range.end)
            || (!run.left_to_right() && c.text_range.start <= b.range.end)
        {
            style = &b.style;
            break;
        }
    }
    let make = |tf: Typeface| -> Run {
        let s = shape::shape(&tf, style.font_size, ellipsis, false, &style.locale, &[]);
        let n = s.glyphs.len();
        let mut r = Run::new(
            Some(tf),
            style.font_size,
            0,
            [s.advance, 0.0],
            n,
            (0, ellipsis.len()),
            0,
            run.height_multiplier,
            run.use_half_leading,
            run.baseline_shift,
            0.0,
        );
        for i in 0..=n {
            if i < n {
                r.glyphs[i] = s.glyphs[i];
                r.cluster_indexes[i] = s.clusters[i] as usize;
            }
            r.positions[i] = s.positions[i];
            r.offsets[i] = s.offsets[i];
        }
        r.ellipsis = true;
        r
    };
    let variations = style.variations();
    let mut last = None;
    if let Some(tf) = run.typeface.clone() {
        let r = make(tf);
        if r.is_resolved() {
            return r;
        }
        last = Some(r);
    }
    for face in p.fonts.find(&style.font_families, style.font_style) {
        let r = make(Typeface::new(&face, &variations));
        if r.is_resolved() {
            return r;
        }
        last = Some(r);
    }
    if let Some(ch) = ellipsis.chars().next() {
        if let Some(face) = p.fonts.fallback(ch, style.font_style) {
            let r = make(Typeface::new(&face, &variations));
            if r.is_resolved() {
                return r;
            }
            last = Some(r);
        }
    }
    last.unwrap_or_else(|| {
        Run::new(
            None,
            style.font_size,
            0,
            [0.0, 0.0],
            0,
            (0, ellipsis.len()),
            0,
            0.0,
            false,
            0.0,
            0.0,
        )
    })
}

/// `measureTextInsideOneRun`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn measure_text_inside_one_run(
    p: &Paragraph,
    line: &TextLine,
    mut text_range: Range,
    run: &Run,
    run_offset_in_line: f32,
    text_offset_in_run_in_line: f32,
    include_ghost_spaces: bool,
    adjustment: Adjustment,
) -> ClipContext {
    let mut result = ClipContext {
        pos: 0,
        size: run.size(),
        text_shift: 0.0,
        clip: [0.0; 4],
        excluded_trailing_spaces: 0.0,
        clipping_needed: false,
    };
    let top = line.sizes.run_top(run, line.ascent_typographic);
    let h = run.calculate_height(line.ascent_typographic, line.descent_typographic);
    if run.ellipsis {
        result.text_shift = run_offset_in_line;
        result.clip = [
            run_offset_in_line,
            top,
            run_offset_in_line + run.advance()[0],
            top + h,
        ];
        return result;
    } else if run.is_placeholder() {
        result.text_shift = run_offset_in_line;
        result.clip = if run.font_metrics.ascent.is_finite() {
            [
                run_offset_in_line,
                top,
                run_offset_in_line + run.advance()[0],
                top + h,
            ]
        } else {
            let a = run.font_metrics.ascent;
            [
                run_offset_in_line,
                a,
                run_offset_in_line + run.advance()[0],
                a,
            ]
        };
        return result;
    } else if text_range.is_empty() {
        return result;
    }
    let original = text_range;
    loop {
        let updated = Range::new(
            p.previous_glyph_cluster(text_range.start),
            p.next_glyph_cluster(text_range.end),
        );
        if adjustment == Adjustment::GlyphCluster {
            text_range = updated;
            break;
        }
        let g = Range::new(
            p.previous_grapheme(updated.start),
            p.next_grapheme(updated.end),
        );
        if g == text_range {
            break;
        }
        text_range = g;
    }
    let mut start = p.cluster_index(text_range.start);
    let mut end = p.cluster_index(text_range.end - usize::from(text_range.width() != 0));
    if start == EMPTY_INDEX || end == EMPTY_INDEX {
        return result;
    }
    if !run.left_to_right() {
        std::mem::swap(&mut start, &mut end);
    }
    let (sc, ec) = (&p.clusters[start], &p.clusters[end]);
    result.pos = sc.start;
    result.size = (if ec.is_hard_break { ec.start } else { ec.end }).saturating_sub(sc.start);
    let text_start_in_run = run.position_x(sc.start);
    let mut text_start_in_line = run_offset_in_line + text_offset_in_run_in_line;
    if !run.left_to_right() {
        std::mem::swap(&mut start, &mut end);
    }
    let (sc, ec) = (&p.clusters[start], &p.clusters[end]);
    result.clip = [
        0.0,
        top,
        run.calculate_width(result.pos, result.pos + result.size),
        top + h,
    ];
    let left_correction = sc.size_to_char(original.start);
    let right_correction = ec.size_from_char(original.end - 1);
    result.clipping_needed = left_correction != 0.0 || right_correction != 0.0;
    if run.left_to_right() {
        result.clip[0] += left_correction;
        result.clip[2] -= right_correction;
        text_start_in_line -= left_correction;
    } else {
        result.clip[2] -= left_correction;
        result.clip[0] += right_correction;
        text_start_in_line -= right_correction;
    }
    result.clip[0] += text_start_in_line;
    result.clip[2] += text_start_in_line;
    if compare_round(result.clip[2], line.advance[0]) > 0
        && !include_ghost_spaces
        && p.style.text_direction == TextDirection::Ltr
    {
        result.excluded_trailing_spaces = (result.clip[2] - line.advance[0]).max(0.0);
        result.clipping_needed = true;
        result.clip[2] = line.advance[0];
    }
    if width(&result.clip) < 0.0 {
        result.clip[2] = result.clip[0];
    }
    result.text_shift = text_start_in_line - text_start_in_run;
    result
}

/// `iterateThroughVisualRuns`: each run (the paragraph's index, or
/// `EMPTY_INDEX` for the ellipsis), its offset in the line and its text in
/// the line; the visitor returns the run's width in the line and whether
/// to go on.
pub(crate) fn iterate_visual_runs(
    p: &Paragraph,
    line: &TextLine,
    including_ghosts: bool,
    mut visitor: impl FnMut(usize, &Run, f32, Range) -> (f32, bool),
) {
    let mut width = 0.0f32;
    let mut run_offset = 0.0f32;
    let text_range = if including_ghosts {
        line.text_with_newlines
    } else {
        line.text_excluding_spaces
    };
    if let Some(e) = &line.ellipsis {
        if p.style.text_direction == TextDirection::Rtl {
            run_offset = e.offset[0];
            width = visitor(EMPTY_INDEX, e, run_offset, e.text_range).0;
        }
    }
    for &ri in &line.runs_in_visual_order {
        let run = &p.runs[ri];
        let li = run.text_range.intersected(text_range);
        if li.width() == 0 && line.width() != 0.0 {
            continue;
        }
        if !run.left_to_right() && run_offset == 0.0 && including_ghosts {
            let ws = Range::new(line.text_excluding_spaces.end, line.text_with_newlines.end)
                .intersected(run.text_range);
            if ws.width() > 0 && ws.start != EMPTY_INDEX {
                let c = measure_text_inside_one_run(
                    p,
                    line,
                    ws,
                    run,
                    run_offset,
                    0.0,
                    true,
                    Adjustment::GlyphCluster,
                );
                run_offset -= width_of(&c.clip);
            }
        }
        run_offset += width;
        let (w, go_on) = visitor(ri, run, run_offset, li);
        width = w;
        if !go_on {
            return;
        }
    }
    run_offset += width;
    if let Some(e) = &line.ellipsis {
        if p.style.text_direction == TextDirection::Ltr {
            visitor(EMPTY_INDEX, e, run_offset, e.text_range);
        }
    }
}

fn width_of(r: &[f32; 4]) -> f32 {
    width(r)
}

/// `iterateThroughSingleRunByStyles` for `StyleType::kNone`: the run's
/// text in the line as one stretch.
fn single_run_whole(
    p: &Paragraph,
    line: &TextLine,
    adjustment: Adjustment,
    run: &Run,
    run_offset: f32,
    text_range: Range,
    mut visitor: impl FnMut(Range, &ClipContext),
) -> f32 {
    if run.ellipsis {
        let c = measure_text_inside_one_run(
            p,
            line,
            run.text_range,
            run,
            run_offset,
            0.0,
            false,
            adjustment,
        );
        let test = Range::new(
            run.cluster_start,
            run.cluster_start + run.text_range.width(),
        );
        let blocks =
            &p.blocks[line.blocks.start.min(p.blocks.len())..line.blocks.end.min(p.blocks.len())];
        if blocks.iter().any(|b| {
            let i = b.range.intersected(test);
            i.start != EMPTY_INDEX && i.width() > 0
        }) {
            visitor(test, &c);
        }
        return run.advance()[0];
    }
    let c =
        measure_text_inside_one_run(p, line, text_range, run, run_offset, 0.0, false, adjustment);
    if height(&c.clip) > 0.0 || width(&c.clip) > 0.0 {
        visitor(text_range, &c);
        width(&c.clip)
    } else {
        0.0
    }
}

/// `getMetrics`.
pub(crate) fn metrics(p: &Paragraph, li: usize) -> LineMetricsInfo {
    let line = &p.lines[li];
    let m = &line.max_run_metrics;
    LineMetricsInfo {
        start_index: p.utf16_index(line.text_excluding_spaces.start),
        end_excluding_whitespace: p.utf16_index(line.text_excluding_spaces.end),
        end_index: p.utf16_index(line.text.end),
        end_including_newline: p.utf16_index(line.text_with_newlines.end),
        hard_break: line.ends_with_hard_line_break(p),
        ascent: f64::from(-m.ascent),
        descent: f64::from(m.descent),
        unscaled_ascent: f64::from(-m.ascent),
        height: f64::from(line.advance[1]),
        width: f64::from(line.advance[0]),
        left: f64::from(line.offset()[0]),
        baseline: f64::from(line.offset()[1] + line.height() - line.sizes.descent),
        line_number: li,
    }
}

/// What `getRectsForRange` compares of two runs before merging their
/// boxes: direction, placeholder, height multiplier and font.
#[derive(Debug, Clone, PartialEq)]
struct RunKey {
    ltr: bool,
    placeholder: bool,
    height_multiplier: f32,
    face: Option<u64>,
    size: f32,
    variations: Vec<([u8; 4], u32)>,
}

/// `getRectsForRange` on one line.
pub(crate) fn rects_for_range(
    p: &Paragraph,
    li: usize,
    range0: Range,
    hs: RectHeightStyle,
    ws: RectWidthStyle,
    boxes: &mut Vec<TextBox>,
) {
    let line = &p.lines[li];
    let first_line = li == 0;
    let last_line = li + 1 == p.lines.len();
    let start_box = boxes.len();
    let mut last_run: Option<RunKey> = None;
    iterate_visual_runs(p, line, true, |_ri, run, run_offset, text_range| {
        let w = single_run_whole(
            p,
            line,
            Adjustment::GraphemeGluster,
            run,
            run_offset,
            text_range,
            |text_range, line_ctx| {
                let intersect = text_range.intersect(range0);
                if intersect.start == EMPTY_INDEX || intersect.is_empty() {
                    return;
                }
                let ctx = measure_text_inside_one_run(
                    p,
                    line,
                    intersect,
                    run,
                    run_offset,
                    0.0,
                    true,
                    Adjustment::GraphemeGluster,
                );
                let mut clip = ctx.clip;
                let dx = line_ctx.text_shift - ctx.text_shift;
                clip[0] += dx;
                clip[2] += dx;
                let delta = line.sizes.delta();
                match hs {
                    RectHeightStyle::Max => {
                        clip[3] = line.height();
                        clip[1] = delta;
                    }
                    RectHeightStyle::IncludeLineSpacingTop => {
                        clip[3] = line.height();
                        clip[1] = delta;
                        let vs = line.sizes.raw_ascent - line.sizes.ascent;
                        if first_line {
                            clip[1] += vs;
                        }
                    }
                    RectHeightStyle::IncludeLineSpacingMiddle => {
                        clip[3] = line.height();
                        clip[1] = delta;
                        let vs = line.sizes.raw_ascent - line.sizes.ascent;
                        clip[1] += vs / 2.0;
                        clip[3] += vs / 2.0;
                        if first_line {
                            clip[1] += vs / 2.0;
                        }
                        if last_line {
                            clip[3] -= vs / 2.0;
                        }
                    }
                    RectHeightStyle::IncludeLineSpacingBottom => {
                        clip[3] = line.height();
                        clip[1] = delta;
                        let vs = line.sizes.raw_ascent - line.sizes.ascent;
                        clip[1] += vs;
                        clip[3] += vs;
                        if last_line {
                            clip[3] -= vs;
                        }
                    }
                    RectHeightStyle::Strut => {
                        let s = &p.style.strut;
                        if s.enabled && s.font_size > 0.0 {
                            let top = line.baseline();
                            clip[1] = top + p.strut_metrics.ascent;
                            clip[3] = top + p.strut_metrics.descent;
                        }
                    }
                    RectHeightStyle::Tight => {
                        if run.height_multiplier > 0.0 {
                            let eb = line.baseline() + line.sizes.delta();
                            clip[1] = eb + run.ascent();
                            clip[3] = eb + run.descent();
                        }
                    }
                }
                let mut trailing = [0.0f32; 4];
                let has_trailing = line.text_excluding_spaces.end < line.text_with_newlines.end
                    && line.text_with_newlines.end == intersect.end
                    && line.text_excluding_spaces.end > intersect.start;
                if has_trailing {
                    let d = line.spaces_width();
                    if p.style.text_align == TextAlign::Justify && last_line {
                        trailing = clip;
                        if run.left_to_right() {
                            trailing[0] = line.width();
                            clip[2] = line.width();
                        } else {
                            trailing[2] = 0.0;
                            clip[0] = 0.0;
                        }
                    } else if p.style.text_direction == TextDirection::Rtl && !run.left_to_right() {
                        trailing = clip;
                        trailing[0] = -d;
                        trailing[2] = 0.0;
                        clip[0] += d;
                    } else if p.style.text_direction == TextDirection::Ltr && run.left_to_right() {
                        trailing = clip;
                        trailing[0] = line.width();
                        trailing[2] = trailing[0] + d;
                        clip[2] -= d;
                    }
                }
                let off = line.offset();
                clip = [
                    clip[0] + off[0],
                    clip[1] + off[1],
                    clip[2] + off[0],
                    clip[3] + off[1],
                ];
                if width(&trailing) > 0.0 {
                    trailing = [
                        trailing[0] + off[0],
                        trailing[1] + off[1],
                        trailing[2] + off[0],
                        trailing[3] + off[1],
                    ];
                }
                let key = RunKey {
                    ltr: run.left_to_right(),
                    placeholder: run.is_placeholder(),
                    height_multiplier: run.height_multiplier,
                    face: run.typeface.as_ref().map(|t| t.face.id),
                    size: run.font_size,
                    variations: run
                        .typeface
                        .as_ref()
                        .map(|t| {
                            t.variations
                                .iter()
                                .map(|(a, v)| (*a, v.to_bits()))
                                .collect()
                        })
                        .unwrap_or_default(),
                };
                let mut merge = |clip: [f32; 4], boxes: &mut Vec<TextBox>| -> bool {
                    let mut merged = false;
                    if let (Some(last), Some(lb)) = (&last_run, boxes.last_mut()) {
                        if last.ltr == key.ltr
                            && !last.placeholder
                            && !key.placeholder
                            && nearly_equal(last.height_multiplier, key.height_multiplier)
                            && last.face == key.face
                            && last.size == key.size
                            && last.variations == key.variations
                            && nearly_equal(lb.rect[1], clip[1])
                            && nearly_equal(lb.rect[3], clip[3])
                            && (nearly_equal(lb.rect[0], clip[2])
                                || nearly_equal(lb.rect[2], clip[0]))
                        {
                            lb.rect[0] = lb.rect[0].min(clip[0]);
                            lb.rect[2] = lb.rect[2].max(clip[2]);
                            merged = true;
                        }
                    }
                    last_run = Some(key.clone());
                    merged
                };
                let dir = |ltr: bool| {
                    if ltr {
                        TextDirection::Ltr
                    } else {
                        TextDirection::Rtl
                    }
                };
                if !merge(clip, boxes) {
                    boxes.push(TextBox {
                        rect: clip,
                        direction: dir(run.left_to_right()),
                    });
                }
                if !nearly_zero(width(&trailing)) && !merge(trailing, boxes) {
                    boxes.push(TextBox {
                        rect: trailing,
                        direction: p.style.text_direction,
                    });
                }
                if ws == RectWidthStyle::Max && !last_line {
                    let line_start = line.offset()[0];
                    let line_end = line.offset()[0] + line.width();
                    let mut left = boxes[start_box];
                    let mut right = boxes[boxes.len() - 1];
                    if left.rect[0] > line_start && left.direction == TextDirection::Rtl {
                        left.rect[2] = left.rect[0];
                        left.rect[0] = 0.0;
                        boxes.insert(start_box + 1, left);
                    }
                    if right.direction == TextDirection::Ltr
                        && right.rect[2] >= line_end
                        && right.rect[2] < p.max_width_with_trailing_spaces
                    {
                        right.rect[0] = right.rect[2];
                        right.rect[2] = p.max_width_with_trailing_spaces;
                        boxes.push(right);
                    }
                }
            },
        );
        (w, true)
    });
}

/// `getGlyphPositionAtCoordinate` on one line.
pub(crate) fn glyph_position_at(p: &Paragraph, li: usize, dx: f32) -> PositionWithAffinity {
    let line = &p.lines[li];
    if nearly_zero(line.width()) && nearly_zero(line.spaces_width()) {
        return PositionWithAffinity {
            position: p.utf16_index(line.text_excluding_spaces.end),
            upstream: false,
        };
    }
    let mut result = PositionWithAffinity {
        position: 0,
        upstream: false,
    };
    iterate_visual_runs(p, line, true, |_ri, run, run_offset, text_range| {
        if run.ellipsis {
            result = PositionWithAffinity {
                position: p.utf16_index(line.text.end),
                upstream: false,
            };
            return (0.0, false);
        }
        let mut keep_looking = true;
        let w = single_run_whole(
            p,
            line,
            Adjustment::GraphemeGluster,
            run,
            run_offset,
            text_range,
            |_tr, ctx0| {
                let offset_x = line.offset()[0];
                let mut ctx = *ctx0;
                if run.left_to_right() {
                    ctx.clip[2] += ctx.excluded_trailing_spaces;
                }
                ctx.clip[0] += offset_x;
                ctx.clip[2] += offset_x;
                if (ctx.clip[2] - dx).abs() <= 0.01 {
                    ctx.clip[2] = dx;
                }
                if dx <= ctx.clip[0] {
                    let u = p.utf16_index(run.global_cluster_index(ctx.pos));
                    if run.left_to_right() {
                        result = PositionWithAffinity {
                            position: u,
                            upstream: false,
                        };
                        keep_looking = false;
                    } else {
                        result = PositionWithAffinity {
                            position: u + 1,
                            upstream: true,
                        };
                        keep_looking = ctx.pos != 0;
                    }
                    return;
                }
                if dx >= ctx.clip[2] {
                    let u = p.utf16_index(run.global_cluster_index(ctx.pos + ctx.size));
                    result = PositionWithAffinity {
                        position: u,
                        upstream: run.left_to_right(),
                    };
                    return;
                }
                let mut found = ctx.pos;
                for index in ctx.pos..ctx.pos + ctx.size {
                    let end = run.position_x(index) + ctx.text_shift + offset_x;
                    if end > dx {
                        break;
                    } else if end == dx && !run.left_to_right() {
                        found = index;
                        break;
                    }
                    found = index;
                }
                let left = run.position_x(found) + ctx.text_shift + offset_x;
                let gw = run.position_x(found + 1) - run.position_x(found);
                let c8 = run.global_cluster_index(found);
                let e8 = run.global_cluster_index(found + 1);
                let graphemes = surrounding_graphemes(p, Range::new(c8, e8));
                let center = left + gw / 2.0;
                if graphemes.len() > 1 {
                    let avg = gw / graphemes.len() as f32;
                    let delta = dx - left;
                    let gi = if nearly_zero(avg) {
                        0
                    } else {
                        ((delta / avg).floor().max(0.0) as usize).min(graphemes.len() - 1)
                    };
                    let gc = left + gi as f32 * avg + avg / 2.0;
                    let g8 = graphemes[gi];
                    result = if (dx < gc) == run.left_to_right() {
                        PositionWithAffinity {
                            position: p.utf16_index(g8),
                            upstream: false,
                        }
                    } else {
                        PositionWithAffinity {
                            position: p.utf16_index(g8 + 1),
                            upstream: true,
                        }
                    };
                } else if (dx < center) == run.left_to_right() {
                    result = PositionWithAffinity {
                        position: p.utf16_index(c8),
                        upstream: false,
                    };
                } else {
                    result = PositionWithAffinity {
                        position: if run.left_to_right() {
                            p.utf16_index(e8)
                        } else {
                            p.utf16_index(c8) + 1
                        },
                        upstream: true,
                    };
                }
                keep_looking = false;
            },
        );
        (w, keep_looking)
    });
    result
}

/// `countSurroundingGraphemes`.
fn surrounding_graphemes(p: &Paragraph, r: Range) -> Vec<usize> {
    let r = Range::new(r.start.min(p.text.len()), r.end.min(p.text.len()));
    let mut g = Vec::new();
    if !p.has_flag(r.start, unicode::flag::GRAPHEME_START) {
        g.push(r.start);
    }
    for i in r.start..r.end {
        if p.has_flag(i, unicode::flag::GRAPHEME_START) {
            g.push(i);
        }
    }
    g
}

/// `getRectsForPlaceholders` on one line.
pub(crate) fn rects_for_placeholders(p: &Paragraph, li: usize, boxes: &mut Vec<TextBox>) {
    let line = &p.lines[li];
    iterate_visual_runs(p, line, true, |_ri, run, run_offset, text_range| {
        let ctx = measure_text_inside_one_run(
            p,
            line,
            text_range,
            run,
            run_offset,
            0.0,
            true,
            Adjustment::GraphemeGluster,
        );
        let w = width(&ctx.clip);
        if text_range.width() == 0 || text_range.start == EMPTY_INDEX || !run.is_placeholder() {
            return (w, true);
        }
        let off = line.offset();
        boxes.push(TextBox {
            rect: [
                ctx.clip[0] + off[0],
                ctx.clip[1] + off[1],
                ctx.clip[2] + off[0],
                ctx.clip[3] + off[1],
            ],
            direction: if run.left_to_right() {
                TextDirection::Ltr
            } else {
                TextDirection::Rtl
            },
        });
        (w, true)
    });
}
