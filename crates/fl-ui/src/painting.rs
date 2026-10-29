//! `PictureRecorder`, `Canvas` and `Picture`.
//!
//! A recorder and the canvas drawing into it share one draw list; ending the
//! recording moves the list into an immutable [`Picture`]. Each canvas call
//! is kept as one [`Draw`], in order, with its Paint decoded; replaying them
//! is the raster side's business.

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use fl_dart_ffi::{args, guard, peer, sys};

use crate::image::{image_of, image_shader_of, ImageData, ImageShader};
use crate::path::{self, Path, RRect};

/// One canvas call, as recorded. Geometry is in the canvas's coordinates at
/// the time of the call, as dart:ui gives it.
#[derive(Debug, Clone, PartialEq)]
pub enum Draw {
    Save,
    /// `saveLayer`: drawn into a layer that is composited with `paint` (its
    /// alpha, blend mode and filters) at the matching restore.
    SaveLayer {
        bounds: Option<[f32; 4]>,
        paint: Paint,
    },
    Restore,
    Translate(f32, f32),
    Scale(f32, f32),
    /// Radians, clockwise.
    Rotate(f32),
    Skew(f32, f32),
    /// A column-major 4x4, as `Float64List` carries it.
    Transform([f64; 16]),
    ClipRect {
        ltrb: [f32; 4],
        op: ClipOp,
        anti_alias: bool,
    },
    ClipRRect {
        rrect: RRect,
        anti_alias: bool,
    },
    ClipPath {
        path: Arc<Path>,
        anti_alias: bool,
    },
    /// The whole clip filled with `paint`.
    Paint(Paint),
    /// The whole clip filled with a color: straight sRGB, 0 to 1, and a
    /// `BlendMode` index.
    Color {
        color: [f32; 4],
        blend_mode: i32,
    },
    Line {
        from: [f32; 2],
        to: [f32; 2],
        paint: Paint,
    },
    Rect {
        ltrb: [f32; 4],
        paint: Paint,
    },
    RRect {
        rrect: RRect,
        paint: Paint,
    },
    /// The ring between two rounded rectangles.
    DRRect {
        outer: RRect,
        inner: RRect,
        paint: Paint,
    },
    Oval {
        ltrb: [f32; 4],
        paint: Paint,
    },
    Circle {
        center: [f32; 2],
        radius: f32,
        paint: Paint,
    },
    Path {
        path: Arc<Path>,
        paint: Paint,
    },
    Picture(Arc<Picture>),
    /// `drawImage` and `drawImageRect`: the image's `src` rect, in texels,
    /// drawn into `dst`.
    Image {
        image: Arc<ImageData>,
        src: [f32; 4],
        dst: [f32; 4],
        paint: Paint,
        /// A `FilterQuality` index.
        quality: i32,
    },
    /// `drawImageNine`: `center`, in texels, stretches; the rest does not.
    ImageNine {
        image: Arc<ImageData>,
        center: [f32; 4],
        dst: [f32; 4],
        paint: Paint,
        quality: i32,
    },
    /// `drawAtlas`: a sprite per `RSTransform` (scos, ssin, tx, ty) and
    /// source rect, each colored if `colors` is given, blended with the
    /// sprite by `blend_mode`.
    Atlas {
        image: Arc<ImageData>,
        transforms: Vec<[f32; 4]>,
        rects: Vec<[f32; 4]>,
        colors: Option<Vec<u32>>,
        blend_mode: i32,
        paint: Paint,
        quality: i32,
    },
}

/// `ClipOp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipOp {
    Difference,
    Intersect,
}

/// `PaintingStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Fill,
    Stroke,
}

/// `StrokeCap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    Butt,
    Round,
    Square,
}

/// `StrokeJoin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    Miter,
    Round,
    Bevel,
}

/// A dart:ui `Paint`: its `ByteData`, 68 bytes in the layout
/// painting.dart:1328-1365 declares, and its objects list. Of the objects,
/// an `ImageShader` is decoded ([`Paint::shader`]); any other shader, and
/// color and image filters, are not yet, and [`Paint::objects`] says one
/// was set.
#[derive(Debug, Clone, PartialEq)]
pub struct Paint {
    /// Straight (not premultiplied) red, green, blue, alpha, 0 to 1, in
    /// [`Paint::color_space`].
    pub color: [f32; 4],
    /// `ColorSpace` index: 0 sRGB, 1 extended sRGB, 2 Display P3.
    pub color_space: i32,
    pub anti_alias: bool,
    /// `BlendMode` index; 3 is `srcOver`.
    pub blend_mode: i32,
    pub style: Style,
    pub stroke_width: f32,
    pub stroke_cap: Cap,
    pub stroke_join: Join,
    pub stroke_miter_limit: f32,
    /// A `FilterQuality` index: what an image shader with none of its own
    /// samples at.
    pub filter_quality: i32,
    /// A blur mask filter: its `BlurStyle` index and sigma.
    pub mask_blur: Option<(i32, f32)>,
    pub invert_colors: bool,
    /// A shader other than an image shader, a color filter or an image
    /// filter is set.
    pub objects: bool,
    pub shader: Option<Arc<ImageShader>>,
}

impl Default for Paint {
    /// `Paint()`: opaque black, anti-aliased, srcOver, fill, butt caps,
    /// miter joins, miter limit 4.
    fn default() -> Self {
        Paint {
            color: [0.0, 0.0, 0.0, 1.0],
            color_space: 0,
            anti_alias: true,
            blend_mode: BLEND_SRC_OVER,
            style: Style::Fill,
            stroke_width: 0.0,
            stroke_cap: Cap::Butt,
            stroke_join: Join::Miter,
            stroke_miter_limit: MITER_LIMIT_DEFAULT,
            filter_quality: 0,
            mask_blur: None,
            invert_colors: false,
            objects: false,
            shader: None,
        }
    }
}

const BLEND_SRC_OVER: i32 = 3;
const MITER_LIMIT_DEFAULT: f32 = 4.0;

impl Paint {
    /// Decodes the `ByteData`, little-endian (natives.dart:44). Fields the
    /// framework stores relative to a default are restored here: alpha is
    /// stored as `1 - a`, anti-alias as "0 means on", blend mode XOR
    /// srcOver, miter limit less 4, so all-zero bytes are `Paint()`. `None`
    /// if the data is short.
    pub fn decode(data: &[u8]) -> Option<Paint> {
        let word = |i: usize| -> Option<[u8; 4]> { data.get(i * 4..i * 4 + 4)?.try_into().ok() };
        let i32_at = |i: usize| word(i).map(i32::from_le_bytes);
        let f32_at = |i: usize| word(i).map(f32::from_le_bytes);
        Some(Paint {
            anti_alias: i32_at(0)? == 0,
            color: [f32_at(1)?, f32_at(2)?, f32_at(3)?, 1.0 - f32_at(4)?],
            color_space: i32_at(5)?,
            blend_mode: i32_at(6)? ^ BLEND_SRC_OVER,
            style: if i32_at(7)? == 1 {
                Style::Stroke
            } else {
                Style::Fill
            },
            stroke_width: f32_at(8)?,
            stroke_cap: match i32_at(9)? {
                1 => Cap::Round,
                2 => Cap::Square,
                _ => Cap::Butt,
            },
            stroke_join: match i32_at(10)? {
                1 => Join::Round,
                2 => Join::Bevel,
                _ => Join::Miter,
            },
            stroke_miter_limit: f32_at(11)? + MITER_LIMIT_DEFAULT,
            filter_quality: i32_at(12)?,
            mask_blur: (i32_at(13)? != 0).then_some((i32_at(14)?, f32_at(15)?)),
            invert_colors: i32_at(16)? != 0,
            objects: false,
            shader: None,
        })
    }
}

/// A finished recording. Immutable, shared by every layer that adds it.
#[derive(Debug, Default, PartialEq)]
pub struct Picture {
    pub draws: Vec<Draw>,
}

/// What a recorder and its canvas share: the draws so far, and how many
/// saves are open (`getSaveCount` less one).
#[derive(Default)]
struct Recording {
    draws: Vec<Draw>,
    saves: usize,
}

impl Recording {
    fn save(&mut self, d: Draw) {
        self.draws.push(d);
        self.saves += 1;
    }

    /// An unbalanced restore is ignored, as Skia and Impeller ignore it.
    fn restore(&mut self) {
        if self.saves > 0 {
            self.draws.push(Draw::Restore);
            self.saves -= 1;
        }
    }
}

type Shared = Arc<Mutex<Recording>>;

/// The `PictureRecorder` peer.
struct Recorder {
    rec: Shared,
}

/// The `Canvas` peer: draws into its recorder's list.
struct CanvasPeer {
    rec: Shared,
}

/// Runs `f` on the recording behind a canvas. A canvas whose recorder has
/// ended records into a list nobody reads, as the engine's does.
///
/// # Safety
/// `canvas` is a `Canvas` receiver's native field.
unsafe fn on<R>(canvas: *mut c_void, f: impl FnOnce(&mut Recording) -> R) -> Option<R> {
    // SAFETY: per this function's contract.
    unsafe {
        peer::with::<CanvasPeer, _>(canvas, |c| {
            f(&mut c.rec.lock().unwrap_or_else(|e| e.into_inner()))
        })
    }
}

/// Records `d` on `canvas`.
///
/// # Safety
/// As [`on`].
unsafe fn push(canvas: *mut c_void, d: Draw) {
    // SAFETY: per this function's contract.
    unsafe { on(canvas, |r| r.draws.push(d)) };
}

/// `Void Function(Handle)`.
pub(crate) extern "C" fn recorder_create(wrapper: sys::Dart_Handle) {
    guard((), || {
        let r = Recorder {
            rec: Arc::default(),
        };
        // SAFETY: a Handle-parameter native: isolate current, scope open.
        if let Err(e) = unsafe { peer::attach(wrapper, r, 0) } {
            eprintln!("fl-ui: PictureRecorder::Create: {e}");
        }
    });
}

/// `Void Function(Handle canvas, Pointer<Void> recorder, Double l, t, r, b)`.
/// The cull rect is not kept: emblema clips to its target.
pub(crate) extern "C" fn canvas_create(
    wrapper: sys::Dart_Handle,
    recorder: *mut c_void,
    _l: f64,
    _t: f64,
    _r: f64,
    _b: f64,
) {
    guard((), || {
        // SAFETY: recorder is the PictureRecorder argument's native field.
        let Some(rec) = (unsafe { peer::with::<Recorder, _>(recorder, |r| Arc::clone(&r.rec)) })
        else {
            eprintln!("fl-ui: Canvas::Create: not a live PictureRecorder");
            return;
        };
        // SAFETY: as recorder_create.
        if let Err(e) = unsafe { peer::attach(wrapper, CanvasPeer { rec }, 0) } {
            eprintln!("fl-ui: Canvas::Create: {e}");
        }
    });
}

/// `Void Function(Pointer<Void> recorder, Handle outPicture)`. Saves left
/// open are closed, so a picture is always balanced.
pub(crate) extern "C" fn recorder_end(recorder: *mut c_void, out_picture: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: recorder is the receiver's native field.
        let draws = unsafe {
            peer::with::<Recorder, _>(recorder, |r| {
                let mut rec = r.rec.lock().unwrap_or_else(|e| e.into_inner());
                while rec.saves > 0 {
                    rec.restore();
                }
                std::mem::take(&mut rec.draws)
            })
        }
        .unwrap_or_default();
        let picture = Arc::new(Picture { draws });
        // SAFETY: as recorder_create.
        if let Err(e) = unsafe { peer::attach(out_picture, picture, 0) } {
            eprintln!("fl-ui: PictureRecorder::endRecording: {e}");
        }
    });
}

/// `Void Function(Pointer<Void>)`.
pub(crate) extern "C" fn picture_dispose(picture: *mut c_void) {
    guard((), || {
        // SAFETY: picture is the receiver's native field.
        drop(unsafe { peer::take::<Arc<Picture>>(picture) });
    });
}

/// The Paint a draw native was given. Malformed data draws with `Paint()`
/// and says so, rather than dropping the draw.
///
/// # Safety
/// As [`fl_dart_ffi::args::bytes`], for both handles.
unsafe fn paint(objects: sys::Dart_Handle, data: sys::Dart_Handle) -> Paint {
    // SAFETY: per this function's contract.
    let bytes = unsafe { args::bytes(data) };
    let p = bytes.as_deref().and_then(Paint::decode).unwrap_or_else(|| {
        eprintln!("fl-ui: malformed Paint data; drawing with Paint()");
        Paint::default()
    });
    // SAFETY: as above.
    let (shader, objects) = unsafe { paint_objects(objects) };
    Paint {
        objects,
        shader,
        ..p
    }
}

/// A paint's objects list, `[shader, colorFilter, imageFilter]`
/// (painting.dart `_kShaderIndex` and on): the image shader, if the shader
/// is one, and whether anything else is set.
///
/// # Safety
/// As [`fl_dart_ffi::args::bytes`].
unsafe fn paint_objects(objects: sys::Dart_Handle) -> (Option<Arc<ImageShader>>, bool) {
    // SAFETY: per this function's contract.
    unsafe {
        if !args::any_non_null(objects) {
            return (None, false);
        }
        let mut n: isize = 0;
        if sys::Dart_IsError(sys::Dart_ListLength(objects, &mut n)) {
            return (None, true);
        }
        let mut shader = None;
        let mut other = false;
        for i in 0..n {
            let o = sys::Dart_ListGetAt(objects, i);
            if sys::Dart_IsError(o) || sys::Dart_IsNull(o) {
                continue;
            }
            match (i, image_shader_of(o)) {
                (0, Some(s)) => shader = Some(s),
                _ => other = true,
            }
        }
        (shader, other)
    }
}

/// An `RRect` argument; `None`, said, if it is not one.
///
/// # Safety
/// As [`fl_dart_ffi::args::bytes`].
unsafe fn rrect(h: sys::Dart_Handle, what: &str) -> Option<RRect> {
    // SAFETY: per this function's contract.
    let rr = unsafe { args::f32s(h) }
        .as_deref()
        .and_then(RRect::from_f32s);
    if rr.is_none() {
        eprintln!("fl-ui: Canvas::{what}: not an RRect");
    }
    rr
}

fn ltrb(l: f64, t: f64, r: f64, b: f64) -> [f32; 4] {
    [l as f32, t as f32, r as f32, b as f32]
}

/// Canvas natives that take only numbers: leaf calls, or calls with no
/// handle, so none touches the Dart API.
macro_rules! canvas_native {
    ($(#[$doc:meta])* $name:ident, ($($arg:ident: $ty:ty),*), $r:ident => $body:expr) => {
        $(#[$doc])*
        pub(crate) extern "C" fn $name(canvas: *mut c_void, $($arg: $ty),*) {
            guard((), || {
                // SAFETY: canvas is the receiver's native field.
                unsafe { on(canvas, |$r| $body) };
            });
        }
    };
}

canvas_native!(save, (), r => r.save(Draw::Save));
canvas_native!(restore, (), r => r.restore());
canvas_native!(
    /// Restores until `count` saves are open, never below the first.
    restore_to_count, (count: i32), r => {
        let keep = usize::try_from(count.max(1) - 1).unwrap_or(0);
        while r.saves > keep {
            r.restore();
        }
    }
);
canvas_native!(translate, (dx: f64, dy: f64), r => r.draws.push(Draw::Translate(dx as f32, dy as f32)));
canvas_native!(scale, (sx: f64, sy: f64), r => r.draws.push(Draw::Scale(sx as f32, sy as f32)));
canvas_native!(rotate, (radians: f64), r => r.draws.push(Draw::Rotate(radians as f32)));
canvas_native!(skew, (sx: f64, sy: f64), r => r.draws.push(Draw::Skew(sx as f32, sy as f32)));
canvas_native!(
    /// `Void Function(Pointer<Void>, Double l, t, r, b, Int32 clipOp, Bool doAntiAlias)`.
    clip_rect, (l: f64, t: f64, rr: f64, b: f64, op: i32, anti_alias: bool), r => {
        r.draws.push(Draw::ClipRect {
            ltrb: ltrb(l, t, rr, b),
            op: if op == 0 { ClipOp::Difference } else { ClipOp::Intersect },
            anti_alias,
        })
    }
);
canvas_native!(
    /// `Void Function(Pointer<Void>, Uint32 color, Int32 blendMode)`: an
    /// ARGB `Color.value`, sRGB.
    draw_color, (color: u32, blend_mode: i32), r => {
        let c = color.to_be_bytes().map(|v| f32::from(v) / 255.0);
        r.draws.push(Draw::Color { color: [c[1], c[2], c[3], c[0]], blend_mode })
    }
);

/// `Int32 Function(Pointer<Void>)`, a leaf call.
pub(crate) extern "C" fn get_save_count(canvas: *mut c_void) -> i32 {
    guard(1, || {
        // SAFETY: canvas is the receiver's native field.
        unsafe { on(canvas, |r| i32::try_from(r.saves + 1).unwrap_or(i32::MAX)) }.unwrap_or(1)
    })
}

/// `Void Function(Pointer<Void>, Handle Float64List matrix4)`.
pub(crate) extern "C" fn transform(canvas: *mut c_void, matrix4: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: a Handle-parameter native; matrix4 is a live handle.
        let Some(m) = unsafe { args::f64s(matrix4) }.and_then(|v| v.try_into().ok()) else {
            eprintln!("fl-ui: Canvas::transform: not a matrix4");
            return;
        };
        // SAFETY: canvas is the receiver's native field.
        unsafe { push(canvas, Draw::Transform(m)) };
    });
}

/// `Void Function(Pointer<Void>, Handle paintObjects, Handle paintData)`.
pub(crate) extern "C" fn save_layer_without_bounds(
    canvas: *mut c_void,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: a Handle-parameter native; both are live handles.
        let paint = unsafe { paint(objects, data) };
        // SAFETY: canvas is the receiver's native field.
        unsafe {
            on(canvas, |r| {
                r.save(Draw::SaveLayer {
                    bounds: None,
                    paint,
                })
            })
        };
    });
}

/// `Void Function(Pointer<Void>, Double l, t, r, b, Handle paintObjects, Handle paintData)`.
pub(crate) extern "C" fn save_layer(
    canvas: *mut c_void,
    l: f64,
    t: f64,
    r: f64,
    b: f64,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: as save_layer_without_bounds.
        let paint = unsafe { paint(objects, data) };
        // SAFETY: canvas is the receiver's native field.
        unsafe {
            on(canvas, |rec| {
                rec.save(Draw::SaveLayer {
                    bounds: Some(ltrb(l, t, r, b)),
                    paint,
                })
            })
        };
    });
}

/// `Void Function(Pointer<Void>, Handle Float32List rrect, Bool doAntiAlias)`.
pub(crate) extern "C" fn clip_rrect(canvas: *mut c_void, rr: sys::Dart_Handle, anti_alias: bool) {
    guard((), || {
        // SAFETY: a Handle-parameter native; rr is a live handle.
        let Some(rrect) = (unsafe { rrect(rr, "clipRRect") }) else {
            return;
        };
        // SAFETY: canvas is the receiver's native field.
        unsafe { push(canvas, Draw::ClipRRect { rrect, anti_alias }) };
    });
}

/// `Void Function(Pointer<Void>, Pointer<Void> path, Bool doAntiAlias)`.
pub(crate) extern "C" fn clip_path(canvas: *mut c_void, p: *mut c_void, anti_alias: bool) {
    guard((), || {
        // SAFETY: p is the path argument's native field.
        let Some(path) = (unsafe { path::snapshot(p) }) else {
            eprintln!("fl-ui: Canvas::clipPath: not a live Path");
            return;
        };
        // SAFETY: canvas is the receiver's native field.
        unsafe { push(canvas, Draw::ClipPath { path, anti_alias }) };
    });
}

/// `Void Function(Pointer<Void>, Handle paintObjects, Handle paintData)`.
pub(crate) extern "C" fn draw_paint(
    canvas: *mut c_void,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: a Handle-parameter native; both are live handles.
        let paint = unsafe { paint(objects, data) };
        // SAFETY: canvas is the receiver's native field.
        unsafe { push(canvas, Draw::Paint(paint)) };
    });
}

/// `Void Function(Pointer<Void>, Double x1, y1, x2, y2, Handle paintObjects, Handle paintData)`.
pub(crate) extern "C" fn draw_line(
    canvas: *mut c_void,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: as draw_paint.
        let paint = unsafe { paint(objects, data) };
        let d = Draw::Line {
            from: [x1 as f32, y1 as f32],
            to: [x2 as f32, y2 as f32],
            paint,
        };
        // SAFETY: canvas is the receiver's native field.
        unsafe { push(canvas, d) };
    });
}

/// `Void Function(Pointer<Void> canvas, Double l, t, r, b, Handle paintObjects, Handle paintData)`.
pub(crate) extern "C" fn draw_rect(
    canvas: *mut c_void,
    l: f64,
    t: f64,
    r: f64,
    b: f64,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: as draw_paint.
        let paint = unsafe { paint(objects, data) };
        let d = Draw::Rect {
            ltrb: ltrb(l, t, r, b),
            paint,
        };
        // SAFETY: canvas is the receiver's native field.
        unsafe { push(canvas, d) };
    });
}

/// `Void Function(Pointer<Void>, Handle Float32List rrect, Handle paintObjects, Handle paintData)`.
pub(crate) extern "C" fn draw_rrect(
    canvas: *mut c_void,
    rr: sys::Dart_Handle,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: a Handle-parameter native; all are live handles.
        let (Some(rrect), paint) = (unsafe { (rrect(rr, "drawRRect"), paint(objects, data)) })
        else {
            return;
        };
        // SAFETY: canvas is the receiver's native field.
        unsafe { push(canvas, Draw::RRect { rrect, paint }) };
    });
}

/// `Void Function(Pointer<Void>, Handle outer, Handle inner, Handle paintObjects, Handle paintData)`.
pub(crate) extern "C" fn draw_drrect(
    canvas: *mut c_void,
    outer: sys::Dart_Handle,
    inner: sys::Dart_Handle,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: as draw_rrect.
        let (Some(outer), Some(inner), paint) = (unsafe {
            (
                rrect(outer, "drawDRRect"),
                rrect(inner, "drawDRRect"),
                paint(objects, data),
            )
        }) else {
            return;
        };
        // SAFETY: canvas is the receiver's native field.
        unsafe {
            push(
                canvas,
                Draw::DRRect {
                    outer,
                    inner,
                    paint,
                },
            )
        };
    });
}

/// `Void Function(Pointer<Void>, Double l, t, r, b, Handle paintObjects, Handle paintData)`.
pub(crate) extern "C" fn draw_oval(
    canvas: *mut c_void,
    l: f64,
    t: f64,
    r: f64,
    b: f64,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: as draw_paint.
        let paint = unsafe { paint(objects, data) };
        let d = Draw::Oval {
            ltrb: ltrb(l, t, r, b),
            paint,
        };
        // SAFETY: canvas is the receiver's native field.
        unsafe { push(canvas, d) };
    });
}

/// `Void Function(Pointer<Void>, Double x, y, radius, Handle paintObjects, Handle paintData)`.
pub(crate) extern "C" fn draw_circle(
    canvas: *mut c_void,
    x: f64,
    y: f64,
    radius: f64,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: as draw_paint.
        let paint = unsafe { paint(objects, data) };
        let d = Draw::Circle {
            center: [x as f32, y as f32],
            radius: radius as f32,
            paint,
        };
        // SAFETY: canvas is the receiver's native field.
        unsafe { push(canvas, d) };
    });
}

/// `Void Function(Pointer<Void>, Double l, t, r, b, Double startAngle,
/// Double sweepAngle, Bool useCenter, Handle paintObjects, Handle paintData)`.
/// Recorded as the path it draws: the arc of the oval in l, t, r, b, closed
/// through the center with `useCenter`, as a wedge.
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn draw_arc(
    canvas: *mut c_void,
    l: f64,
    t: f64,
    r: f64,
    b: f64,
    start: f64,
    sweep: f64,
    use_center: bool,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: as draw_paint.
        let paint = unsafe { paint(objects, data) };
        let oval = ltrb(l, t, r, b);
        let mut path = Path::default();
        if use_center {
            path.move_to([(oval[0] + oval[2]) / 2.0, (oval[1] + oval[3]) / 2.0]);
            path.arc_to(oval, start as f32, sweep as f32, false);
            path.close();
        } else {
            path.add_arc(oval, start as f32, sweep as f32);
        }
        let d = Draw::Path {
            path: Arc::new(path),
            paint,
        };
        // SAFETY: canvas is the receiver's native field.
        unsafe { push(canvas, d) };
    });
}

/// `Void Function(Pointer<Void>, Pointer<Void> path, Handle paintObjects, Handle paintData)`.
pub(crate) extern "C" fn draw_path(
    canvas: *mut c_void,
    p: *mut c_void,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: p is the path argument's native field.
        let Some(path) = (unsafe { path::snapshot(p) }) else {
            eprintln!("fl-ui: Canvas::drawPath: not a live Path");
            return;
        };
        // SAFETY: as draw_paint.
        let paint = unsafe { paint(objects, data) };
        // SAFETY: canvas is the receiver's native field.
        unsafe { push(canvas, Draw::Path { path, paint }) };
    });
}

/// `Void Function(Pointer<Void>, Pointer<Void> path, Uint32 color, Double
/// elevation, Bool transparentOccluder)`. A stub until M3's stage 3: the
/// shadow is not drawn.
pub(crate) extern "C" fn draw_shadow(
    _canvas: *mut c_void,
    _path: *mut c_void,
    _color: u32,
    _elevation: f64,
    _transparent: bool,
) {
}

/// Records an image draw, or says why not: a disposed image draws nothing,
/// as the engine's draws nothing.
///
/// # Safety
/// `canvas` and `image` are native fields of this call's arguments.
unsafe fn push_image(
    canvas: *mut c_void,
    image: *mut c_void,
    what: &str,
    d: impl FnOnce(Arc<ImageData>) -> Draw,
) -> sys::Dart_Handle {
    // SAFETY: per this function's contract.
    unsafe {
        match image_of(image) {
            Some(i) => push(canvas, d(i)),
            None => eprintln!("fl-ui: Canvas::{what}: not a live Image"),
        }
        args::null()
    }
}

/// `Handle Function(Pointer<Void>, Pointer<Void> image, Double x, Double y,
/// Handle paintObjects, Handle paintData, Int32 filterQuality)`.
pub(crate) extern "C" fn draw_image(
    canvas: *mut c_void,
    image: *mut c_void,
    x: f64,
    y: f64,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
    quality: i32,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: as draw_paint; image is the Image argument's native field.
        unsafe {
            let paint = paint(objects, data);
            push_image(canvas, image, "drawImage", |image| {
                let (w, h) = (image.width as f32, image.height as f32);
                let (x, y) = (x as f32, y as f32);
                Draw::Image {
                    image,
                    src: [0.0, 0.0, w, h],
                    dst: [x, y, x + w, y + h],
                    paint,
                    quality,
                }
            })
        }
    })
}

/// `Handle Function(Pointer<Void>, Pointer<Void> image, Double srcLeft,
/// srcTop, srcRight, srcBottom, dstLeft, dstTop, dstRight, dstBottom,
/// Handle paintObjects, Handle paintData, Int32 filterQuality)`.
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn draw_image_rect(
    canvas: *mut c_void,
    image: *mut c_void,
    sl: f64,
    st: f64,
    sr: f64,
    sb: f64,
    dl: f64,
    dt: f64,
    dr: f64,
    db: f64,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
    quality: i32,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: as draw_image.
        unsafe {
            let paint = paint(objects, data);
            push_image(canvas, image, "drawImageRect", |image| Draw::Image {
                image,
                src: ltrb(sl, st, sr, sb),
                dst: ltrb(dl, dt, dr, db),
                paint,
                quality,
            })
        }
    })
}

/// `Handle Function(Pointer<Void>, Pointer<Void> image, Double
/// centerLeft, centerTop, centerRight, centerBottom, dstLeft, dstTop,
/// dstRight, dstBottom, Handle paintObjects, Handle paintData, Int32
/// filterQuality)`.
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn draw_image_nine(
    canvas: *mut c_void,
    image: *mut c_void,
    cl: f64,
    ct: f64,
    cr: f64,
    cb: f64,
    dl: f64,
    dt: f64,
    dr: f64,
    db: f64,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
    quality: i32,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: as draw_image.
        unsafe {
            let paint = paint(objects, data);
            push_image(canvas, image, "drawImageNine", |image| Draw::ImageNine {
                image,
                center: ltrb(cl, ct, cr, cb),
                dst: ltrb(dl, dt, dr, db),
                paint,
                quality,
            })
        }
    })
}

/// `Handle Function(Pointer<Void>, Handle paintObjects, Handle paintData,
/// Int32 filterQuality, Pointer<Void> atlas, Handle Float32List
/// rstTransforms, Handle Float32List rects, Handle Int32List? colors, Int32
/// blendMode, Handle Float32List? cullRect)`. The cull rect is not kept.
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn draw_atlas(
    canvas: *mut c_void,
    objects: sys::Dart_Handle,
    data: sys::Dart_Handle,
    quality: i32,
    atlas: *mut c_void,
    transforms: sys::Dart_Handle,
    rects: sys::Dart_Handle,
    colors: sys::Dart_Handle,
    blend_mode: i32,
    _cull: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: as draw_image; the list handles are live.
        unsafe {
            let paint = paint(objects, data);
            let quads = |h| {
                args::f32s(h).map(|v| {
                    v.chunks_exact(4)
                        .map(|c| [c[0], c[1], c[2], c[3]])
                        .collect::<Vec<_>>()
                })
            };
            let (Some(transforms), Some(rects)) = (quads(transforms), quads(rects)) else {
                eprintln!("fl-ui: Canvas::drawAtlas: malformed transforms or rects");
                return null;
            };
            if transforms.len() != rects.len() {
                eprintln!("fl-ui: Canvas::drawAtlas: a transform for each rect is needed");
                return null;
            }
            let colors = (!sys::Dart_IsNull(colors))
                .then(|| args::bytes(colors))
                .flatten()
                .map(|b| {
                    b.chunks_exact(4)
                        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect::<Vec<_>>()
                });
            push_image(canvas, atlas, "drawAtlas", |image| Draw::Atlas {
                image,
                transforms,
                rects,
                colors,
                blend_mode,
                paint,
                quality,
            })
        }
    })
}

/// `Void Function(Pointer<Void>, Pointer<Void> picture)`.
pub(crate) extern "C" fn draw_picture(canvas: *mut c_void, picture: *mut c_void) {
    guard((), || {
        // SAFETY: picture is the Picture argument's native field.
        let Some(p) = (unsafe { peer::with::<Arc<Picture>, _>(picture, |p| Arc::clone(p)) }) else {
            eprintln!("fl-ui: Canvas::drawPicture: not a live Picture");
            return;
        };
        // SAFETY: canvas is the receiver's native field.
        unsafe { push(canvas, Draw::Picture(p)) };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_f32(d: &mut [u8; 68], i: usize, v: f32) {
        d[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes())
    }

    fn put_i32(d: &mut [u8; 68], i: usize, v: i32) {
        d[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes())
    }

    #[test]
    fn all_zero_bytes_are_the_default_paint() {
        assert_eq!(Paint::decode(&[0; 68]), Some(Paint::default()));
    }

    #[test]
    fn decodes_what_the_framework_writes() {
        // Color(0xFF2196F3), anti-alias off, stroke 2.5, blend multiply (24),
        // round caps, bevel joins, miter limit 10, a blur of sigma 3, inverted.
        let mut d = [0u8; 68];
        put_i32(&mut d, 0, 1);
        put_f32(&mut d, 1, 0x21 as f32 / 255.0);
        put_f32(&mut d, 2, 0x96 as f32 / 255.0);
        put_f32(&mut d, 3, 0xF3 as f32 / 255.0);
        put_f32(&mut d, 4, 0.0);
        put_i32(&mut d, 6, 24 ^ 3);
        put_i32(&mut d, 7, 1);
        put_f32(&mut d, 8, 2.5);
        put_i32(&mut d, 9, 1);
        put_i32(&mut d, 10, 2);
        put_f32(&mut d, 11, 10.0 - 4.0);
        put_i32(&mut d, 13, 1);
        put_i32(&mut d, 14, 0);
        put_f32(&mut d, 15, 3.0);
        put_i32(&mut d, 16, 1);
        let p = Paint::decode(&d).unwrap();
        assert!(!p.anti_alias);
        assert_eq!(
            p.color,
            [
                0x21 as f32 / 255.0,
                0x96 as f32 / 255.0,
                0xF3 as f32 / 255.0,
                1.0
            ]
        );
        assert_eq!(p.blend_mode, 24);
        assert_eq!(p.style, Style::Stroke);
        assert_eq!(p.stroke_width, 2.5);
        assert_eq!(p.stroke_cap, Cap::Round);
        assert_eq!(p.stroke_join, Join::Bevel);
        assert_eq!(p.stroke_miter_limit, 10.0);
        assert_eq!(p.mask_blur, Some((0, 3.0)));
        assert!(p.invert_colors);
    }

    #[test]
    fn short_data_is_refused() {
        assert_eq!(Paint::decode(&[0; 20]), None);
    }

    #[test]
    fn restores_are_balanced_against_saves() {
        let mut r = Recording::default();
        r.restore();
        assert!(r.draws.is_empty());
        r.save(Draw::Save);
        r.save(Draw::Save);
        r.restore();
        assert_eq!(r.saves, 1);
        assert_eq!(r.draws, [Draw::Save, Draw::Save, Draw::Restore]);
    }
}
