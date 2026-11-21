//! `ImageFilter`: dart:ui's blur, dilate, erode, matrix, color and composed
//! filters, as the engine's `ImageFilter` receives them (image_filter.cc). A
//! paint carries one in its objects list
//! ([`crate::painting::Paint::image_filter`]); `SceneBuilder.pushImageFilter`
//! and `pushBackdropFilter` a layer's.
//!
//! The natives return nothing, so a filter whose arguments do not decode
//! keeps no filter, and what uses it is refused by name, not drawn
//! unfiltered.

use std::ffi::c_void;
use std::sync::Arc;

use fl_dart_ffi::{args, guard, peer, sys};

use crate::color_filter::{self, ColorFilter};

/// An image filter, in dart:ui's terms.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageFilter {
    /// `ImageFilter.blur`: deviations in the space the filter is used in.
    /// `tile` is a `TileMode` index, `None` for the default the layer that
    /// uses it chooses (the engine's dynamic tile mode: decal for an image
    /// filter layer, mirror for a backdrop). `bounds`, left, top, right,
    /// bottom, limit what is sampled.
    Blur {
        sigma_x: f32,
        sigma_y: f32,
        tile: Option<i32>,
        bounds: Option<[f32; 4]>,
    },
    /// `ImageFilter.dilate`: radii in the space the filter is used in.
    Dilate { radius_x: f32, radius_y: f32 },
    /// `ImageFilter.erode`.
    Erode { radius_x: f32, radius_y: f32 },
    /// `ImageFilter.matrix`: column-major 4x4 and a `FilterQuality` index.
    Matrix { matrix: [f64; 16], quality: i32 },
    /// `ColorFilter` used as an image filter.
    Color(Arc<ColorFilter>),
    /// `ImageFilter.compose`: `inner` first, then `outer`. `depth` is the
    /// composition's, kept so that finding it does not walk a chain that
    /// shares its parts, which `compose(a, a)` repeated makes exponential.
    Compose {
        outer: Arc<ImageFilter>,
        inner: Arc<ImageFilter>,
        depth: usize,
    },
    /// `ImageFilter.shader`: a fragment program, which fl-ui does not run.
    Shader,
}

type Peer = Option<Arc<ImageFilter>>;

/// The deepest composition kept: dart:ui bounds none, and what reads one
/// walks it recursively. The engine's own `DlComposeImageFilter` is as
/// deep as the program made it; past this the filter keeps none and is
/// refused by name.
const MAX_DEPTH: usize = 16;

impl ImageFilter {
    /// How many filters deep it is: one, or a composition's own count.
    pub fn depth(&self) -> usize {
        match self {
            ImageFilter::Compose { depth, .. } => *depth,
            _ => 1,
        }
    }

    /// `inner` then `outer`, or `None` past [`MAX_DEPTH`].
    fn compose(outer: Arc<ImageFilter>, inner: Arc<ImageFilter>) -> Option<ImageFilter> {
        let depth = 1 + outer.depth().max(inner.depth());
        (depth <= MAX_DEPTH).then_some(ImageFilter::Compose {
            outer,
            inner,
            depth,
        })
    }
}

/// `Void Function(Handle)`: an empty filter, given its kind next.
pub(crate) extern "C" fn create(wrapper: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: a Handle-parameter native: isolate current, scope open.
        if let Err(e) = unsafe { peer::attach(wrapper, Peer::None, 0) } {
            eprintln!("fl-ui: ImageFilter::Create: {e}");
        }
    });
}

/// Sets `f`'s filter, or says it is not a live one.
///
/// # Safety
/// `f` is the receiver's native field.
unsafe fn set(what: &str, f: *mut c_void, filter: Option<ImageFilter>) {
    // SAFETY: per this function's contract.
    if unsafe { peer::with::<Peer, _>(f, |v| *v = filter.map(Arc::new)) }.is_none() {
        eprintln!("fl-ui: ImageFilter::{what}: not a live image filter");
    }
}

fn finite(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}

/// `Void Function(Pointer<Void>, Double sigmaX, Double sigmaY, Int32
/// tileMode, Bool bounded, Double left, Double top, Double right, Double
/// bottom)`, a leaf call. A negative tile mode is the default.
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn init_blur(
    f: *mut c_void,
    sigma_x: f64,
    sigma_y: f64,
    tile: i32,
    bounded: bool,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
) {
    guard((), || {
        let filter =
            finite(&[sigma_x, sigma_y, left, top, right, bottom]).then(|| ImageFilter::Blur {
                sigma_x: sigma_x as f32,
                sigma_y: sigma_y as f32,
                tile: (tile >= 0).then_some(tile),
                bounds: bounded.then_some([left as f32, top as f32, right as f32, bottom as f32]),
            });
        if filter.is_none() {
            eprintln!("fl-ui: ImageFilter::initBlur: not finite");
        }
        // SAFETY: f is the receiver's native field.
        unsafe { set("initBlur", f, filter) };
    });
}

/// `Void Function(Pointer<Void>, Double radiusX, Double radiusY)`, a leaf
/// call.
pub(crate) extern "C" fn init_dilate(f: *mut c_void, radius_x: f64, radius_y: f64) {
    guard((), || {
        let filter = finite(&[radius_x, radius_y]).then_some(ImageFilter::Dilate {
            radius_x: radius_x as f32,
            radius_y: radius_y as f32,
        });
        // SAFETY: f is the receiver's native field.
        unsafe { set("initDilate", f, filter) };
    });
}

/// `Void Function(Pointer<Void>, Double radiusX, Double radiusY)`, a leaf
/// call.
pub(crate) extern "C" fn init_erode(f: *mut c_void, radius_x: f64, radius_y: f64) {
    guard((), || {
        let filter = finite(&[radius_x, radius_y]).then_some(ImageFilter::Erode {
            radius_x: radius_x as f32,
            radius_y: radius_y as f32,
        });
        // SAFETY: f is the receiver's native field.
        unsafe { set("initErode", f, filter) };
    });
}

/// `Void Function(Pointer<Void>, Handle matrix4, Int32 filterQuality)`.
pub(crate) extern "C" fn init_matrix(f: *mut c_void, matrix: sys::Dart_Handle, quality: i32) {
    guard((), || {
        // SAFETY: a Handle-parameter native: isolate current, scope open.
        let filter = unsafe { args::f64s(matrix) }
            .and_then(|m| <[f64; 16]>::try_from(m).ok())
            .filter(|m| finite(m))
            .map(|matrix| ImageFilter::Matrix { matrix, quality });
        if filter.is_none() {
            eprintln!("fl-ui: ImageFilter::initMatrix: not 16 finite numbers");
        }
        // SAFETY: f is the receiver's native field.
        unsafe { set("initMatrix", f, filter) };
    });
}

/// `Void Function(Pointer<Void>, Pointer<Void> colorFilter)`.
pub(crate) extern "C" fn init_color_filter(f: *mut c_void, color: *mut c_void) {
    guard((), || {
        // SAFETY: color is the argument's native field, or null.
        let filter = unsafe { color_filter::of_field(color) }.map(ImageFilter::Color);
        if filter.is_none() {
            eprintln!("fl-ui: ImageFilter::initColorFilter: not a live color filter");
        }
        // SAFETY: f is the receiver's native field.
        unsafe { set("initColorFilter", f, filter) };
    });
}

/// `Void Function(Pointer<Void>, Pointer<Void> outer, Pointer<Void> inner)`.
pub(crate) extern "C" fn init_compose(f: *mut c_void, outer: *mut c_void, inner: *mut c_void) {
    guard((), || {
        // SAFETY: outer and inner are the arguments' native fields.
        let filter = unsafe { of_field(outer).zip(of_field(inner)) }
            .and_then(|(outer, inner)| ImageFilter::compose(outer, inner));
        if filter.is_none() {
            eprintln!(
                "fl-ui: ImageFilter::initComposeFilter: not two live filters, or past {MAX_DEPTH} deep"
            );
        }
        // SAFETY: f is the receiver's native field.
        unsafe { set("initComposeFilter", f, filter) };
    });
}

/// `Void Function(Pointer<Void>, Pointer<Void> shader)`: kept as what it
/// is, for the raster side to refuse by name.
pub(crate) extern "C" fn init_shader(f: *mut c_void, _shader: *mut c_void) {
    guard((), || {
        // SAFETY: f is the receiver's native field.
        unsafe { set("initShader", f, Some(ImageFilter::Shader)) };
    });
}

/// The filter an `_ImageFilter` wrapper holds, by its native field.
///
/// # Safety
/// `f` is a native field this module attached, or null.
pub(crate) unsafe fn of_field(f: *mut c_void) -> Option<Arc<ImageFilter>> {
    // SAFETY: per this function's contract.
    unsafe { peer::with::<Peer, _>(f, |v| v.clone()) }.flatten()
}

/// The filter a paint's objects list holds as its image filter, if that is
/// what it holds.
///
/// # Safety
/// The isolate is current with an API scope open; `filter` is a live handle.
pub(crate) unsafe fn of(filter: sys::Dart_Handle) -> Option<Arc<ImageFilter>> {
    // SAFETY: per this function's contract.
    unsafe {
        let field = peer::field(filter)?;
        of_field(field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dilate() -> Arc<ImageFilter> {
        Arc::new(ImageFilter::Dilate {
            radius_x: 1.0,
            radius_y: 1.0,
        })
    }

    #[test]
    fn a_composition_s_depth_is_its_deepest_side() {
        let one = Arc::new(ImageFilter::compose(dilate(), dilate()).unwrap());
        let two = ImageFilter::compose(dilate(), one).unwrap();
        assert_eq!(two.depth(), 3);
    }

    #[test]
    fn a_composition_past_the_limit_keeps_none() {
        // Each step composes the last with itself: a chain that shares its
        // parts, whose depth is found without walking it.
        let mut f = dilate();
        for _ in 1..MAX_DEPTH {
            f = Arc::new(ImageFilter::compose(Arc::clone(&f), Arc::clone(&f)).unwrap());
        }
        assert_eq!(f.depth(), MAX_DEPTH);
        assert_eq!(ImageFilter::compose(Arc::clone(&f), f), None);
    }
}
