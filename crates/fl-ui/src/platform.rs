//! `PlatformConfigurationNativeApi`: the dispatcher's way out to the engine.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use fl_dart_ffi::{args, guard, peer, sys};

use crate::compositing::Scene;

/// One `FlutterView.render`: a scene for a view, at a size in physical
/// pixels.
#[derive(Debug, Clone)]
pub struct Frame {
    pub view_id: i64,
    pub width: f64,
    pub height: f64,
    pub scene: Arc<Scene>,
}

type FrameSink = Box<dyn FnMut(Frame) + Send>;

/// Where [`render`] sends frames. One per process: the VM is (fl-dart-vm's
/// `Vm`), and so is the engine that owns its root isolate.
static FRAME_SINK: Mutex<Option<FrameSink>> = Mutex::new(None);

/// Sets where rendered frames go. The sink runs on the UI thread, inside the
/// `Render` native, and must not call this again (the slot is locked).
pub fn set_frame_sink(sink: impl FnMut(Frame) + Send + 'static) {
    *FRAME_SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(sink));
}

/// `Int64 Function()`. The framework caches it (`RootIsolateToken.instance`)
/// and takes 0 to mean a background isolate, whose messenger then throws.
pub(crate) extern "C" fn get_root_isolate_token() -> i64 {
    static NEXT: AtomicI64 = AtomicI64::new(1);
    guard(1, || NEXT.fetch_add(1, Ordering::Relaxed))
}

/// `Void Function(Handle name, Bool listening)`.
pub(crate) extern "C" fn send_channel_update(_name: sys::Dart_Handle, _listening: bool) {}

/// `Handle Function(Handle name, Handle callback, Handle data)`. Null is
/// success; a returned String is thrown as an error. The message is
/// dropped: nothing on the first frame's path awaits a reply, and an
/// unanswered request never completes rather than failing.
pub(crate) extern "C" fn send_platform_message(
    _name: sys::Dart_Handle,
    _callback: sys::Dart_Handle,
    _data: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a native with Handle parameters runs with an isolate current.
    unsafe { args::null() }
}

/// `Void Function(Bool)`.
pub(crate) extern "C" fn set_semantics_tree_enabled(_enabled: bool) {}

/// `Void Function()`. Frames after the warm-up frame need a vsync source
/// behind this (stage 4).
pub(crate) extern "C" fn schedule_frame() {}

/// `Void Function(Bool)`.
pub(crate) extern "C" fn set_needs_report_timings(_needed: bool) {}

/// `Void Function()`.
pub(crate) extern "C" fn end_warm_up_frame() {}

/// `Void Function(Int64 viewId, Pointer<Void> scene, Double width, Double height)`.
pub(crate) extern "C" fn render(
    view_id: i64,
    scene: *mut std::ffi::c_void,
    width: f64,
    height: f64,
) {
    guard((), || {
        // SAFETY: scene is the Scene argument's native field.
        let Some(scene) = (unsafe { peer::with::<Arc<Scene>, _>(scene, |s| Arc::clone(s)) }) else {
            eprintln!("fl-ui: render: not a live Scene");
            return;
        };
        let frame = Frame {
            view_id,
            width,
            height,
            scene,
        };
        if let Some(sink) = FRAME_SINK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            sink(frame);
        }
    });
}
