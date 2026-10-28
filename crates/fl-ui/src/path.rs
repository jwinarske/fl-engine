//! `Path`, and the `RRect` the drawing natives take.
//!
//! A path is kept as the segments a renderer fills: move, line, quadratic,
//! conic, cubic, close. Everything else dart:ui offers (rectangles, ovals,
//! arcs, rounded rectangles, relative and endpoint forms) becomes those as it
//! is added, the way Skia's `SkPath` does it, so the raster side needs no
//! knowledge of where a segment came from. Arcs become conics, which trace a
//! circle or ellipse exactly.

use std::f32::consts::{FRAC_1_SQRT_2, FRAC_PI_2, TAU};
use std::ffi::c_void;
use std::sync::Arc;

use fl_dart_ffi::{args, guard, peer, sys};

pub type Point = [f32; 2];

/// One path segment. Each ends at its last point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Move(Point),
    Line(Point),
    Quad(Point, Point),
    /// Control point, end point, weight.
    Conic(Point, Point, f32),
    Cubic(Point, Point, Point),
    Close,
}

/// `PathFillType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillType {
    #[default]
    NonZero,
    EvenOdd,
}

/// A path's geometry, as it stood when it was drawn or clipped with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Path {
    pub fill: FillType,
    pub segs: Vec<Seg>,
}

/// A dart:ui `RRect`: the rectangle and each corner's x and y radius, in
/// dart:ui's order, top-left, top-right, bottom-right, bottom-left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RRect {
    pub ltrb: [f32; 4],
    pub radii: [[f32; 2]; 4],
}

impl RRect {
    /// From the 12 floats of `RRect._getValue32` (geometry.dart).
    pub fn from_f32s(v: &[f32]) -> Option<RRect> {
        let v: &[f32; 12] = v.get(..12)?.try_into().ok()?;
        Some(RRect {
            ltrb: [v[0], v[1], v[2], v[3]],
            radii: [[v[4], v[5]], [v[6], v[7]], [v[8], v[9]], [v[10], v[11]]],
        })
    }

    /// The radii scaled down together until adjacent corners fit along
    /// every side, as Skia and Impeller fit them; negative radii are zero.
    pub fn fitted_radii(&self) -> [[f32; 2]; 4] {
        let [l, t, r, b] = self.ltrb;
        let (w, h) = ((r - l).abs(), (b - t).abs());
        let mut radii = self.radii.map(|c| c.map(|v| v.max(0.0)));
        let [tl, tr, br, bl] = radii;
        let mut scale = 1.0f32;
        for (side, a, b) in [
            (w, tl[0], tr[0]),
            (h, tr[1], br[1]),
            (w, br[0], bl[0]),
            (h, bl[1], tl[1]),
        ] {
            if a + b > side && a + b > 0.0 {
                scale = scale.min(side / (a + b));
            }
        }
        if scale < 1.0 {
            for c in &mut radii {
                *c = c.map(|v| v * scale);
            }
        }
        radii
    }
}

impl Path {
    /// Where the next segment starts: the last point, or after a close the
    /// start of the contour it closed. `None` for an empty path.
    pub fn current(&self) -> Option<Point> {
        match self.segs.last()? {
            Seg::Close => self.contour_start(),
            s => end_of(s),
        }
    }

    /// The point of the last move.
    fn contour_start(&self) -> Option<Point> {
        self.segs.iter().rev().find_map(|s| match s {
            Seg::Move(p) => Some(*p),
            _ => None,
        })
    }

    /// A segment that draws needs a contour to draw in. Skia starts one at
    /// the origin in an empty path, and after a close at the closed
    /// contour's start.
    fn ensure_contour(&mut self) {
        match self.segs.last() {
            None => self.segs.push(Seg::Move([0.0, 0.0])),
            Some(Seg::Close) => {
                let p = self.contour_start().unwrap_or([0.0, 0.0]);
                self.segs.push(Seg::Move(p));
            }
            Some(_) => {}
        }
    }

    pub fn move_to(&mut self, p: Point) {
        self.segs.push(Seg::Move(p));
    }

    pub fn line_to(&mut self, p: Point) {
        self.ensure_contour();
        self.segs.push(Seg::Line(p));
    }

    pub fn quad_to(&mut self, c: Point, p: Point) {
        self.ensure_contour();
        self.segs.push(Seg::Quad(c, p));
    }

    /// A weight of 1 is a quadratic; zero, negative or not finite, a line,
    /// as Skia reduces them.
    pub fn conic_to(&mut self, c: Point, p: Point, w: f32) {
        if !(w.is_finite() && w > 0.0) {
            self.line_to(p);
        } else if w == 1.0 {
            self.quad_to(c, p);
        } else {
            self.ensure_contour();
            self.segs.push(Seg::Conic(c, p, w));
        }
    }

    pub fn cubic_to(&mut self, c0: Point, c1: Point, p: Point) {
        self.ensure_contour();
        self.segs.push(Seg::Cubic(c0, c1, p));
    }

    /// Closes the open contour; a no-op with none open.
    pub fn close(&mut self) {
        if matches!(self.segs.last(), Some(s) if !matches!(s, Seg::Close)) {
            self.segs.push(Seg::Close);
        }
    }

    /// The point relative operations offset from.
    fn rel(&self, d: Point) -> Point {
        let c = self.current().unwrap_or([0.0, 0.0]);
        [c[0] + d[0], c[1] + d[1]]
    }

    /// A closed contour, clockwise from the top-left corner.
    pub fn add_rect(&mut self, [l, t, r, b]: [f32; 4]) {
        self.move_to([l, t]);
        self.segs
            .extend([Seg::Line([r, t]), Seg::Line([r, b]), Seg::Line([l, b])]);
        self.close();
    }

    /// The ellipse inscribed in `ltrb`, as four quarter conics from the
    /// right-hand point, clockwise.
    pub fn add_oval(&mut self, ltrb: [f32; 4]) {
        let e = Ellipse::of(ltrb);
        self.move_to(e.at(0.0));
        e.arc_into(&mut self.segs, 0.0, TAU);
        self.close();
    }

    /// The arc of the ellipse inscribed in `ltrb` from `start` sweeping
    /// `sweep` radians (positive is clockwise), as a new open contour.
    pub fn add_arc(&mut self, ltrb: [f32; 4], start: f32, sweep: f32) {
        let e = Ellipse::of(ltrb);
        self.move_to(e.at(start));
        e.arc_into(&mut self.segs, start, sweep);
    }

    /// As [`Path::add_arc`], but continuing the current contour with a line
    /// to the arc's start unless `force_move` or there is no contour.
    pub fn arc_to(&mut self, ltrb: [f32; 4], start: f32, sweep: f32, force_move: bool) {
        let e = Ellipse::of(ltrb);
        if force_move || self.segs.is_empty() {
            self.move_to(e.at(start));
        } else {
            self.line_to(e.at(start));
        }
        e.arc_into(&mut self.segs, start, sweep);
    }

    /// SVG's elliptical arc from the current point to `end` (SVG 1.1
    /// appendix F.6.5). `rotation` is in degrees, as dart:ui documents it;
    /// radii too small to reach are scaled up, and a zero radius is a line.
    pub fn arc_to_point(
        &mut self,
        end: Point,
        radii: Point,
        rotation: f32,
        large: bool,
        clockwise: bool,
    ) {
        let start = self.current().unwrap_or([0.0, 0.0]);
        let (mut rx, mut ry) = (radii[0].abs(), radii[1].abs());
        if start == end || rx == 0.0 || ry == 0.0 || !(rx.is_finite() && ry.is_finite()) {
            self.line_to(end);
            return;
        }
        let (sin, cos) = rotation.to_radians().sin_cos();
        let (hx, hy) = ((start[0] - end[0]) / 2.0, (start[1] - end[1]) / 2.0);
        let x1 = cos * hx + sin * hy;
        let y1 = -sin * hx + cos * hy;
        let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
        if lambda > 1.0 {
            rx *= lambda.sqrt();
            ry *= lambda.sqrt();
        }
        let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
        let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
        let sign = if large == clockwise { -1.0 } else { 1.0 };
        let coef = sign * (num / den).max(0.0).sqrt();
        let (cx1, cy1) = (coef * rx * y1 / ry, -coef * ry * x1 / rx);
        let center = [
            cos * cx1 - sin * cy1 + (start[0] + end[0]) / 2.0,
            sin * cx1 + cos * cy1 + (start[1] + end[1]) / 2.0,
        ];
        let u = [(x1 - cx1) / rx, (y1 - cy1) / ry];
        let v = [(-x1 - cx1) / rx, (-y1 - cy1) / ry];
        let theta = u[1].atan2(u[0]);
        let mut sweep = (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]);
        if clockwise && sweep < 0.0 {
            sweep += TAU;
        } else if !clockwise && sweep > 0.0 {
            sweep -= TAU;
        }
        self.ensure_contour();
        let e = Ellipse {
            center,
            radii: [rx, ry],
            sin,
            cos,
        };
        e.arc_into(&mut self.segs, theta, sweep);
        // Land exactly on the end point, not on its rounding.
        if let Some(Seg::Conic(_, p, _)) = self.segs.last_mut() {
            *p = end;
        }
    }

    /// A polyline through `points` (x, y pairs), as a new contour.
    pub fn add_polygon(&mut self, points: &[f32], close: bool) {
        let mut it = points.chunks_exact(2).map(|p| [p[0], p[1]]);
        let Some(first) = it.next() else { return };
        self.move_to(first);
        self.segs.extend(it.map(Seg::Line));
        if close {
            self.close();
        }
    }

    /// A closed contour clockwise from the end of the top-left corner, each
    /// corner a quarter-ellipse conic. All-zero radii are a rectangle.
    pub fn add_rrect(&mut self, rr: &RRect) {
        let [l, t, r, b] = rr.ltrb;
        let [tl, tr, br, bl] = rr.fitted_radii();
        if [tl, tr, br, bl].iter().flatten().all(|v| *v == 0.0) {
            self.add_rect(rr.ltrb);
            return;
        }
        let w = FRAC_1_SQRT_2;
        self.move_to([l + tl[0], t]);
        self.segs.extend([
            Seg::Line([r - tr[0], t]),
            Seg::Conic([r, t], [r, t + tr[1]], w),
            Seg::Line([r, b - br[1]]),
            Seg::Conic([r, b], [r - br[0], b], w),
            Seg::Line([l + bl[0], b]),
            Seg::Conic([l, b], [l, b - bl[1]], w),
            Seg::Line([l, t + tl[1]]),
            Seg::Conic([l, t], [l + tl[0], t], w),
        ]);
        self.close();
    }

    /// `other`'s segments appended, each point mapped by `m`.
    pub fn add_path(&mut self, other: &Path, m: &Affine) {
        self.segs.extend(other.segs.iter().map(|s| m.seg(s)));
    }

    /// Every point mapped by `m`, the fill type kept.
    pub fn transformed(&self, m: &Affine) -> Path {
        Path {
            fill: self.fill,
            segs: self.segs.iter().map(|s| m.seg(s)).collect(),
        }
    }

    /// Whether `p` is inside the area the path fills, by its fill type:
    /// each contour closed, curves flattened to 16 lines (`FLATTEN_STEPS`),
    /// then the winding number of the result. What `Path.contains` asks,
    /// and the framework asks it of every clip-path layer when it looks
    /// for annotations after a frame.
    pub fn contains(&self, p: Point) -> bool {
        let mut w = Winding { p, n: 0 };
        let (mut start, mut cur): (Option<Point>, Option<Point>) = (None, None);
        for s in &self.segs {
            match *s {
                Seg::Move(q) => {
                    if let (Some(a), Some(b)) = (cur, start) {
                        w.edge(a, b);
                    }
                    start = Some(q);
                    cur = Some(q);
                }
                Seg::Close => {
                    if let (Some(a), Some(b)) = (cur, start) {
                        w.edge(a, b);
                    }
                    cur = start;
                }
                seg => {
                    let Some(a) = cur else { continue };
                    let mut prev = a;
                    for i in 1..=FLATTEN_STEPS {
                        let q = point_on(a, &seg, i as f32 / FLATTEN_STEPS as f32);
                        w.edge(prev, q);
                        prev = q;
                    }
                    cur = Some(prev);
                }
            }
        }
        if let (Some(a), Some(b)) = (cur, start) {
            w.edge(a, b);
        }
        match self.fill {
            FillType::NonZero => w.n != 0,
            FillType::EvenOdd => w.n % 2 != 0,
        }
    }

    /// The bounds of every point, control points included, as Skia's
    /// `getBounds` reports them. All zero for an empty path.
    pub fn bounds(&self) -> [f32; 4] {
        let mut pts = self.segs.iter().flat_map(points_of);
        let Some(first) = pts.next() else {
            return [0.0; 4];
        };
        pts.fold([first[0], first[1], first[0], first[1]], |b, p| {
            [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ]
        })
    }
}

/// Lines each curve is flattened to for [`Path::contains`]. A quarter
/// circle of radius 100 in 16 chords is off by under 0.25 units.
const FLATTEN_STEPS: usize = 16;

/// The point at `t` along `seg`, a curve starting at `a`.
fn point_on(a: Point, seg: &Seg, t: f32) -> Point {
    let lerp = |p: Point, q: Point, t: f32| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
    match *seg {
        Seg::Line(q) => lerp(a, q, t),
        Seg::Quad(c, q) => lerp(lerp(a, c, t), lerp(c, q, t), t),
        Seg::Cubic(c0, c1, q) => {
            let (ab, bc, cd) = (lerp(a, c0, t), lerp(c0, c1, t), lerp(c1, q, t));
            lerp(lerp(ab, bc, t), lerp(bc, cd, t), t)
        }
        Seg::Conic(c, q, w) => {
            // A rational quadratic: weights 1, w, 1.
            let (u, wt) = (1.0 - t, w * 2.0 * t * (1.0 - t));
            let d = u * u + wt + t * t;
            [
                (u * u * a[0] + wt * c[0] + t * t * q[0]) / d,
                (u * u * a[1] + wt * c[1] + t * t * q[1]) / d,
            ]
        }
        Seg::Move(q) => q,
        Seg::Close => a,
    }
}

/// A winding number about `p`, accumulated edge by edge.
struct Winding {
    p: Point,
    n: i32,
}

impl Winding {
    fn edge(&mut self, a: Point, b: Point) {
        let p = self.p;
        // Which side of a->b the point is on: positive is left.
        let side = (b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1]);
        if a[1] <= p[1] {
            if b[1] > p[1] && side > 0.0 {
                self.n += 1;
            }
        } else if b[1] <= p[1] && side < 0.0 {
            self.n -= 1;
        }
    }
}

fn end_of(s: &Seg) -> Option<Point> {
    match *s {
        Seg::Move(p)
        | Seg::Line(p)
        | Seg::Quad(_, p)
        | Seg::Conic(_, p, _)
        | Seg::Cubic(_, _, p) => Some(p),
        Seg::Close => None,
    }
}

fn points_of(s: &Seg) -> Vec<Point> {
    match *s {
        Seg::Move(p) | Seg::Line(p) => vec![p],
        Seg::Quad(c, p) | Seg::Conic(c, p, _) => vec![c, p],
        Seg::Cubic(c0, c1, p) => vec![c0, c1, p],
        Seg::Close => vec![],
    }
}

/// A 2D affine map, `[a, b, c, d, tx, ty]`: x' = a x + c y + tx,
/// y' = b x + d y + ty.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine(pub [f32; 6]);

impl Affine {
    pub fn translate(dx: f32, dy: f32) -> Affine {
        Affine([1.0, 0.0, 0.0, 1.0, dx, dy])
    }

    /// The 2D part of a column-major 4x4 (`Float64List` matrix4). `None`
    /// if it has perspective, which a conic does not survive.
    pub fn from_4x4(m: &[f64]) -> Option<Affine> {
        let m: &[f64; 16] = m.get(..16)?.try_into().ok()?;
        if m[3] != 0.0 || m[7] != 0.0 || m[15] != 1.0 {
            return None;
        }
        Some(Affine(
            [m[0], m[1], m[4], m[5], m[12], m[13]].map(|v| v as f32),
        ))
    }

    pub fn apply(&self, [x, y]: Point) -> Point {
        let [a, b, c, d, tx, ty] = self.0;
        [a * x + c * y + tx, b * x + d * y + ty]
    }

    fn seg(&self, s: &Seg) -> Seg {
        let f = |p| self.apply(p);
        match *s {
            Seg::Move(p) => Seg::Move(f(p)),
            Seg::Line(p) => Seg::Line(f(p)),
            Seg::Quad(c, p) => Seg::Quad(f(c), f(p)),
            Seg::Conic(c, p, w) => Seg::Conic(f(c), f(p), w),
            Seg::Cubic(c0, c1, p) => Seg::Cubic(f(c0), f(c1), f(p)),
            Seg::Close => Seg::Close,
        }
    }
}

/// An ellipse, possibly rotated: `center + R (rx cos a, ry sin a)`.
struct Ellipse {
    center: Point,
    radii: Point,
    sin: f32,
    cos: f32,
}

impl Ellipse {
    fn of([l, t, r, b]: [f32; 4]) -> Ellipse {
        Ellipse {
            center: [(l + r) / 2.0, (t + b) / 2.0],
            radii: [(r - l) / 2.0, (b - t) / 2.0],
            sin: 0.0,
            cos: 1.0,
        }
    }

    /// The point at parameter `a`, scaled by `k` from the center.
    fn at_scaled(&self, a: f32, k: f32) -> Point {
        let (x, y) = (self.radii[0] * a.cos() * k, self.radii[1] * a.sin() * k);
        [
            self.center[0] + self.cos * x - self.sin * y,
            self.center[1] + self.sin * x + self.cos * y,
        ]
    }

    fn at(&self, a: f32) -> Point {
        self.at_scaled(a, 1.0)
    }

    /// Conics from parameter `start` through `sweep`, at most a quarter
    /// turn each: a quarter or less of a circle is one exact conic, and an
    /// ellipse is a circle under an affine map. Nothing for a zero or
    /// non-finite sweep; a sweep past a full turn is clamped to one.
    fn arc_into(&self, segs: &mut Vec<Seg>, start: f32, sweep: f32) {
        if !(sweep.is_finite() && start.is_finite()) || sweep == 0.0 {
            return;
        }
        let sweep = sweep.clamp(-TAU, TAU);
        let n = (sweep.abs() / FRAC_PI_2).ceil().max(1.0);
        let step = sweep / n;
        let half = step / 2.0;
        let w = half.cos();
        for i in 0..n as usize {
            let a0 = start + step * i as f32;
            segs.push(Seg::Conic(
                self.at_scaled(a0 + half, 1.0 / w),
                self.at(a0 + step),
                w,
            ));
        }
    }
}

/// The `Path` peer: a path under construction.
struct NativePath(Path);

/// The geometry of a path argument, for draws and clips that keep it.
///
/// # Safety
/// `raw` is a `_NativePath` argument's native field.
pub(crate) unsafe fn snapshot(raw: *mut c_void) -> Option<Arc<Path>> {
    // SAFETY: per this function's contract.
    unsafe { peer::with::<NativePath, _>(raw, |p| Arc::new(p.0.clone())) }
}

/// Runs `f` on the path behind `raw`, as each path native does.
///
/// # Safety
/// As [`snapshot`].
unsafe fn edit(raw: *mut c_void, what: &str, f: impl FnOnce(&mut Path)) {
    // SAFETY: per this function's contract.
    if unsafe { peer::with::<NativePath, _>(raw, |p| f(&mut p.0)) }.is_none() {
        eprintln!("fl-ui: Path::{what}: not a live Path");
    }
}

/// Attaches `path` to a new `Path` object.
///
/// # Safety
/// A Handle-parameter native: isolate current, scope open.
unsafe fn attach(out: sys::Dart_Handle, path: Path, what: &str) {
    // SAFETY: per this function's contract.
    if let Err(e) = unsafe { peer::attach(out, NativePath(path), 0) } {
        eprintln!("fl-ui: Path::{what}: {e}");
    }
}

/// `Void Function(Handle)`.
pub(crate) extern "C" fn create(wrapper: sys::Dart_Handle) {
    // SAFETY: a Handle-parameter native.
    guard((), || unsafe { attach(wrapper, Path::default(), "Create") });
}

/// `Void Function(Pointer<Void>, Handle outPath)`.
pub(crate) extern "C" fn clone(raw: *mut c_void, out: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: raw is the receiver's native field; a Handle-parameter
        // native.
        unsafe {
            let p = peer::with::<NativePath, _>(raw, |p| p.0.clone()).unwrap_or_default();
            attach(out, p, "clone");
        }
    });
}

/// `Void Function(Pointer<Void>, Handle outPath, Double dx, Double dy)`.
pub(crate) extern "C" fn shift(raw: *mut c_void, out: sys::Dart_Handle, dx: f64, dy: f64) {
    guard((), || {
        let m = Affine::translate(dx as f32, dy as f32);
        // SAFETY: as clone.
        unsafe {
            let p = peer::with::<NativePath, _>(raw, |p| p.0.transformed(&m)).unwrap_or_default();
            attach(out, p, "shift");
        }
    });
}

/// `Void Function(Pointer<Void>, Handle outPath, Handle Float64List matrix4)`.
pub(crate) extern "C" fn transform(raw: *mut c_void, out: sys::Dart_Handle, m: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: as clone; m is a live handle.
        unsafe {
            let Some(m) = args::f64s(m).as_deref().and_then(Affine::from_4x4) else {
                eprintln!("fl-ui: Path::transform: not an affine matrix4");
                attach(out, Path::default(), "transform");
                return;
            };
            let p = peer::with::<NativePath, _>(raw, |p| p.0.transformed(&m)).unwrap_or_default();
            attach(out, p, "transform");
        }
    });
}

/// `Int32 Function(Pointer<Void>)`, a leaf call.
pub(crate) extern "C" fn get_fill_type(raw: *mut c_void) -> i32 {
    guard(0, || {
        // SAFETY: raw is the receiver's native field.
        unsafe { peer::with::<NativePath, _>(raw, |p| p.0.fill as i32) }.unwrap_or(0)
    })
}

/// `Void Function(Pointer<Void>, Int32)`, a leaf call.
pub(crate) extern "C" fn set_fill_type(raw: *mut c_void, fill: i32) {
    guard((), || {
        let fill = if fill == 1 {
            FillType::EvenOdd
        } else {
            FillType::NonZero
        };
        // SAFETY: raw is the receiver's native field.
        unsafe { edit(raw, "setFillType", |p| p.fill = fill) };
    });
}

/// Path natives that only edit the receiver. Each is a leaf call or takes
/// only numbers, so none touches the Dart API.
macro_rules! path_native {
    ($(#[$doc:meta])* $name:ident, $what:literal, ($($arg:ident: $ty:ty),*), $p:ident => $body:expr) => {
        $(#[$doc])*
        pub(crate) extern "C" fn $name(raw: *mut c_void, $($arg: $ty),*) {
            guard((), || {
                // SAFETY: raw is the receiver's native field.
                unsafe { edit(raw, $what, |$p| $body) };
            });
        }
    };
}

fn pt(x: f64, y: f64) -> Point {
    [x as f32, y as f32]
}

fn ltrb(l: f64, t: f64, r: f64, b: f64) -> [f32; 4] {
    [l as f32, t as f32, r as f32, b as f32]
}

path_native!(move_to, "moveTo", (x: f64, y: f64), p => p.move_to(pt(x, y)));
path_native!(relative_move_to, "relativeMoveTo", (x: f64, y: f64), p => {
    let q = p.rel(pt(x, y));
    p.move_to(q)
});
path_native!(line_to, "lineTo", (x: f64, y: f64), p => p.line_to(pt(x, y)));
path_native!(relative_line_to, "relativeLineTo", (x: f64, y: f64), p => {
    let q = p.rel(pt(x, y));
    p.line_to(q)
});
path_native!(quad_to, "quadraticBezierTo", (x1: f64, y1: f64, x2: f64, y2: f64), p => {
    p.quad_to(pt(x1, y1), pt(x2, y2))
});
path_native!(relative_quad_to, "relativeQuadraticBezierTo", (x1: f64, y1: f64, x2: f64, y2: f64), p => {
    let (c, e) = (p.rel(pt(x1, y1)), p.rel(pt(x2, y2)));
    p.quad_to(c, e)
});
path_native!(cubic_to, "cubicTo", (x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64), p => {
    p.cubic_to(pt(x1, y1), pt(x2, y2), pt(x3, y3))
});
path_native!(relative_cubic_to, "relativeCubicTo", (x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64), p => {
    let (c0, c1, e) = (p.rel(pt(x1, y1)), p.rel(pt(x2, y2)), p.rel(pt(x3, y3)));
    p.cubic_to(c0, c1, e)
});
path_native!(conic_to, "conicTo", (x1: f64, y1: f64, x2: f64, y2: f64, w: f64), p => {
    p.conic_to(pt(x1, y1), pt(x2, y2), w as f32)
});
path_native!(relative_conic_to, "relativeConicTo", (x1: f64, y1: f64, x2: f64, y2: f64, w: f64), p => {
    let (c, e) = (p.rel(pt(x1, y1)), p.rel(pt(x2, y2)));
    p.conic_to(c, e, w as f32)
});
path_native!(arc_to, "arcTo", (l: f64, t: f64, r: f64, b: f64, start: f64, sweep: f64, force_move: bool), p => {
    p.arc_to(ltrb(l, t, r, b), start as f32, sweep as f32, force_move)
});
path_native!(arc_to_point, "arcToPoint", (x: f64, y: f64, rx: f64, ry: f64, rotation: f64, large: bool, clockwise: bool), p => {
    p.arc_to_point(pt(x, y), pt(rx, ry), rotation as f32, large, clockwise)
});
path_native!(relative_arc_to_point, "relativeArcToPoint", (x: f64, y: f64, rx: f64, ry: f64, rotation: f64, large: bool, clockwise: bool), p => {
    let e = p.rel(pt(x, y));
    p.arc_to_point(e, pt(rx, ry), rotation as f32, large, clockwise)
});
path_native!(add_rect, "addRect", (l: f64, t: f64, r: f64, b: f64), p => p.add_rect(ltrb(l, t, r, b)));
path_native!(add_oval, "addOval", (l: f64, t: f64, r: f64, b: f64), p => p.add_oval(ltrb(l, t, r, b)));
path_native!(add_arc, "addArc", (l: f64, t: f64, r: f64, b: f64, start: f64, sweep: f64), p => {
    p.add_arc(ltrb(l, t, r, b), start as f32, sweep as f32)
});
path_native!(close, "close", (), p => p.close());
path_native!(reset, "reset", (), p => *p = Path::default());

/// `Void Function(Pointer<Void>, Handle Float32List points, Bool close)`.
pub(crate) extern "C" fn add_polygon(raw: *mut c_void, points: sys::Dart_Handle, close: bool) {
    guard((), || {
        // SAFETY: a Handle-parameter native; points is a live handle.
        let points = unsafe { args::f32s(points) }.unwrap_or_default();
        // SAFETY: raw is the receiver's native field.
        unsafe { edit(raw, "addPolygon", |p| p.add_polygon(&points, close)) };
    });
}

/// `Void Function(Pointer<Void>, Handle Float32List rrect)`.
pub(crate) extern "C" fn add_rrect(raw: *mut c_void, rrect: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: a Handle-parameter native; rrect is a live handle.
        let Some(rr) = unsafe { args::f32s(rrect) }
            .as_deref()
            .and_then(RRect::from_f32s)
        else {
            eprintln!("fl-ui: Path::addRRect: not an RRect");
            return;
        };
        // SAFETY: raw is the receiver's native field.
        unsafe { edit(raw, "addRRect", |p| p.add_rrect(&rr)) };
    });
}

/// `Void Function(Pointer<Void>, Pointer<Void> path, Double dx, Double dy)`.
pub(crate) extern "C" fn add_path(raw: *mut c_void, other: *mut c_void, dx: f64, dy: f64) {
    guard((), || {
        // SAFETY: other is the path argument's native field. Copied out
        // first: it may be the receiver itself.
        let Some(other) = (unsafe { snapshot(other) }) else {
            eprintln!("fl-ui: Path::addPath: not a live Path");
            return;
        };
        let m = Affine::translate(dx as f32, dy as f32);
        // SAFETY: raw is the receiver's native field.
        unsafe { edit(raw, "addPath", |p| p.add_path(&other, &m)) };
    });
}

/// `Void Function(Pointer<Void>, Pointer<Void> path, Double dx, Double dy, Handle Float64List matrix4)`.
pub(crate) extern "C" fn add_path_with_matrix(
    raw: *mut c_void,
    other: *mut c_void,
    dx: f64,
    dy: f64,
    matrix4: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: a Handle-parameter native; matrix4 is a live handle.
        let Some(m) = unsafe { args::f64s(matrix4) }
            .as_deref()
            .and_then(Affine::from_4x4)
        else {
            eprintln!("fl-ui: Path::addPathWithMatrix: not an affine matrix4");
            return;
        };
        // The offset is applied after the matrix (CanvasPath::addPathWithMatrix).
        let [a, b, c, d, tx, ty] = m.0;
        let m = Affine([a, b, c, d, tx + dx as f32, ty + dy as f32]);
        // SAFETY: as add_path.
        let Some(other) = (unsafe { snapshot(other) }) else {
            eprintln!("fl-ui: Path::addPathWithMatrix: not a live Path");
            return;
        };
        // SAFETY: raw is the receiver's native field.
        unsafe { edit(raw, "addPathWithMatrix", |p| p.add_path(&other, &m)) };
    });
}

/// `Bool Function(Pointer<Void>, Double x, Double y)`, a leaf call.
pub(crate) extern "C" fn contains(raw: *mut c_void, x: f64, y: f64) -> bool {
    guard(false, || {
        // SAFETY: raw is the receiver's native field.
        unsafe { peer::with::<NativePath, _>(raw, |p| p.0.contains(pt(x, y))) }.unwrap_or(false)
    })
}

/// `Handle Function(Pointer<Void>)`: a new `Float32List` of l, t, r, b.
pub(crate) extern "C" fn get_bounds(raw: *mut c_void) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open. Made
    // first so a panic below still returns a handle.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: raw is the receiver's native field.
        let b = unsafe { peer::with::<NativePath, _>(raw, |p| p.0.bounds()) }.unwrap_or([0.0; 4]);
        // SAFETY: a Handle-result native.
        unsafe { args::new_f32s(&b) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close_to(a: Point, b: Point) -> bool {
        (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
    }

    #[test]
    fn a_line_without_a_move_starts_at_the_origin() {
        let mut p = Path::default();
        p.line_to([3.0, 4.0]);
        assert_eq!(p.segs, [Seg::Move([0.0, 0.0]), Seg::Line([3.0, 4.0])]);
    }

    #[test]
    fn a_line_after_close_starts_at_the_closed_contour() {
        let mut p = Path::default();
        p.move_to([1.0, 1.0]);
        p.line_to([5.0, 1.0]);
        p.close();
        p.line_to([5.0, 5.0]);
        assert_eq!(p.segs[3], Seg::Move([1.0, 1.0]));
        assert_eq!(p.current(), Some([5.0, 5.0]));
    }

    #[test]
    fn relative_ops_offset_from_the_current_point() {
        let mut p = Path::default();
        p.move_to([10.0, 10.0]);
        let q = p.rel([2.0, 3.0]);
        p.line_to(q);
        assert_eq!(p.current(), Some([12.0, 13.0]));
    }

    #[test]
    fn an_oval_is_four_conics_through_its_extremes() {
        let mut p = Path::default();
        p.add_oval([0.0, 0.0, 20.0, 10.0]);
        let ends: Vec<Point> = p.segs.iter().filter_map(end_of).collect();
        assert_eq!(ends.len(), 5);
        for (got, want) in ends.iter().zip([
            [20.0, 5.0],
            [10.0, 10.0],
            [0.0, 5.0],
            [10.0, 0.0],
            [20.0, 5.0],
        ]) {
            assert!(close_to(*got, want), "{got:?} != {want:?}");
        }
        assert!(matches!(p.segs[1], Seg::Conic(_, _, w) if (w - FRAC_1_SQRT_2).abs() < 1e-6));
        assert_eq!(p.segs.last(), Some(&Seg::Close));
    }

    #[test]
    fn rrect_radii_are_fitted_together() {
        let rr = RRect {
            ltrb: [0.0, 0.0, 100.0, 40.0],
            radii: [[30.0; 2]; 4],
        };
        // 30 + 30 > 40 on the short sides: everything scales by 40/60.
        for c in rr.fitted_radii() {
            assert!(close_to(c, [20.0, 20.0]), "{c:?}");
        }
    }

    #[test]
    fn rrect_with_no_radii_is_a_rect() {
        let mut a = Path::default();
        a.add_rrect(&RRect {
            ltrb: [1.0, 2.0, 3.0, 4.0],
            radii: [[0.0; 2]; 4],
        });
        let mut b = Path::default();
        b.add_rect([1.0, 2.0, 3.0, 4.0]);
        assert_eq!(a, b);
    }

    #[test]
    fn arc_to_point_half_circle() {
        // From (0, 0) to (20, 0), radius 10, clockwise on screen: over the
        // top, through (10, -10) in y-down coordinates.
        let mut p = Path::default();
        p.move_to([0.0, 0.0]);
        p.arc_to_point([20.0, 0.0], [10.0, 10.0], 0.0, false, true);
        let ends: Vec<Point> = p.segs.iter().filter_map(end_of).collect();
        assert_eq!(ends.len(), 3);
        assert!(close_to(ends[1], [10.0, -10.0]), "{:?}", ends[1]);
        assert_eq!(ends[2], [20.0, 0.0]);
    }

    #[test]
    fn arc_to_point_scales_radii_too_small_to_reach() {
        let mut p = Path::default();
        p.move_to([0.0, 0.0]);
        p.arc_to_point([20.0, 0.0], [1.0, 1.0], 0.0, false, false);
        let ends: Vec<Point> = p.segs.iter().filter_map(end_of).collect();
        // Counter-clockwise: under the bottom, the radius scaled up to 10.
        assert!(close_to(ends[1], [10.0, 10.0]), "{:?}", ends[1]);
    }

    #[test]
    fn contains_follows_the_fill_type() {
        // A square with a square hole, both wound the same way.
        let mut p = Path::default();
        p.add_rect([0.0, 0.0, 10.0, 10.0]);
        p.add_rect([3.0, 3.0, 7.0, 7.0]);
        assert!(p.contains([1.0, 1.0]));
        assert!(p.contains([5.0, 5.0]), "nonzero: the hole winds twice");
        p.fill = FillType::EvenOdd;
        assert!(!p.contains([5.0, 5.0]), "even-odd: the hole is out");
        assert!(!p.contains([11.0, 5.0]));
    }

    #[test]
    fn contains_closes_open_contours_and_follows_curves() {
        let mut p = Path::default();
        p.add_oval([0.0, 0.0, 20.0, 20.0]);
        assert!(p.contains([10.0, 10.0]));
        // Inside the bounding box, outside the circle.
        assert!(!p.contains([1.0, 1.0]));
        // An open triangle still fills.
        let mut t = Path::default();
        t.move_to([0.0, 0.0]);
        t.line_to([10.0, 0.0]);
        t.line_to([0.0, 10.0]);
        assert!(t.contains([2.0, 2.0]));
        assert!(!Path::default().contains([0.0, 0.0]));
    }

    #[test]
    fn bounds_include_control_points() {
        let mut p = Path::default();
        p.move_to([0.0, 0.0]);
        p.quad_to([5.0, -10.0], [10.0, 0.0]);
        assert_eq!(p.bounds(), [0.0, -10.0, 10.0, 0.0]);
        assert_eq!(Path::default().bounds(), [0.0; 4]);
    }

    #[test]
    fn perspective_matrices_are_refused() {
        let mut m = [0.0f64; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        m[12] = 7.0;
        assert_eq!(
            Affine::from_4x4(&m).map(|a| a.apply([1.0, 1.0])),
            Some([8.0, 1.0])
        );
        m[3] = 0.5;
        assert_eq!(Affine::from_4x4(&m), None);
    }
}
