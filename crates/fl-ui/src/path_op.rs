//! `Path.combine` (`Path::op`): a boolean operation of two paths' filled
//! areas. The engine runs Skia's PathOps, exact on curves. Here both paths
//! are flattened to polygons, finely enough that the difference is under a
//! fiftieth of a logical pixel, and `i_overlay` combines those: the
//! result's edges are lines. Scaled far up afterwards, its curves would
//! show as the polygons they are.
//!
//! The result is even-odd, as Skia's `Op` gives it, its contours closed and
//! not overlapping. Each operand is filled by its own fill type first.

use std::ffi::c_void;

use i_overlay::core::fill_rule::FillRule as Rule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::simplify::SimplifyShape;
use i_overlay::float::single::SingleFloatOverlay;

use fl_dart_ffi::guard;

use crate::path::{self, FillType, Path, Point, Seg};

/// How far a flattened curve may stray from the curve, in the path's own
/// units: a fiftieth of a logical pixel.
const TOLERANCE: f64 = 0.02;

/// The most lines one curve becomes.
const MAX_LINES_PER_CURVE: usize = 1024;

/// The most points both operands flatten to: past it the op fails, as
/// Skia's fails on what it cannot do, and dart:ui throws.
const MAX_POINTS: usize = 1 << 20;

/// `PathOperation`'s index: Skia's `SkPathOp`, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Difference,
    Intersect,
    Union,
    Xor,
    ReverseDifference,
}

impl Op {
    fn from_index(i: i32) -> Option<Op> {
        Some(match i {
            0 => Op::Difference,
            1 => Op::Intersect,
            2 => Op::Union,
            3 => Op::Xor,
            4 => Op::ReverseDifference,
            _ => return None,
        })
    }

    fn rule(self) -> OverlayRule {
        match self {
            Op::Difference => OverlayRule::Difference,
            Op::Intersect => OverlayRule::Intersect,
            Op::Union => OverlayRule::Union,
            Op::Xor => OverlayRule::Xor,
            Op::ReverseDifference => OverlayRule::InverseDifference,
        }
    }
}

type Contour = Vec<[f64; 2]>;

fn rule(fill: FillType) -> Rule {
    match fill {
        FillType::NonZero => Rule::NonZero,
        FillType::EvenOdd => Rule::EvenOdd,
    }
}

fn f64s([x, y]: Point) -> [f64; 2] {
    [f64::from(x), f64::from(y)]
}

/// How many lines a curve whose control polygon bends by `bend` (the
/// largest second difference of its points) needs to stay within
/// [`TOLERANCE`]: a quadratic's is `bend / 8` at most from its chord, and
/// a cubic's `3 * bend / 4` (Wang's formula).
fn lines_for(bend: f64, factor: f64) -> usize {
    let n = (factor * bend / TOLERANCE).sqrt().ceil();
    if n.is_finite() {
        (n as usize).clamp(1, MAX_LINES_PER_CURVE)
    } else {
        MAX_LINES_PER_CURVE
    }
}

fn second_difference(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (a[0] - 2.0 * b[0] + c[0]).hypot(a[1] - 2.0 * b[1] + c[1])
}

/// The path's contours as polygons, each closed implicitly, as a fill
/// closes them. `None` for a point that is not finite, or past
/// [`MAX_POINTS`] with `count` the points so far.
fn flatten(p: &Path, count: &mut usize) -> Option<Vec<Contour>> {
    let mut contours = Vec::new();
    let mut cur: Contour = Vec::new();
    let mut start = [0.0, 0.0];
    let finish = |c: &mut Contour, out: &mut Vec<Contour>| {
        // Fewer than three points enclose nothing.
        if c.len() >= 3 {
            out.push(std::mem::take(c));
        } else {
            c.clear();
        }
    };
    for s in &p.segs {
        let from = cur.last().copied().unwrap_or(start);
        match *s {
            Seg::Move(q) => {
                finish(&mut cur, &mut contours);
                start = f64s(q);
                cur.push(start);
            }
            Seg::Line(q) => cur.push(f64s(q)),
            Seg::Quad(c, q) => {
                let (c, q) = (f64s(c), f64s(q));
                let n = lines_for(second_difference(from, c, q), 0.125);
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    let u = 1.0 - t;
                    cur.push([
                        u * u * from[0] + 2.0 * u * t * c[0] + t * t * q[0],
                        u * u * from[1] + 2.0 * u * t * c[1] + t * t * q[1],
                    ]);
                }
            }
            Seg::Conic(c, q, w) => {
                let (c, q, w) = (f64s(c), f64s(q), f64::from(w));
                // A conic bends no more than the quadratic of its points
                // does, weighted by how far its weight pulls toward the
                // control point.
                let n = lines_for(second_difference(from, c, q) * w.max(1.0), 0.125);
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    let u = 1.0 - t;
                    let (a, b, d) = (u * u, 2.0 * w * u * t, t * t);
                    let den = a + b + d;
                    cur.push([
                        (a * from[0] + b * c[0] + d * q[0]) / den,
                        (a * from[1] + b * c[1] + d * q[1]) / den,
                    ]);
                }
            }
            Seg::Cubic(c0, c1, q) => {
                let (c0, c1, q) = (f64s(c0), f64s(c1), f64s(q));
                let bend = second_difference(from, c0, c1).max(second_difference(c0, c1, q));
                let n = lines_for(bend, 0.75);
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    cur.push([
                        a * from[0] + b * c0[0] + c * c1[0] + d * q[0],
                        a * from[1] + b * c0[1] + c * c1[1] + d * q[1],
                    ]);
                }
            }
            Seg::Close => finish(&mut cur, &mut contours),
        }
        *count = count.checked_add(cur.len())?;
        if *count > MAX_POINTS
            || cur
                .last()
                .is_some_and(|p| !(p[0].is_finite() && p[1].is_finite()))
        {
            return None;
        }
    }
    finish(&mut cur, &mut contours);
    Some(contours)
}

/// `a op b`, or `None` where the op cannot be done: a point not finite, or
/// operands too large.
pub(crate) fn combine(a: &Path, b: &Path, op: Op) -> Option<Path> {
    let mut count = 0;
    let (ca, cb) = (flatten(a, &mut count)?, flatten(b, &mut count)?);
    let shapes = if a.fill == b.fill {
        ca.overlay(&cb, op.rule(), rule(a.fill))
    } else {
        // Each filled by its own rule first: what is left of each does not
        // overlap itself, so either rule reads it the same.
        let (sa, sb) = (
            ca.simplify_shape(rule(a.fill)),
            cb.simplify_shape(rule(b.fill)),
        );
        sa.overlay(&sb, op.rule(), Rule::NonZero)
    };
    let mut out = Path {
        fill: FillType::EvenOdd,
        segs: Vec::new(),
    };
    for contour in shapes.iter().flatten() {
        let mut points = contour.iter().map(|&[x, y]| [x as f32, y as f32]);
        let Some(first) = points.next() else {
            continue;
        };
        out.move_to(first);
        for p in points {
            out.line_to(p);
        }
        out.close();
    }
    Some(out)
}

/// `Bool Function(Pointer<Void>, Pointer<Void> path1, Pointer<Void> path2,
/// Int32 operation)`: the receiver set to `path1 operation path2`. False,
/// which dart:ui throws on, for an operation past the five or a path that
/// cannot be combined; the receiver is then left as it was.
pub(crate) extern "C" fn op(
    raw: *mut c_void,
    a: *mut c_void,
    b: *mut c_void,
    operation: i32,
) -> bool {
    guard(false, || {
        let Some(op) = Op::from_index(operation) else {
            eprintln!("fl-ui: Path::op: no operation {operation}");
            return false;
        };
        // SAFETY: a and b are the arguments' native fields, copied out:
        // either may be the other, or the receiver.
        let (Some(a), Some(b)) = (unsafe { path::snapshot(a) }, unsafe { path::snapshot(b) })
        else {
            eprintln!("fl-ui: Path::op: not a live Path");
            return false;
        };
        let Some(result) = combine(&a, &b, op) else {
            return false;
        };
        // SAFETY: raw is the receiver's native field.
        unsafe { path::replace(raw, "op", result) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::RRect;

    fn rect(l: f32, t: f32, r: f32, b: f32) -> Path {
        let mut p = Path::default();
        p.add_rect([l, t, r, b]);
        p
    }

    /// The filled area, by sampling a grid of cell centers: `contains`, the
    /// fill rule's own reading, on each.
    fn area(p: &Path, [l, t, r, b]: [f32; 4], step: f32) -> f32 {
        let mut n = 0;
        let mut y = t + step / 2.0;
        while y < b {
            let mut x = l + step / 2.0;
            while x < r {
                n += usize::from(p.contains([x, y]));
                x += step;
            }
            y += step;
        }
        n as f32 * step * step
    }

    #[test]
    fn the_five_operations_on_two_overlapping_squares() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let b = rect(5.0, 5.0, 15.0, 15.0);
        let bounds = [0.0, 0.0, 15.0, 15.0];
        let cases = [
            (Op::Difference, 75.0),
            (Op::Intersect, 25.0),
            (Op::Union, 175.0),
            (Op::Xor, 150.0),
            (Op::ReverseDifference, 75.0),
        ];
        for (op, want) in cases {
            let r = combine(&a, &b, op).unwrap();
            assert_eq!(r.fill, FillType::EvenOdd);
            assert_eq!(area(&r, bounds, 0.5), want, "{op:?}");
        }
        let d = combine(&a, &b, Op::Difference).unwrap();
        assert!(d.contains([2.0, 2.0]) && !d.contains([7.0, 7.0]));
        let r = combine(&a, &b, Op::ReverseDifference).unwrap();
        assert!(r.contains([12.0, 12.0]) && !r.contains([7.0, 7.0]));
    }

    #[test]
    fn a_rounded_window_cut_out_of_a_rectangle() {
        // Wonderous's photo gallery cutout: a screen less a rounded
        // rectangle in its middle.
        let screen = rect(0.0, 0.0, 400.0, 300.0);
        let mut window = Path::default();
        window.add_rrect(&RRect {
            ltrb: [100.0, 75.0, 300.0, 225.0],
            radii: [[6.0, 6.0]; 4],
        });
        let r = combine(&screen, &window, Op::Difference).unwrap();
        assert!(r.contains([10.0, 10.0]), "the dimmed edge");
        assert!(!r.contains([200.0, 150.0]), "the window");
        // Inside the window's corner box, outside its rounding: dimmed.
        assert!(r.contains([100.5, 75.5]));
        assert!(!r.contains([102.0, 77.5]));
        let want = 400.0 * 300.0 - (200.0 * 150.0 - (4.0 - std::f32::consts::PI) * 36.0);
        // Cell centers on a pixel grid: the straight edges, on whole
        // pixels, sampled exactly; the four corners within a few pixels.
        let got = area(&r, [0.0, 0.0, 400.0, 300.0], 1.0);
        assert!((got - want).abs() < 8.0, "{got} {want}");
    }

    #[test]
    fn curves_are_flattened_within_the_tolerance() {
        let mut circle = Path::default();
        circle.add_oval([0.0, 0.0, 200.0, 200.0]);
        let r = combine(&circle, &Path::default(), Op::Union).unwrap();
        let mut worst = 0.0f32;
        for s in &r.segs {
            if let Seg::Move(p) | Seg::Line(p) = s {
                worst = worst.max(((p[0] - 100.0).hypot(p[1] - 100.0) - 100.0).abs());
            }
        }
        // The points are on the circle; between them the chords stray by
        // no more than the tolerance.
        assert!(worst < 1e-3, "{worst}");
        let lines = r.segs.iter().filter(|s| matches!(s, Seg::Line(_))).count();
        assert!((100..2000).contains(&lines), "{lines}");
    }

    #[test]
    fn each_operand_is_filled_by_its_own_rule() {
        // a: a square with a square hole wound the same way. Nonzero fills
        // the hole; even-odd leaves it.
        let mut a = rect(0.0, 0.0, 10.0, 10.0);
        a.add_rect([3.0, 3.0, 7.0, 7.0]);
        let b = rect(20.0, 20.0, 21.0, 21.0);
        let nonzero = combine(&a, &b, Op::Union).unwrap();
        assert!(nonzero.contains([5.0, 5.0]));
        a.fill = FillType::EvenOdd;
        let evenodd = combine(&a, &b, Op::Union).unwrap();
        assert!(!evenodd.contains([5.0, 5.0]));
        assert!(evenodd.contains([1.0, 1.0]) && evenodd.contains([20.5, 20.5]));
    }

    #[test]
    fn extremes_neither_panic_nor_run_long() {
        // What an app may build: coordinates near f32's limits, tiny
        // shapes, degenerate contours, curves bent past the line cap.
        let mut huge = Path::default();
        huge.add_rect([-3.0e38, -3.0e38, 3.0e38, 3.0e38]);
        let mut tiny = Path::default();
        tiny.add_oval([0.0, 0.0, 1.0e-30, 1.0e-30]);
        let mut flat = Path::default();
        flat.move_to([0.0, 0.0]);
        flat.line_to([10.0, 0.0]);
        flat.line_to([20.0, 0.0]);
        flat.close();
        let mut bent = Path::default();
        bent.move_to([0.0, 0.0]);
        bent.cubic_to([1.0e30, -1.0e30], [-1.0e30, 1.0e30], [1.0, 1.0]);
        let mut many = Path::default();
        for i in 0..2000 {
            let x = (i % 50) as f32 * 3.0;
            let y = (i / 50) as f32 * 3.0;
            many.add_oval([x, y, x + 5.0, y + 5.0]);
        }
        let start = std::time::Instant::now();
        let paths = [huge, tiny, flat, bent, many, rect(0.0, 0.0, 10.0, 10.0)];
        for a in &paths {
            for b in &paths {
                for op in [Op::Difference, Op::Union, Op::Xor] {
                    let _ = combine(a, b, op);
                }
            }
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(30));
    }

    #[test]
    fn what_cannot_be_combined_is_refused() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let bad = rect(0.0, 0.0, f32::NAN, 10.0);
        assert_eq!(combine(&a, &bad, Op::Union), None, "not finite");
        assert_eq!(Op::from_index(5), None);
        // Empty operands combine to nothing, not to a failure.
        let empty = combine(&Path::default(), &Path::default(), Op::Union).unwrap();
        assert!(empty.segs.is_empty());
        assert_eq!(combine(&a, &a, Op::Difference).unwrap().segs, vec![]);
    }
}
