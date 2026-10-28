//! `SceneBuilder`, `EngineLayer` and `Scene`.
//!
//! The builder records the framework's layer operations in order; `build`
//! freezes them into an immutable [`Scene`], which `render` hands on. Stage 1
//! keeps the operations as given; flattening them into an emblema canvas is
//! stage 3.

use std::ffi::c_void;
use std::sync::Arc;

use fl_dart_ffi::{guard, peer, sys};

use crate::painting::Picture;

/// One SceneBuilder call.
#[derive(Debug, Clone)]
pub enum LayerOp {
    /// A column-major 4x4, as `Float64List` carries it.
    PushTransform([f64; 16]),
    PushOffset {
        dx: f64,
        dy: f64,
    },
    AddPicture {
        dx: f64,
        dy: f64,
        picture: Arc<Picture>,
    },
    Pop,
}

/// A built scene: the layer operations, balanced.
#[derive(Debug, Default)]
pub struct Scene {
    pub ops: Vec<LayerOp>,
}

/// The `SceneBuilder` peer.
#[derive(Default)]
struct Builder {
    ops: Vec<LayerOp>,
}

/// The `EngineLayer` peer. The framework keeps layers to pass back as
/// `oldLayer` for retained rendering; nothing is retained yet.
struct EngineLayer;

/// `Void Function(Handle)`.
pub(crate) extern "C" fn builder_create(wrapper: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: a Handle-parameter native: isolate current, scope open.
        if let Err(e) = unsafe { peer::attach(wrapper, Builder::default(), 0) } {
            eprintln!("fl-ui: SceneBuilder::Create: {e}");
        }
    });
}

/// Attaches an `EngineLayer` peer to the framework's out-object.
///
/// # Safety
/// As [`peer::attach`].
unsafe fn new_layer(out_layer: sys::Dart_Handle, what: &str) {
    // SAFETY: per this function's contract.
    if let Err(e) = unsafe { peer::attach(out_layer, EngineLayer, 0) } {
        eprintln!("fl-ui: {what}: {e}");
    }
}

/// `Void Function(Pointer<Void>, Handle outLayer, Handle Float64List matrix4, Handle oldLayer)`.
pub(crate) extern "C" fn push_transform(
    builder: *mut c_void,
    out_layer: sys::Dart_Handle,
    matrix4: sys::Dart_Handle,
    _old_layer: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: a Handle-parameter native, matrix4 a live handle.
        let m = unsafe { float64x16(matrix4) }.unwrap_or(IDENTITY);
        // SAFETY: builder is the receiver's native field.
        unsafe { peer::with::<Builder, _>(builder, |b| b.ops.push(LayerOp::PushTransform(m))) };
        // SAFETY: as above.
        unsafe { new_layer(out_layer, "SceneBuilder::pushTransform") };
    });
}

/// `Void Function(Pointer<Void>, Handle outLayer, Double dx, Double dy, Handle oldLayer)`.
pub(crate) extern "C" fn push_offset(
    builder: *mut c_void,
    out_layer: sys::Dart_Handle,
    dx: f64,
    dy: f64,
    _old_layer: sys::Dart_Handle,
) {
    guard((), || {
        // SAFETY: builder is the receiver's native field.
        unsafe {
            peer::with::<Builder, _>(builder, |b| b.ops.push(LayerOp::PushOffset { dx, dy }))
        };
        // SAFETY: a Handle-parameter native.
        unsafe { new_layer(out_layer, "SceneBuilder::pushOffset") };
    });
}

/// `Void Function(Pointer<Void>, Double dx, Double dy, Pointer<Void> picture, Int32 hints)`.
pub(crate) extern "C" fn add_picture(
    builder: *mut c_void,
    dx: f64,
    dy: f64,
    picture: *mut c_void,
    _hints: i32,
) {
    guard((), || {
        // SAFETY: picture is the Picture argument's native field.
        let Some(picture) = (unsafe { peer::with::<Arc<Picture>, _>(picture, |p| Arc::clone(p)) })
        else {
            eprintln!("fl-ui: SceneBuilder::addPicture: not a live Picture");
            return;
        };
        // SAFETY: builder is the receiver's native field.
        unsafe {
            peer::with::<Builder, _>(builder, |b| {
                b.ops.push(LayerOp::AddPicture { dx, dy, picture })
            })
        };
    });
}

/// `Void Function(Pointer<Void>)`, a leaf call.
pub(crate) extern "C" fn pop(builder: *mut c_void) {
    guard((), || {
        // SAFETY: builder is the receiver's native field.
        unsafe { peer::with::<Builder, _>(builder, |b| b.ops.push(LayerOp::Pop)) };
    });
}

/// `Void Function(Pointer<Void>, Handle outScene)`.
pub(crate) extern "C" fn build(builder: *mut c_void, out_scene: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: builder is the receiver's native field.
        let ops = unsafe { peer::with::<Builder, _>(builder, |b| std::mem::take(&mut b.ops)) }
            .unwrap_or_default();
        // SAFETY: a Handle-parameter native.
        if let Err(e) = unsafe { peer::attach(out_scene, Arc::new(Scene { ops }), 0) } {
            eprintln!("fl-ui: SceneBuilder::build: {e}");
        }
    });
}

/// `Void Function(Pointer<Void>)`.
pub(crate) extern "C" fn scene_dispose(scene: *mut c_void) {
    guard((), || {
        // SAFETY: scene is the receiver's native field.
        drop(unsafe { peer::take::<Arc<Scene>>(scene) });
    });
}

/// `Void Function(Pointer<Void>)`.
pub(crate) extern "C" fn layer_dispose(layer: *mut c_void) {
    guard((), || {
        // SAFETY: layer is the receiver's native field.
        let _ = unsafe { peer::take::<EngineLayer>(layer) };
    });
}

const IDENTITY: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// The 16 doubles of a `Float64List` of length 16.
///
/// # Safety
/// The isolate is current with a scope open; `h` is a live handle.
unsafe fn float64x16(h: sys::Dart_Handle) -> Option<[f64; 16]> {
    let mut ty = sys::Dart_TypedData_Type::Dart_TypedData_kFloat64;
    let mut data: *mut c_void = std::ptr::null_mut();
    let mut len: isize = 0;
    // SAFETY: per this function's contract; between acquire and release no
    // other API call is made.
    unsafe {
        if sys::Dart_IsError(sys::Dart_TypedDataAcquireData(
            h, &mut ty, &mut data, &mut len,
        )) {
            return None;
        }
        let m = (ty == sys::Dart_TypedData_Type::Dart_TypedData_kFloat64
            && len == 16
            && !data.is_null())
        .then(|| {
            let mut m = [0.0; 16];
            std::ptr::copy_nonoverlapping(data as *const f64, m.as_mut_ptr(), 16);
            m
        });
        let _ = sys::Dart_TypedDataReleaseData(h);
        m
    }
}
