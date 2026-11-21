//! `ColorFilter`: dart:ui's mode, matrix and gamma filters, as the engine's
//! `ColorFilter` receives them (color_filter.cc). A paint carries one in its
//! objects list ([`crate::painting::Paint::color_filter`]) and
//! `SceneBuilder.pushColorFilter` a layer's.
//!
//! The natives return nothing, so a filter whose arguments do not decode
//! keeps no filter, and a paint that uses it is refused by name, not drawn
//! unfiltered.

use std::ffi::c_void;
use std::sync::Arc;

use fl_dart_ffi::{args, guard, peer, sys};

/// A color filter, in dart:ui's terms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorFilter {
    /// `ColorFilter.mode`: `color` blended onto what is drawn, which is the
    /// destination. Straight red, green, blue, alpha, sRGB-encoded, 0 to 1;
    /// `blend` a `BlendMode` index.
    Mode { color: [f32; 4], blend: i32 },
    /// `ColorFilter.matrix`: row-major, five wide, the last column already
    /// divided by 255 as the engine divides it, so every entry is in 0 to 1
    /// units.
    Matrix([f32; 20]),
    /// `ColorFilter.linearToSrgbGamma`.
    LinearToSrgbGamma,
    /// `ColorFilter.srgbToLinearGamma`.
    SrgbToLinearGamma,
}

type Peer = Option<Arc<ColorFilter>>;

/// `Void Function(Handle)`: an empty filter, given its kind next.
pub(crate) extern "C" fn create(wrapper: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: a Handle-parameter native: isolate current, scope open.
        if let Err(e) = unsafe { peer::attach(wrapper, Peer::None, 0) } {
            eprintln!("fl-ui: ColorFilter::Create: {e}");
        }
    });
}

/// Sets `f`'s filter, or says it is not a live one.
///
/// # Safety
/// `f` is the receiver's native field.
unsafe fn set(what: &str, f: *mut c_void, filter: Option<ColorFilter>) {
    // SAFETY: per this function's contract.
    if unsafe { peer::with::<Peer, _>(f, |v| *v = filter.map(Arc::new)) }.is_none() {
        eprintln!("fl-ui: ColorFilter::{what}: not a live color filter");
    }
}

/// `Void Function(Pointer<Void>, Int32 color, Int32 blendMode)`, a leaf
/// call: `color` is a `Color`'s 32-bit ARGB value.
pub(crate) extern "C" fn init_mode(f: *mut c_void, color: i32, blend: i32) {
    guard((), || {
        let filter = ColorFilter::Mode {
            color: argb(color as u32),
            blend,
        };
        // SAFETY: f is the receiver's native field.
        unsafe { set("initMode", f, Some(filter)) };
    });
}

/// `Void Function(Pointer<Void>, Handle matrix)`: 20 floats, or none kept.
pub(crate) extern "C" fn init_matrix(f: *mut c_void, matrix: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: a Handle-parameter native: isolate current, scope open.
        let m = unsafe { args::f32s(matrix) }.and_then(|m| matrix_of(&m));
        if m.is_none() {
            eprintln!("fl-ui: ColorFilter::initMatrix: not 20 finite numbers");
        }
        // SAFETY: f is the receiver's native field.
        unsafe { set("initMatrix", f, m) };
    });
}

/// `Void Function(Pointer<Void>)`, a leaf call.
pub(crate) extern "C" fn init_linear_to_srgb_gamma(f: *mut c_void) {
    guard((), || {
        // SAFETY: f is the receiver's native field.
        unsafe {
            set(
                "initLinearToSrgbGamma",
                f,
                Some(ColorFilter::LinearToSrgbGamma),
            )
        };
    });
}

/// `Void Function(Pointer<Void>)`, a leaf call.
pub(crate) extern "C" fn init_srgb_to_linear_gamma(f: *mut c_void) {
    guard((), || {
        // SAFETY: f is the receiver's native field.
        unsafe {
            set(
                "initSrgbToLinearGamma",
                f,
                Some(ColorFilter::SrgbToLinearGamma),
            )
        };
    });
}

/// A 32-bit ARGB color as straight red, green, blue, alpha, 0 to 1.
fn argb(c: u32) -> [f32; 4] {
    let byte = |shift: u32| ((c >> shift) & 0xFF) as f32 / 255.0;
    [byte(16), byte(8), byte(0), byte(24)]
}

/// dart:ui's matrix with its last column in 0 to 1 units, as
/// `ColorFilter::initMatrix` scales it; `None` unless 20 finite numbers.
fn matrix_of(m: &[f32]) -> Option<ColorFilter> {
    let mut m = <[f32; 20]>::try_from(m).ok()?;
    if m.iter().any(|v| !v.is_finite()) {
        return None;
    }
    for i in [4, 9, 14, 19] {
        m[i] /= 255.0;
    }
    Some(ColorFilter::Matrix(m))
}

/// The filter a `_ColorFilter` wrapper holds, by its native field.
///
/// # Safety
/// `f` is a native field this module attached, or null.
pub(crate) unsafe fn of_field(f: *mut c_void) -> Option<Arc<ColorFilter>> {
    // SAFETY: per this function's contract.
    unsafe { peer::with::<Peer, _>(f, |v| v.clone()) }.flatten()
}

/// The filter a paint's objects list holds as its color filter, if that is
/// what it holds.
///
/// # Safety
/// The isolate is current with an API scope open; `filter` is a live handle.
pub(crate) unsafe fn of(filter: sys::Dart_Handle) -> Option<Arc<ColorFilter>> {
    // SAFETY: per this function's contract.
    unsafe {
        let field = peer::field(filter)?;
        of_field(field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mode_color_is_argb_as_straight_rgba() {
        assert_eq!(argb(0x80FF0000), [1.0, 0.0, 0.0, 128.0 / 255.0]);
        assert_eq!(argb(0xFF00FF00), [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn a_matrix_s_last_column_is_scaled_to_one() {
        let mut m = [0.0f32; 20];
        m[0] = 1.0;
        m[4] = 255.0;
        m[19] = 51.0;
        let Some(ColorFilter::Matrix(got)) = matrix_of(&m) else {
            panic!("a 20-entry matrix decodes");
        };
        assert_eq!(got[0], 1.0);
        assert_eq!(got[4], 1.0);
        assert_eq!(got[19], 0.2);
    }

    #[test]
    fn a_matrix_of_the_wrong_length_or_not_finite_is_none() {
        assert_eq!(matrix_of(&[0.0; 19]), None);
        let mut m = [0.0f32; 20];
        m[3] = f32::NAN;
        assert_eq!(matrix_of(&m), None);
    }
}
