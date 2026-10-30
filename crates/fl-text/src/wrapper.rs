//! Breaking the endless line into lines: SkParagraph's `TextWrapper`,
//! clusters addressed by index where it holds pointers.

use crate::paragraph::{Cluster, LineMetrics, Paragraph, Range, EMPTY_INDEX};
use crate::style::height_behavior;

/// A position inside a cluster: the cluster and a glyph offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClusterPos {
    cluster: usize,
    pos: usize,
}

impl ClusterPos {
    const NONE: ClusterPos = ClusterPos {
        cluster: EMPTY_INDEX,
        pos: 0,
    };
}

/// `TextStretch`.
#[derive(Debug, Clone, Copy)]
struct Stretch {
    start: ClusterPos,
    end: ClusterPos,
    brk: ClusterPos,
    metrics: LineMetrics,
    width: f32,
    width_with_ghosts: f32,
}

impl Stretch {
    fn new(force_strut: bool) -> Stretch {
        Stretch {
            start: ClusterPos::NONE,
            end: ClusterPos::NONE,
            brk: ClusterPos::NONE,
            metrics: LineMetrics::clean_with(force_strut),
            width: 0.0,
            width_with_ghosts: 0.0,
        }
    }

    /// `TextStretch(s, e, forceStrut)`.
    fn between(p: &Paragraph, s: usize, e: usize, force_strut: bool) -> Stretch {
        let mut t = Stretch::new(force_strut);
        t.start = ClusterPos { cluster: s, pos: 0 };
        t.end = ClusterPos {
            cluster: e,
            pos: p.clusters[e].end,
        };
        for c in s..=e {
            if let Some(r) = p.runs.get(p.clusters[c].run_index) {
                t.metrics.add_run(r);
            }
            if c < e {
                t.width += p.clusters[c].width;
            }
        }
        t.width_with_ghosts = t.width;
        t
    }

    fn end_of_cluster(&self, p: &Paragraph) -> bool {
        p.clusters
            .get(self.end.cluster)
            .is_some_and(|c| self.end.pos == c.end)
    }

    fn end_of_word(&self, p: &Paragraph) -> bool {
        self.end_of_cluster(p) && {
            let c = &p.clusters[self.end.cluster];
            c.is_hard_break || p.is_soft_break(c)
        }
    }

    fn extend(&mut self, other: &mut Stretch) {
        self.metrics.add(&other.metrics);
        self.end = other.end;
        self.width += other.width;
        other.clean();
    }

    fn is_empty(&self) -> bool {
        self.start.cluster == self.end.cluster && self.start.pos == self.end.pos
    }

    fn extend_cluster(&mut self, p: &Paragraph, ci: usize) {
        let c = &p.clusters[ci];
        if self.start.cluster == EMPTY_INDEX {
            self.start = ClusterPos {
                cluster: ci,
                pos: c.start,
            };
        }
        self.end = ClusterPos {
            cluster: ci,
            pos: c.end,
        };
        if let Some(r) = p.runs.get(c.run_index) {
            // A newline's metrics are left out, as Flutter does.
            if !c.is_hard_break && !r.is_placeholder() {
                self.metrics.add_run(r);
            }
        }
        self.width += c.width;
    }

    fn start_from(&mut self, p: &Paragraph, ci: usize, pos: usize) {
        self.start = ClusterPos { cluster: ci, pos };
        self.end = self.start;
        if let Some(r) = p.clusters.get(ci).and_then(|c| p.runs.get(c.run_index)) {
            if !r.is_placeholder() {
                self.metrics.add_run(r);
            }
        }
        self.width = 0.0;
    }

    fn save_break(&mut self) {
        self.width_with_ghosts = self.width;
        self.brk = self.end;
    }

    fn restore_break(&mut self) {
        self.width = self.width_with_ghosts;
        self.end = self.brk;
    }

    fn shift_break(&mut self) {
        self.brk.cluster += 1;
        self.brk.pos = 0;
    }

    /// `trim()`: the end cluster's trailing glyph space.
    fn trim(&mut self, p: &Paragraph) {
        let Some(c) = p.clusters.get(self.end.cluster) else {
            return;
        };
        let Some(r) = p.runs.get(c.run_index) else {
            return;
        };
        if !r.is_placeholder() && self.width > 0.0 {
            self.width -= c.width - p.trimmed_width(c, self.end.pos);
        }
    }

    /// `trim(cluster)`: the end cluster dropped.
    fn trim_cluster(&mut self, p: &Paragraph, ci: usize) {
        if self.end.cluster > self.start.cluster {
            self.end.cluster -= 1;
            self.end.pos = p.clusters[self.end.cluster].end;
            self.width -= p.clusters[ci].width;
        } else {
            self.end.pos = self.start.pos;
            self.width = 0.0;
        }
    }

    fn clean(&mut self) {
        self.start = ClusterPos::NONE;
        self.end = ClusterPos::NONE;
        self.width = 0.0;
        self.metrics.clean();
    }
}

/// `LineBreakerWithLittleRounding`, the rounding hack off as Flutter sets
/// it: widths within a quarter pixel compared floored to a hundredth.
struct Breaker {
    lower: f32,
    max: f32,
    upper: f32,
}

impl Breaker {
    fn new(max: f32) -> Breaker {
        Breaker {
            lower: max - 0.25,
            max,
            upper: max + 0.25,
        }
    }

    fn break_line(&self, width: f32) -> bool {
        if width < self.lower {
            return false;
        }
        if width > self.upper {
            return true;
        }
        let v = width.abs();
        let rounded = if v < 10000.0 {
            (width * 100.0).floor() * (1.0 / 100.0)
        } else if v < 100000.0 {
            (width * 10.0).floor() * (1.0 / 10.0)
        } else {
            width.floor()
        };
        rounded > self.max
    }
}

struct Wrapper {
    words: Stretch,
    clusters: Stretch,
    clip: Stretch,
    end_line: Stretch,
    line_number: usize,
    too_long_word: bool,
    too_long_cluster: bool,
    hard_line_break: bool,
    exceeded_max_lines: bool,
    height: f32,
    min_intrinsic: f32,
    max_intrinsic: f32,
}

/// One line as the wrapper hands it to the paragraph (`addLine`).
pub(crate) struct LineSpec {
    pub text_excluding_spaces: Range,
    pub text: Range,
    pub text_with_newlines: Range,
    pub clusters: Range,
    pub clusters_with_ghosts: Range,
    pub width_with_spaces: f32,
    pub offset: [f32; 2],
    pub advance: [f32; 2],
    pub metrics: LineMetrics,
    pub ellipsis: bool,
}

impl Wrapper {
    fn reset(&mut self) {
        self.words.clean();
        self.clusters.clean();
        self.clip.clean();
        self.too_long_cluster = false;
        self.too_long_word = false;
        self.hard_line_break = false;
    }

    fn clusters_trimmed_width(&self, p: &Paragraph) -> f32 {
        let mut width = 0.0;
        let mut trailing = true;
        let (s, e) = (self.clusters.start.cluster, self.clusters.end.cluster);
        if s == EMPTY_INDEX || e == EMPTY_INDEX {
            return 0.0;
        }
        for ci in (s..=e).rev() {
            let c = &p.clusters[ci];
            if p.runs.get(c.run_index).is_none_or(|r| r.is_placeholder()) {
                continue;
            }
            if trailing {
                if !c.is_whitespace_break {
                    width += p.trimmed_width(c, c.end);
                    trailing = false;
                }
                continue;
            }
            width += c.width;
        }
        width
    }

    fn is_placeholder(p: &Paragraph, c: &Cluster) -> bool {
        p.runs.get(c.run_index).is_some_and(|r| r.is_placeholder())
    }

    fn look_ahead(&mut self, p: &Paragraph, max_width: f32, end: usize) {
        self.reset();
        self.end_line.metrics.clean();
        let (sc, sp) = (self.end_line.start.cluster, self.end_line.start.pos);
        self.words.start_from(p, sc, sp);
        self.clusters.start_from(p, sc, sp);
        self.clip.start_from(p, sc, sp);
        let breaker = Breaker::new(max_width);
        let mut next_non_breaking_space = EMPTY_INDEX;
        let mut ci = self.end_line.end.cluster;
        while ci < end {
            let cluster = &p.clusters[ci];
            if !cluster.is_hard_break {
                let width = self.words.width + self.clusters.width + cluster.width;
                if breaker.break_line(width) {
                    if cluster.is_whitespace_break {
                        // Spaces hang past the edge.
                        self.clusters.extend_cluster(p, ci);
                        self.min_intrinsic = self.min_intrinsic.max(self.clusters_trimmed_width(p));
                        let mut c = self.clusters;
                        self.words.extend(&mut c);
                        self.clusters = c;
                        ci += 1;
                        continue;
                    } else if Self::is_placeholder(p, cluster) {
                        if !self.clusters.is_empty() {
                            self.min_intrinsic =
                                self.min_intrinsic.max(self.clusters_trimmed_width(p));
                            let mut c = self.clusters;
                            self.words.extend(&mut c);
                            self.clusters = c;
                        }
                        if cluster.width > max_width && self.words.is_empty() {
                            self.clusters.extend_cluster(p, ci);
                            self.too_long_cluster = true;
                            self.too_long_word = true;
                        }
                        break;
                    }
                    let mut next_word_length = self.clusters.width;
                    let mut next_short_word_length = next_word_length;
                    for fi in ci..end {
                        let further = &p.clusters[fi];
                        if p.is_soft_break(further)
                            || further.is_hard_break
                            || further.is_whitespace_break
                        {
                            break;
                        }
                        if Self::is_placeholder(p, further) {
                            break;
                        }
                        if next_word_length > 0.0
                            && next_word_length <= max_width
                            && further.is_intra_word_break
                        {
                            next_non_breaking_space = fi;
                            next_short_word_length = next_word_length;
                        }
                        if max_width == 0.0 {
                            next_word_length = next_word_length.max(further.width);
                        } else {
                            next_word_length += further.width;
                        }
                    }
                    if next_word_length > max_width {
                        if next_non_breaking_space != EMPTY_INDEX {
                            let short_length = self.words.width + next_short_word_length;
                            if !breaker.break_line(short_length) {
                                let force = self.clusters.metrics.force_strut;
                                self.clusters = Stretch::between(
                                    p,
                                    self.clusters.start.cluster,
                                    next_non_breaking_space,
                                    force,
                                );
                                self.min_intrinsic = self.min_intrinsic.max(next_short_word_length);
                                let mut c = self.clusters;
                                self.words.extend(&mut c);
                                self.clusters = c;
                            } else {
                                self.clusters.clean();
                            }
                            break;
                        }
                        self.min_intrinsic = self.min_intrinsic.max(next_word_length);
                        if self.clusters.end.pos.wrapping_sub(self.clusters.start.pos) > 1
                            || self.words.is_empty()
                        {
                            self.too_long_word = true;
                        }
                    }
                    if cluster.width > max_width {
                        self.clusters.extend_cluster(p, ci);
                        self.too_long_cluster = true;
                        self.too_long_word = true;
                    }
                    break;
                }
            }
            if Self::is_placeholder(p, cluster) {
                if !self.clusters.is_empty() {
                    self.min_intrinsic = self.min_intrinsic.max(self.clusters_trimmed_width(p));
                    let mut c = self.clusters;
                    self.words.extend(&mut c);
                    self.clusters = c;
                }
                self.min_intrinsic = self.min_intrinsic.max(cluster.width);
                self.words.extend_cluster(p, ci);
            } else {
                self.clusters.extend_cluster(p, ci);
                if self.clusters.end_of_word(p) {
                    self.min_intrinsic = self.min_intrinsic.max(self.clusters_trimmed_width(p));
                    let mut c = self.clusters;
                    self.words.extend(&mut c);
                    self.clusters = c;
                }
            }
            self.hard_line_break = cluster.is_hard_break;
            if self.hard_line_break {
                break;
            }
            ci += 1;
        }
    }

    fn move_forward(&mut self, has_ellipsis: bool) {
        if !self.words.is_empty() {
            let mut w = self.words;
            self.end_line.extend(&mut w);
            self.words = w;
            if !self.too_long_word && !has_ellipsis {
                return;
            }
        }
        if !self.clusters.is_empty() {
            let mut c = self.clusters;
            self.end_line.extend(&mut c);
            self.clusters = c;
            if !self.too_long_cluster {
                return;
            }
        }
        if !self.clip.is_empty() {
            let m = self.clip.metrics;
            self.end_line.metrics.add(&m);
        }
    }

    fn trim_end_spaces(&mut self, p: &Paragraph) {
        self.end_line.save_break();
        loop {
            let ci = self.end_line.end.cluster;
            if ci == EMPTY_INDEX
                || ci < self.end_line.start.cluster
                || !p.clusters[ci].is_whitespace_break
            {
                break;
            }
            let at_start = ci == self.end_line.start.cluster;
            self.end_line.trim_cluster(p, ci);
            if at_start {
                // `--cluster` passes the start: the loop ends.
                break;
            }
        }
        self.end_line.trim(p);
    }

    /// The next line's start cluster and position, and this line's width
    /// with its trailing spaces.
    fn trim_start_spaces(&mut self, p: &Paragraph, end: usize) -> (usize, usize, f32) {
        if self.hard_line_break {
            let mut width = self.end_line.width;
            let mut ci = self.end_line.end.cluster + 1;
            while ci < self.end_line.brk.cluster && p.clusters[ci].is_whitespace_break {
                width += p.clusters[ci].width;
                ci += 1;
            }
            return (self.end_line.brk.cluster + 1, 0, width);
        }
        let mut width = self.end_line.width_with_ghosts;
        let mut ci = self.end_line.brk.cluster + 1;
        while ci < end && p.clusters[ci].is_whitespace_break {
            width += p.clusters[ci].width;
            ci += 1;
        }
        if p.clusters[self.end_line.brk.cluster].is_whitespace_break
            && self.end_line.brk.cluster < end
        {
            self.end_line.shift_break();
        }
        (ci, 0, width)
    }
}

/// `breakTextIntoLines`, and the paragraph's `addLine` for each line.
pub(crate) fn break_text_into_lines(p: &mut Paragraph, max_width: f32) {
    let mut w = Wrapper {
        words: Stretch::new(false),
        clusters: Stretch::new(false),
        clip: Stretch::new(false),
        end_line: Stretch::new(false),
        line_number: 1,
        too_long_word: false,
        too_long_cluster: false,
        hard_line_break: false,
        exceeded_max_lines: false,
        height: 0.0,
        min_intrinsic: f32::MIN_POSITIVE,
        max_intrinsic: f32::MIN_POSITIVE,
    };
    if p.clusters.is_empty() {
        return;
    }
    let max_lines = p.style.max_lines;
    let unlimited = p.style.unlimited_lines();
    let endless = !max_width.is_finite();
    let has_ellipsis = p.style.ellipsized();
    let hb = p.style.text_height_behavior;
    let disable_first_ascent = hb & height_behavior::DISABLE_FIRST_ASCENT != 0;
    let disable_last_descent = hb & height_behavior::DISABLE_LAST_DESCENT != 0;
    let mut first_line = true;
    let mut soft_line_max_intrinsic = 0.0f32;
    let force = p.style.strut_force_height();
    w.end_line = Stretch::between(p, 0, 0, force);
    let end = p.clusters.len() - 1;
    let mut max_run_metrics = LineMetrics::clean_with(false);
    let mut need_ellipsis = false;
    let mut specs: Vec<LineSpec> = Vec::new();
    while w.end_line.end.cluster != end {
        w.look_ahead(p, max_width, end);
        let last_line = (has_ellipsis && unlimited) || w.line_number >= max_lines;
        need_ellipsis = has_ellipsis && !endless && last_line;
        w.move_forward(need_ellipsis);
        need_ellipsis &= w.end_line.end.cluster.wrapping_add(1) < end;
        w.trim_end_spaces(p);
        let (start_line, pos, mut width_with_spaces) = w.trim_start_spaces(p, end);
        if need_ellipsis && !w.hard_line_break {
            w.end_line.restore_break();
            width_with_spaces = w.end_line.width_with_ghosts;
        }
        if w.end_line.metrics.is_clean() {
            w.end_line.metrics = p.empty_metrics;
        }
        let mut last_run = EMPTY_INDEX;
        for ci in w.end_line.start.cluster..=w.end_line.end.cluster {
            let ri = p.clusters[ci].run_index;
            if ri == last_run {
                continue;
            }
            last_run = ri;
            if let Some(pi) = p.runs.get(ri).and_then(|r| r.placeholder) {
                let style = p.placeholders[pi].style;
                p.runs[ri].update_placeholder_metrics(&style, &mut w.end_line.metrics);
            }
        }
        max_run_metrics = w.end_line.metrics;
        max_run_metrics.force_strut = false;
        let sc = &p.clusters[w.end_line.start.cluster];
        let ec = &p.clusters[w.end_line.end.cluster];
        let bc = &p.clusters[w.end_line.brk.cluster];
        let mut text_excluding = Range::new(sc.text_range.start, ec.text_range.end);
        let mut text = Range::new(sc.text_range.start, bc.text_range.start);
        let mut text_with_newlines =
            Range::new(sc.text_range.start, p.clusters[start_line].text_range.start);
        if start_line == end {
            text_with_newlines.end = p.text.len();
            text.end = p.text.len();
        }
        let mut clusters = Range::new(w.end_line.start.cluster, w.end_line.end.cluster + 1);
        let ghosts = Range::new(w.end_line.start.cluster, start_line);
        if disable_first_ascent && first_line {
            w.end_line.metrics.ascent = w.end_line.metrics.raw_ascent;
        }
        if disable_last_descent && (last_line || (start_line == end && !w.hard_line_break)) {
            w.end_line.metrics.descent = w.end_line.metrics.raw_descent;
        }
        if p.strut_enabled() {
            p.strut_metrics.update_line(&mut w.end_line.metrics);
        }
        let line_height = w.end_line.metrics.height();
        first_line = false;
        if w.end_line.is_empty() {
            text_excluding.end = text_excluding.start;
            clusters.end = clusters.start;
        }
        text.end = text.end.max(text_excluding.end);
        specs.push(LineSpec {
            text_excluding_spaces: text_excluding,
            text,
            text_with_newlines,
            clusters,
            clusters_with_ghosts: ghosts,
            width_with_spaces,
            offset: [0.0, w.height],
            advance: [w.end_line.width, line_height],
            metrics: w.end_line.metrics,
            ellipsis: need_ellipsis && !w.hard_line_break,
        });
        soft_line_max_intrinsic += width_with_spaces;
        w.max_intrinsic = w.max_intrinsic.max(soft_line_max_intrinsic);
        if w.hard_line_break {
            soft_line_max_intrinsic = 0.0;
        }
        w.height += line_height;
        if !w.hard_line_break || start_line != end {
            w.end_line.clean();
        }
        w.end_line.start_from(p, start_line, pos);
        p.max_width_with_trailing_spaces = p.max_width_with_trailing_spaces.max(width_with_spaces);
        if has_ellipsis && unlimited {
            if !w.hard_line_break {
                break;
            }
        } else if last_line {
            w.hard_line_break = false;
            break;
        }
        w.line_number += 1;
    }
    if w.end_line.end.cluster != EMPTY_INDEX {
        let mut last_word_length = 0.0f32;
        let mut ci = w.end_line.end.cluster;
        while ci != end || p.clusters[ci].end < p.clusters[end].end {
            w.exceeded_max_lines = true;
            let c = &p.clusters[ci];
            if c.is_hard_break {
                w.max_intrinsic = w.max_intrinsic.max(soft_line_max_intrinsic);
                soft_line_max_intrinsic = 0.0;
                w.min_intrinsic = w.min_intrinsic.max(last_word_length);
                last_word_length = 0.0;
            } else if c.is_whitespace_break {
                soft_line_max_intrinsic += c.width;
                w.min_intrinsic = w.min_intrinsic.max(last_word_length);
                last_word_length = 0.0;
            } else if Wrapper::is_placeholder(p, c) {
                w.min_intrinsic = w.min_intrinsic.max(last_word_length);
                soft_line_max_intrinsic += c.width;
                w.min_intrinsic = w.min_intrinsic.max(c.width);
                last_word_length = 0.0;
            } else {
                soft_line_max_intrinsic += c.width;
                last_word_length += c.width;
            }
            ci += 1;
            if ci > end {
                break;
            }
        }
        w.min_intrinsic = w.min_intrinsic.max(last_word_length);
        w.max_intrinsic = w.max_intrinsic.max(soft_line_max_intrinsic);
        if specs.is_empty() {
            if disable_first_ascent {
                w.end_line.metrics.ascent = w.end_line.metrics.raw_ascent;
            }
            if disable_last_descent && !w.hard_line_break {
                w.end_line.metrics.descent = w.end_line.metrics.raw_descent;
            }
            w.height = w.height.max(w.end_line.metrics.height());
        }
    }
    let mut trailing_hard_break = false;
    if w.hard_line_break {
        if disable_last_descent {
            w.end_line.metrics.descent = w.end_line.metrics.raw_descent;
        }
        if p.strut_enabled() {
            p.strut_metrics.update_line(&mut w.end_line.metrics);
        }
        let bc = w.end_line.brk.cluster;
        let ec = w.end_line.end.cluster;
        let clusters = Range::new(bc, ec);
        let h = w.end_line.metrics.height();
        specs.push(LineSpec {
            text_excluding_spaces: p.clusters[bc].text_range,
            text: p.clusters[bc].text_range,
            text_with_newlines: p.clusters[ec].text_range,
            clusters,
            clusters_with_ghosts: clusters,
            width_with_spaces: 0.0,
            offset: [0.0, w.height],
            advance: [0.0, h],
            metrics: w.end_line.metrics,
            ellipsis: need_ellipsis,
        });
        w.height += h;
        trailing_hard_break = true;
    }
    for spec in specs {
        let ellipsis = spec.ellipsis;
        let width_with_spaces = spec.width_with_spaces;
        let line = crate::line::TextLine::from_spec(p, spec);
        p.lines.push(line);
        let li = p.lines.len() - 1;
        if ellipsis {
            crate::line::create_ellipsis(p, li, max_width);
        }
        let lw = p.lines[li].width();
        p.longest_line = p.longest_line.max(if lw.abs() <= f32::EPSILON {
            width_with_spaces
        } else {
            lw
        });
    }
    if trailing_hard_break {
        if let Some(l) = p.lines.last_mut() {
            l.max_run_metrics = max_run_metrics;
        }
    }
    p.height = w.height;
    p.max_intrinsic_width = w.max_intrinsic;
    p.min_intrinsic_width = w.min_intrinsic;
    p.exceeded_max_lines = w.exceeded_max_lines;
    if disable_first_ascent {
        if let Some(l) = p.lines.first_mut() {
            l.ascent_typographic = true;
        }
    }
    if disable_last_descent {
        if let Some(l) = p.lines.last_mut() {
            l.descent_typographic = true;
        }
    }
}
