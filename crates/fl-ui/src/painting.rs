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
    Rect { ltrb: [f64; 4] },
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
    _paint_data: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: canvas is the receiver's native field.
        unsafe {
            peer::with::<CanvasPeer, _>(canvas, |c| {
                push(&c.draws, Draw::Rect { ltrb: [l, t, r, b] })
            })
        };
    });
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
