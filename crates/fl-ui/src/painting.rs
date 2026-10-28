//! `PictureRecorder`, `Canvas` and `Picture`.
//!
//! A recorder and the canvas drawing into it share one draw list; ending the
//! recording moves the list into an immutable [`Picture`]. Stage 1 keeps the
//! geometry of each draw; Paint decoding is stage 2.

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use fl_dart_ffi::{guard, peer, sys};

/// One draw call, as recorded.
#[derive(Debug, Clone, PartialEq)]
pub enum Draw {
    Rect { ltrb: [f64; 4], paint: Paint },
}

/// `PaintingStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Fill,
    Stroke,
}

/// The parts of a dart:ui `Paint` that are plain data: its `ByteData`, 68
/// bytes in the layout painting.dart:1328-1365 declares. Shaders, filters
/// and the like travel in the separate objects list and are not decoded yet.
#[derive(Debug, Clone, Copy, PartialEq)]
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
}

impl Default for Paint {
    /// `Paint()`: opaque black, anti-aliased, srcOver, fill.
    fn default() -> Self {
        Paint {
            color: [0.0, 0.0, 0.0, 1.0],
            color_space: 0,
            anti_alias: true,
            blend_mode: BLEND_SRC_OVER,
            style: Style::Fill,
            stroke_width: 0.0,
        }
    }
}

const BLEND_SRC_OVER: i32 = 3;

impl Paint {
    /// Decodes the `ByteData`, little-endian (natives.dart:44). Fields the
    /// framework stores relative to a default are restored here: alpha is
    /// stored as `1 - a`, anti-alias as "0 means on", blend mode XOR srcOver,
    /// so all-zero bytes are `Paint()`. `None` if the data is short.
    pub fn decode(data: &[u8]) -> Option<Paint> {
        let i32_at = |i: usize| -> Option<i32> {
            Some(i32::from_le_bytes(
                data.get(i * 4..i * 4 + 4)?.try_into().ok()?,
            ))
        };
        let f32_at = |i: usize| -> Option<f32> {
            Some(f32::from_le_bytes(
                data.get(i * 4..i * 4 + 4)?.try_into().ok()?,
            ))
        };
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
        })
    }
}

/// A finished recording. Immutable, shared by every layer that adds it.
#[derive(Debug, Default)]
pub struct Picture {
    pub draws: Vec<Draw>,
}

type Draws = Arc<Mutex<Vec<Draw>>>;

/// The `PictureRecorder` peer.
struct Recorder {
    draws: Draws,
}

/// The `Canvas` peer: draws into its recorder's list.
struct CanvasPeer {
    draws: Draws,
}

fn push(draws: &Draws, d: Draw) {
    draws.lock().unwrap_or_else(|e| e.into_inner()).push(d);
}

/// `Void Function(Handle)`.
pub(crate) extern "C" fn recorder_create(wrapper: sys::Dart_Handle) {
    guard((), || {
        let r = Recorder {
            draws: Arc::default(),
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
        let Some(draws) =
            (unsafe { peer::with::<Recorder, _>(recorder, |r| Arc::clone(&r.draws)) })
        else {
            eprintln!("fl-ui: Canvas::Create: not a live PictureRecorder");
            return;
        };
        // SAFETY: as recorder_create.
        if let Err(e) = unsafe { peer::attach(wrapper, CanvasPeer { draws }, 0) } {
            eprintln!("fl-ui: Canvas::Create: {e}");
        }
    });
}

/// `Void Function(Pointer<Void> canvas, Double l, t, r, b, Handle paintObjects, Handle paintData)`.
pub(crate) extern "C" fn canvas_draw_rect(
    canvas: *mut c_void,
    l: f64,
    t: f64,
    r: f64,
    b: f64,
    _paint_objects: sys::Dart_Handle,
    paint_data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: a Handle-parameter native; paint_data is a live handle.
        let paint = unsafe { paint(paint_data) };
        // SAFETY: canvas is the receiver's native field.
        unsafe {
            peer::with::<CanvasPeer, _>(canvas, |c| {
                push(
                    &c.draws,
                    Draw::Rect {
                        ltrb: [l, t, r, b],
                        paint,
                    },
                )
            })
        };
    });
}

/// The Paint a draw native was given. Malformed data draws with `Paint()`
/// and says so, rather than dropping the draw.
///
/// # Safety
/// As [`fl_dart_ffi::args::bytes`].
unsafe fn paint(data: sys::Dart_Handle) -> Paint {
    // SAFETY: per this function's contract.
    let bytes = unsafe { fl_dart_ffi::args::bytes(data) };
    bytes.as_deref().and_then(Paint::decode).unwrap_or_else(|| {
        eprintln!("fl-ui: malformed Paint data; drawing with Paint()");
        Paint::default()
    })
}

/// `Void Function(Pointer<Void> recorder, Handle outPicture)`.
pub(crate) extern "C" fn recorder_end(recorder: *mut c_void, out_picture: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: recorder is the receiver's native field.
        let draws = unsafe {
            peer::with::<Recorder, _>(recorder, |r| {
                std::mem::take(&mut *r.draws.lock().unwrap_or_else(|e| e.into_inner()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_zero_bytes_are_the_default_paint() {
        assert_eq!(Paint::decode(&[0; 68]), Some(Paint::default()));
    }

    #[test]
    fn decodes_what_the_framework_writes() {
        // Color(0xFF2196F3), anti-alias off, stroke 2.5, blend multiply (24).
        let mut d = [0u8; 68];
        let put_f32 = |d: &mut [u8; 68], i: usize, v: f32| {
            d[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes())
        };
        let put_i32 = |d: &mut [u8; 68], i: usize, v: i32| {
            d[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes())
        };
        put_i32(&mut d, 0, 1);
        put_f32(&mut d, 1, 0x21 as f32 / 255.0);
        put_f32(&mut d, 2, 0x96 as f32 / 255.0);
        put_f32(&mut d, 3, 0xF3 as f32 / 255.0);
        put_f32(&mut d, 4, 0.0);
        put_i32(&mut d, 6, 24 ^ 3);
        put_i32(&mut d, 7, 1);
        put_f32(&mut d, 8, 2.5);
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
    }

    #[test]
    fn short_data_is_refused() {
        assert_eq!(Paint::decode(&[0; 20]), None);
    }
}
