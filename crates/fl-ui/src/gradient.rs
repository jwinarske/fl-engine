//! `Gradient`: dart:ui's linear, radial, two-point conical and sweep
//! gradients, as the engine's `Gradient` receives them (gradient.cc).
//!
//! The natives return nothing, so a gradient whose arguments do not decode
//! keeps no gradient, and a paint that uses it is a shader fl-ui does not
//! read: the draw is refused by name, not drawn wrong.

use std::ffi::c_void;
use std::sync::Arc;

use fl_dart_ffi::{args, guard, peer, sys};

/// Which gradient, in the gradient's own space, before its matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientKind {
    Linear {
        start: [f32; 2],
        end: [f32; 2],
    },
    Radial {
        center: [f32; 2],
        radius: f32,
    },
    /// `Gradient.radial` with a focal point: from the start circle to the
    /// end one.
    Conical {
        start: [f32; 2],
        start_radius: f32,
        end: [f32; 2],
        end_radius: f32,
    },
    /// Angles in radians, clockwise on screen from the positive X axis.
    Sweep {
        center: [f32; 2],
        start_angle: f32,
        end_angle: f32,
    },
}

/// A gradient: its kind, its colors at their stops, how it tiles, and the
/// matrix from its space to the canvas's.
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    /// Straight red, green, blue, alpha, extended sRGB, one a stop.
    pub colors: Vec<[f32; 4]>,
    /// Each color's position, 0 to 1, as many as the colors.
    pub stops: Vec<f32>,
    /// A `TileMode` index.
    pub tile: i32,
    /// Column-major; `None` is the identity.
    pub matrix: Option<[f64; 16]>,
}

type Peer = Option<Arc<Gradient>>;

/// `Void Function(Handle)`: an empty gradient, given its kind next.
pub(crate) extern "C" fn create(wrapper: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: a Handle-parameter native: isolate current, scope open.
        if let Err(e) = unsafe { peer::attach(wrapper, Peer::None, 0) } {
            eprintln!("fl-ui: Gradient::Create: {e}");
        }
    });
}

/// The colors, `_encodeWideColorList`'s alpha, red, green, blue a color,
/// as red, green, blue, alpha; the stops, evenly spaced when not given, as
/// Skia spaces them. `None` if they do not decode or do not agree.
///
/// # Safety
/// The isolate is current with an API scope open; the handles are live.
unsafe fn colors_and_stops(
    colors: sys::Dart_Handle,
    stops: sys::Dart_Handle,
) -> Option<(Vec<[f32; 4]>, Vec<f32>)> {
    // SAFETY: per this function's contract.
    let raw = unsafe { args::f32s(colors) }?;
    if raw.is_empty() || raw.len() % 4 != 0 {
        return None;
    }
    let colors: Vec<[f32; 4]> = raw
        .chunks_exact(4)
        .map(|c| [c[1], c[2], c[3], c[0]])
        .collect();
    // SAFETY: per this function's contract.
    let stops = match unsafe { args::f32s(stops) } {
        Some(s) if s.len() == colors.len() => s,
        Some(_) => return None,
        None => {
            let last = (colors.len() - 1).max(1) as f32;
            (0..colors.len()).map(|i| i as f32 / last).collect()
        }
    };
    if colors
        .iter()
        .flatten()
        .chain(&stops)
        .any(|v| !v.is_finite())
    {
        return None;
    }
    Some((colors, stops))
}

/// The matrix argument: `None` for null, else its 16 entries; `Err` for any
/// other length.
///
/// # Safety
/// As [`colors_and_stops`].
unsafe fn matrix_of(matrix: sys::Dart_Handle) -> Result<Option<[f64; 16]>, ()> {
    // SAFETY: per this function's contract.
    match unsafe { args::f64s(matrix) } {
        None => Ok(None),
        Some(m) => <[f64; 16]>::try_from(m).map(Some).map_err(|_| ()),
    }
}

/// Sets `g`'s gradient: `kind` with the decoded colors, stops and matrix,
/// or none, said, if they do not decode.
///
/// # Safety
/// `g` is the receiver's native field; the isolate is current with an API
/// scope open; the handles are live.
unsafe fn init(
    what: &str,
    g: *mut c_void,
    kind: GradientKind,
    colors: sys::Dart_Handle,
    stops: sys::Dart_Handle,
    tile: i32,
    matrix: sys::Dart_Handle,
) {
    // SAFETY: per this function's contract.
    let decoded = unsafe { colors_and_stops(colors, stops).zip(matrix_of(matrix).ok()) };
    let gradient = decoded.map(|((colors, stops), matrix)| {
        Arc::new(Gradient {
            kind,
            colors,
            stops,
            tile,
            matrix,
        })
    });
    if gradient.is_none() {
        eprintln!("fl-ui: Gradient::{what}: colors, stops or matrix do not decode");
    }
    // SAFETY: per this function's contract.
    if unsafe { peer::with::<Peer, _>(g, |v| *v = gradient) }.is_none() {
        eprintln!("fl-ui: Gradient::{what}: not a live gradient");
    }
}

/// `Void Function(Pointer<Void>, Handle endPoints, Handle colors, Handle
/// colorStops, Int32 tileMode, Handle matrix4)`.
pub(crate) extern "C" fn init_linear(
    g: *mut c_void,
    ends: sys::Dart_Handle,
    colors: sys::Dart_Handle,
    stops: sys::Dart_Handle,
    tile: i32,
    matrix: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: a native's arguments: g is the receiver's field, the
        // handles live.
        unsafe {
            let Some([x0, y0, x1, y1]) =
                args::f32s(ends).and_then(|e| <[f32; 4]>::try_from(e).ok())
            else {
                eprintln!("fl-ui: Gradient::initLinear: endPoints must have 4 entries");
                return;
            };
            let kind = GradientKind::Linear {
                start: [x0, y0],
                end: [x1, y1],
            };
            init("initLinear", g, kind, colors, stops, tile, matrix);
        }
    });
}

/// `Void Function(Pointer<Void>, Double, Double, Double, Handle, Handle,
/// Int32, Handle)`.
pub(crate) extern "C" fn init_radial(
    g: *mut c_void,
    cx: f64,
    cy: f64,
    radius: f64,
    colors: sys::Dart_Handle,
    stops: sys::Dart_Handle,
    tile: i32,
    matrix: sys::Dart_Handle,
) {
    guard((), || {
        let kind = GradientKind::Radial {
            center: [cx as f32, cy as f32],
            radius: radius as f32,
        };
        // SAFETY: as init_linear.
        unsafe { init("initRadial", g, kind, colors, stops, tile, matrix) };
    });
}

/// `Void Function(Pointer<Void>, Double x6, Handle, Handle, Int32,
/// Handle)`: start circle, end circle.
pub(crate) extern "C" fn init_conical(
    g: *mut c_void,
    sx: f64,
    sy: f64,
    start_radius: f64,
    ex: f64,
    ey: f64,
    end_radius: f64,
    colors: sys::Dart_Handle,
    stops: sys::Dart_Handle,
    tile: i32,
    matrix: sys::Dart_Handle,
) {
    guard((), || {
        let kind = GradientKind::Conical {
            start: [sx as f32, sy as f32],
            start_radius: start_radius as f32,
            end: [ex as f32, ey as f32],
            end_radius: end_radius as f32,
        };
        // SAFETY: as init_linear.
        unsafe { init("initTwoPointConical", g, kind, colors, stops, tile, matrix) };
    });
}

/// `Void Function(Pointer<Void>, Double, Double, Handle, Handle, Int32,
/// Double, Double, Handle)`.
pub(crate) extern "C" fn init_sweep(
    g: *mut c_void,
    cx: f64,
    cy: f64,
    colors: sys::Dart_Handle,
    stops: sys::Dart_Handle,
    tile: i32,
    start_angle: f64,
    end_angle: f64,
    matrix: sys::Dart_Handle,
) {
    guard((), || {
        let kind = GradientKind::Sweep {
            center: [cx as f32, cy as f32],
            start_angle: start_angle as f32,
            end_angle: end_angle as f32,
        };
        // SAFETY: as init_linear.
        unsafe { init("initSweep", g, kind, colors, stops, tile, matrix) };
    });
}

/// The gradient a paint's objects list holds as its shader, if that is
/// what it holds.
///
/// # Safety
/// The isolate is current with an API scope open; `shader` is a live handle.
pub(crate) unsafe fn gradient_of(shader: sys::Dart_Handle) -> Option<Arc<Gradient>> {
    // SAFETY: per this function's contract.
    unsafe {
        let field = peer::field(shader)?;
        peer::with::<Peer, _>(field, |g| g.clone()).flatten()
    }
}
