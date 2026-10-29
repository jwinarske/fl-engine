//! `ParagraphBuilder`, `Paragraph` and `FontCollection`: stubs until M3's
//! stage 4 (brief M3 §12).
//!
//! Every paragraph lays out as empty: no lines, zero width and height,
//! nothing painted. That is enough for a framework app to lay out and
//! render without a Dart error, its text drawn as nothing, which is what
//! stage 1 asks. Each native is listed in [`crate::STUBS`] with what it
//! returns; stage 4 replaces them with fl-text.

use std::ffi::c_void;

use fl_dart_ffi::{args, guard, peer, sys};

use crate::platform::{queue_reply, ReplyArg};

/// The `ParagraphBuilder` peer: nothing kept yet.
struct Builder;

/// The `Paragraph` peer: nothing laid out yet.
struct Paragraph;

/// Attaches `value` to `wrapper`, saying so on failure.
///
/// # Safety
/// A Handle-parameter native: isolate current, scope open.
unsafe fn attach<T: Send + 'static>(wrapper: sys::Dart_Handle, value: T, what: &str) {
    // SAFETY: per this function's contract.
    if let Err(e) = unsafe { peer::attach(wrapper, value, 0) } {
        eprintln!("fl-ui: {what}: {e}");
    }
}

/// `Void Function(Handle this, Handle encoded, Handle strutData, Handle
/// fontFamily, Handle strutFontFamily, Double fontSize, Double height,
/// Handle ellipsis, Handle locale)`.
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn builder_create(
    wrapper: sys::Dart_Handle,
    _encoded: sys::Dart_Handle,
    _strut: sys::Dart_Handle,
    _family: sys::Dart_Handle,
    _strut_family: sys::Dart_Handle,
    _size: f64,
    _height: f64,
    _ellipsis: sys::Dart_Handle,
    _locale: sys::Dart_Handle,
) {
    // SAFETY: a Handle-parameter native.
    guard((), || unsafe {
        attach(wrapper, Builder, "ParagraphBuilder::Create")
    });
}

/// `Void Function(Pointer<Void>, Handle encoded, Handle fontFamilies,
/// Double fontSize, Double letterSpacing, Double wordSpacing, Double height,
/// Double decorationThickness, Handle locale, Handle backgroundObjects,
/// Handle backgroundData, Handle foregroundObjects, Handle foregroundData,
/// Handle shadowsData, Handle fontFeaturesData, Handle fontVariationsData)`.
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn push_style(
    _builder: *mut c_void,
    _encoded: sys::Dart_Handle,
    _families: sys::Dart_Handle,
    _size: f64,
    _letter: f64,
    _word: f64,
    _height: f64,
    _thickness: f64,
    _locale: sys::Dart_Handle,
    _bg_objects: sys::Dart_Handle,
    _bg_data: sys::Dart_Handle,
    _fg_objects: sys::Dart_Handle,
    _fg_data: sys::Dart_Handle,
    _shadows: sys::Dart_Handle,
    _features: sys::Dart_Handle,
    _variations: sys::Dart_Handle,
) {
}

/// `Void Function(Pointer<Void>)`, a leaf call.
pub(crate) extern "C" fn builder_pop(_builder: *mut c_void) {}

/// `Handle Function(Pointer<Void>, Handle text)`: null, no error.
pub(crate) extern "C" fn add_text(
    _builder: *mut c_void,
    _text: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    unsafe { args::null() }
}

/// `Void Function(Pointer<Void>, Double width, Double height, Uint32
/// alignment, Double baselineOffset, Uint32 baseline)`.
pub(crate) extern "C" fn add_placeholder(
    _builder: *mut c_void,
    _width: f64,
    _height: f64,
    _alignment: u32,
    _offset: f64,
    _baseline: u32,
) {
}

/// `Void Function(Pointer<Void>, Handle outParagraph)`.
pub(crate) extern "C" fn build(_builder: *mut c_void, out: sys::Dart_Handle) {
    // SAFETY: a Handle-parameter native.
    guard((), || unsafe {
        attach(out, Paragraph, "ParagraphBuilder::build")
    });
}

/// Paragraph getters that answer a number, all leaf calls: 0, an empty
/// paragraph's.
macro_rules! zero {
    ($($name:ident -> $ty:ty),* $(,)?) => {
        $(pub(crate) extern "C" fn $name(_p: *mut c_void) -> $ty {
            <$ty>::default()
        })*
    };
}

zero!(
    width -> f64,
    height -> f64,
    longest_line -> f64,
    min_intrinsic_width -> f64,
    max_intrinsic_width -> f64,
    alphabetic_baseline -> f64,
    ideographic_baseline -> f64,
    did_exceed_max_lines -> bool,
    number_of_lines -> u32,
);

/// `Void Function(Pointer<Void>, Double width)`, a leaf call.
pub(crate) extern "C" fn layout(_p: *mut c_void, _width: f64) {}

/// `Int32 Function(Pointer<Void>, Uint32 offset)`: -1, no line.
pub(crate) extern "C" fn line_number_at(_p: *mut c_void, _offset: u32) -> i32 {
    -1
}

/// `Handle Function(Pointer<Void>, Uint32 start, Uint32 end, Uint32
/// heightStyle, Uint32 widthStyle)`: no boxes.
pub(crate) extern "C" fn rects_for_range(
    _p: *mut c_void,
    _start: u32,
    _end: u32,
    _h: u32,
    _w: u32,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    unsafe { args::new_f32s(&[]) }
}

/// `Handle Function(Pointer<Void>)`: no boxes.
pub(crate) extern "C" fn rects_for_placeholders(_p: *mut c_void) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    unsafe { args::new_f32s(&[]) }
}

/// `Handle Function(Pointer<Void>, Double dx, Double dy)`: `[0, 0]`, the
/// start, downstream.
pub(crate) extern "C" fn position_for_offset(
    _p: *mut c_void,
    _dx: f64,
    _dy: f64,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    unsafe { args::new_i64s(&[0, 0]) }
}

/// `Handle Function(Pointer<Void>, Uint32 offset)`: the empty range at 0.
pub(crate) extern "C" fn word_boundary(_p: *mut c_void, _offset: u32) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    unsafe { args::new_i64s(&[0, 0]) }
}

/// `Handle Function(Pointer<Void>, Uint32 offset)`: the empty range at 0.
pub(crate) extern "C" fn line_boundary(_p: *mut c_void, _offset: u32) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    unsafe { args::new_i64s(&[0, 0]) }
}

/// `Handle Function(Pointer<Void>, Uint32 offset, Handle constructor)`:
/// null, no glyph.
pub(crate) extern "C" fn glyph_info_at(
    _p: *mut c_void,
    _offset: u32,
    _ctor: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    unsafe { args::null() }
}

/// `Handle Function(Pointer<Void>, Double dx, Double dy, Handle
/// constructor)`: null, no glyph.
pub(crate) extern "C" fn closest_glyph_info(
    _p: *mut c_void,
    _dx: f64,
    _dy: f64,
    _ctor: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    unsafe { args::null() }
}

/// `Handle Function(Pointer<Void>)`: no lines.
pub(crate) extern "C" fn compute_line_metrics(_p: *mut c_void) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    unsafe { args::new_f64s(&[]) }
}

/// `Handle Function(Pointer<Void>, Uint32 line, Handle constructor)`:
/// null, no such line.
pub(crate) extern "C" fn line_metrics_at(
    _p: *mut c_void,
    _line: u32,
    _ctor: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    unsafe { args::null() }
}

/// `Void Function(Pointer<Void>, Pointer<Void> canvas, Double x, Double y)`:
/// nothing to draw.
pub(crate) extern "C" fn paint(_p: *mut c_void, _canvas: *mut c_void, _x: f64, _y: f64) {}

/// `Void Function(Pointer<Void>)`.
pub(crate) extern "C" fn paragraph_dispose(p: *mut c_void) {
    guard((), || {
        // SAFETY: p is the receiver's native field.
        let _ = unsafe { peer::take::<Paragraph>(p) };
    });
}

/// `Void Function(Handle Uint8List list, Handle callback, Handle family)`:
/// the callback answered with 0, as the engine answers it
/// (font_collection.cc), the font not yet registered. `FontLoader`
/// completes; text in that family still draws nothing.
pub(crate) extern "C" fn load_font_from_list(
    _list: sys::Dart_Handle,
    callback: sys::Dart_Handle,
    _family: sys::Dart_Handle,
) {
    // SAFETY: a Handle-parameter native; callback is a live handle.
    guard((), || unsafe { queue_reply(callback, ReplyArg::Int(0)) });
}
