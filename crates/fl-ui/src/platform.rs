//! `PlatformConfigurationNativeApi`: the dispatcher's way out to the engine.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use fl_dart_ffi::{args, guard, peer, sys};

use crate::compositing::Scene;
use crate::image::ImageData;

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

type FrameScheduler = Box<dyn Fn() + Send>;

/// What [`schedule_frame`] calls: the shell's vsync request.
static FRAME_SCHEDULER: Mutex<Option<FrameScheduler>> = Mutex::new(None);

/// View 0's device pixel ratio, as `f64` bits: what `drawShadow` scales its
/// elevation by.
static DPR: AtomicU64 = AtomicU64::new(0x3FF0_0000_0000_0000);

/// Sets the device pixel ratio `drawShadow` reads, the implicit view's, as
/// the engine reads it from view 0's metrics. 1 until set.
pub fn set_device_pixel_ratio(dpr: f64) {
    let dpr = if dpr.is_finite() && dpr > 0.0 {
        dpr
    } else {
        1.0
    };
    DPR.store(dpr.to_bits(), Ordering::Relaxed);
}

pub(crate) fn device_pixel_ratio() -> f64 {
    f64::from_bits(DPR.load(Ordering::Relaxed))
}

/// Sets what `PlatformDispatcher.scheduleFrame` asks for a frame through.
/// It runs on the UI thread, inside the `ScheduleFrame` native, and must not
/// call this again (the slot is locked).
pub fn set_frame_scheduler(schedule: impl Fn() + Send + 'static) {
    *FRAME_SCHEDULER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(schedule));
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
/// success; a returned String is thrown as an error.
///
/// Two channels are answered:
/// * `flutter/assets`: the data is an asset key as UTF-8; the answer is the
///   asset's bytes from the bundle [`crate::set_asset_bundle`] set, or null,
///   which the framework reads as not found. Every request is answered,
///   found or not (brief M3 §15)
/// * `flutter/restoration` `get`, which `MaterialApp` awaits before its
///   first frame when it has a restoration scope. The answer is a successful
///   method result of null, no restoration data; an empty reply would mean
///   no handler, and the framework would throw
///
/// Every other message is dropped: nothing on the path so far awaits one,
/// and an unanswered request never completes rather than failing.
///
/// Replies are asynchronous, as the engine's are: queued here and delivered
/// by [`deliver_replies`], which the shell's loop calls between steps.
pub(crate) extern "C" fn send_platform_message(
    name: sys::Dart_Handle,
    callback: sys::Dart_Handle,
    data: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a native with Handle parameters runs with an isolate current
    // and a scope open. Made first so a panic below still returns a handle.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: as above; the three are live handles.
        let (name, bytes) = unsafe { (args::string(name), args::bytes(data)) };
        let reply = match name.as_deref() {
            Some("flutter/assets") => Some(
                bytes
                    .as_deref()
                    .and_then(|k| std::str::from_utf8(k).ok())
                    .and_then(crate::buffer::load_asset)
                    .map_or(ReplyArg::Null, ReplyArg::Bytes),
            ),
            Some("flutter/restoration")
                if bytes.as_deref().and_then(method_name) == Some("get") =>
            {
                Some(ReplyArg::Bytes(RESULT_NULL.to_vec()))
            }
            _ => None,
        };
        if let Some(reply) = reply {
            // SAFETY: as above; callback is a live handle.
            unsafe { queue_reply(callback, reply) };
        }
        null
    })
}

/// A `StandardMethodCodec` success envelope holding null: the result byte 0,
/// then `StandardMessageCodec`'s null.
const RESULT_NULL: [u8; 2] = [0, 0];

/// The method name of a `StandardMethodCodec` method call: its first value,
/// a string (type 7, a one-byte length below 254, UTF-8). `None` for
/// anything else.
fn method_name(call: &[u8]) -> Option<&str> {
    let (&[7, len], rest) = call.split_first_chunk::<2>()? else {
        return None;
    };
    if len >= 254 {
        return None;
    }
    std::str::from_utf8(rest.get(..usize::from(len))?).ok()
}

/// What a reply calls its callback with.
pub(crate) enum ReplyArg {
    Null,
    /// The `void` type object: what the engine passes a `_Callback<void>`
    /// to mean success (`Dart_TypeVoid`), since `_futurize` reads null as
    /// failure.
    Void,
    /// A new `ByteData`.
    Bytes(Vec<u8>),
    Int(i64),
    /// `getNextFrame`'s three: a new `_Image`, a duration of 0 ms, and "".
    Frame(Arc<ImageData>),
    /// `getNextFrame`'s three on failure: null, 0, and why.
    FrameError(String),
}

/// A persistent handle to a callback, kept from the native that was given
/// it until its reply is delivered.
pub(crate) struct Persistent(sys::Dart_PersistentHandle);

impl Persistent {
    /// # Safety
    /// The isolate is current with an API scope open; `h` is a live handle.
    pub(crate) unsafe fn new(h: sys::Dart_Handle) -> Persistent {
        // SAFETY: per this function's contract; deleted by deliver_replies,
        // or with the isolate.
        Persistent(unsafe { sys::Dart_NewPersistentHandle(h) })
    }
}

// SAFETY: a Persistent is made on the UI thread with the isolate current,
// and only dereferenced by deliver_replies, whose contract is the same
// thread and isolate. Moving the pointer between is never touching it.
unsafe impl Send for Persistent {}

/// A reply waiting to be delivered: the response callback, kept alive, and
/// what to call it with.
struct Reply {
    callback: Persistent,
    arg: ReplyArg,
}

/// Queues `arg` for `callback`, to be delivered between loop steps. A null
/// callback is nothing to answer.
///
/// # Safety
/// A Handle-parameter native: isolate current, scope open; `callback` is a
/// live handle.
pub(crate) unsafe fn queue_reply(callback: sys::Dart_Handle, arg: ReplyArg) {
    // SAFETY: per this function's contract.
    unsafe {
        if sys::Dart_IsNull(callback) {
            return;
        }
        queue_persistent(Persistent::new(callback), arg);
    }
}

/// Queues `arg` for a callback already kept.
pub(crate) fn queue_persistent(callback: Persistent, arg: ReplyArg) {
    REPLIES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(Reply { callback, arg });
}

static REPLIES: Mutex<Vec<Reply>> = Mutex::new(Vec::new());

static REPLY_WAKER: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

/// Sets what wakes the UI loop when a reply arrives from another thread: a
/// finished decode. Replies queued on the UI thread need no waking.
pub fn set_reply_waker(wake: impl Fn() + Send + 'static) {
    *REPLY_WAKER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(wake));
}

/// Wakes the UI loop for a reply, from any thread.
pub(crate) fn wake_for_replies() {
    if let Some(wake) = REPLY_WAKER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        wake();
    }
}

/// Whether a reply is waiting for [`deliver_replies`]: a platform message's
/// or a finished decode's.
pub fn has_pending_replies() -> bool {
    !REPLIES.lock().unwrap_or_else(|e| e.into_inner()).is_empty() || crate::image::has_done()
}

/// Calls each waiting reply's response callback with its data, in the order
/// the messages were sent. The microtasks that completes are the caller's
/// to run. An exception a callback leaves uncaught is the error; the replies
/// after it are still delivered.
///
/// # Safety
/// The isolate that sent the messages is current on this thread, with an API
/// scope open.
pub unsafe fn deliver_replies() -> Result<(), String> {
    crate::image::take_done();
    let replies = std::mem::take(&mut *REPLIES.lock().unwrap_or_else(|e| e.into_inner()));
    let mut first_error = None;
    for r in replies {
        // SAFETY: per this function's contract; the handle was made by
        // Persistent::new in this isolate and is deleted only here.
        unsafe {
            let callback = sys::Dart_HandleFromPersistent(r.callback.0);
            let mut args3 = [
                args::null(),
                sys::Dart_NewInteger(0),
                sys::Dart_EmptyString(),
            ];
            let one = match &r.arg {
                ReplyArg::Bytes(d) => Some(args::new_byte_data(d)),
                ReplyArg::Int(n) => Some(sys::Dart_NewInteger(*n)),
                ReplyArg::Null => Some(args::null()),
                ReplyArg::Void => Some(sys::Dart_TypeVoid()),
                ReplyArg::Frame(image) => {
                    match crate::image::new_image(image) {
                        Ok(h) => args3[0] = h,
                        Err(e) => args3[2] = error_string(&e),
                    }
                    None
                }
                ReplyArg::FrameError(e) => {
                    args3[2] = error_string(e);
                    None
                }
            };
            let result = match one {
                Some(a) => {
                    let mut a = [a];
                    sys::Dart_InvokeClosure(callback, 1, a.as_mut_ptr())
                }
                None => sys::Dart_InvokeClosure(callback, 3, args3.as_mut_ptr()),
            };
            sys::Dart_DeletePersistentHandle(r.callback.0);
            if sys::Dart_IsError(result) && first_error.is_none() {
                let msg = sys::Dart_GetError(result);
                first_error = Some(if msg.is_null() {
                    "a platform message reply callback failed".to_owned()
                } else {
                    std::ffi::CStr::from_ptr(msg).to_string_lossy().into_owned()
                });
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// A Dart String of `s`.
///
/// # Safety
/// The isolate is current with an API scope open.
unsafe fn error_string(s: &str) -> sys::Dart_Handle {
    let s = std::ffi::CString::new(s).unwrap_or_default();
    // SAFETY: per this function's contract.
    unsafe { sys::Dart_NewStringFromCString(s.as_ptr()) }
}

/// Drops waiting replies without delivering them, for when their isolate is
/// gone: the VM freed their handles with it. Decodes for it are dropped
/// too, queued or finishing.
pub fn discard_replies() {
    REPLIES.lock().unwrap_or_else(|e| e.into_inner()).clear();
    crate::image::end_generation();
}

/// `Handle Function()`: the route the app starts at. `/`, the engine's
/// default when the embedder names none (`Settings::route`).
pub(crate) extern "C" fn default_route_name() -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    unsafe { sys::Dart_NewStringFromCString(c"/".as_ptr()) }
}

/// `Void Function(Handle locale)`: the locale the app resolved, for the
/// platform's use. No platform here wants it.
pub(crate) extern "C" fn set_application_locale(_locale: sys::Dart_Handle) {}

/// `Void Function(Int64 viewId, Int64 state, Int64 direction)`: the
/// framework telling the platform where focus moved. One view, no platform
/// focus to move.
pub(crate) extern "C" fn request_view_focus_change(_view_id: i64, _state: i64, _direction: i64) {}

/// `Void Function(Bool)`.
pub(crate) extern "C" fn set_semantics_tree_enabled(_enabled: bool) {}

/// `Void Function()`, a leaf call: asks for `_beginFrame` and `_drawFrame`
/// at the next vsync. Without a scheduler set, nothing.
pub(crate) extern "C" fn schedule_frame() {
    guard((), || {
        if let Some(schedule) = FRAME_SCHEDULER
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            schedule();
        }
    });
}

/// `Void Function(Bool)`.
pub(crate) extern "C" fn set_needs_report_timings(_needed: bool) {}

/// `Void Function()`. A frame scheduled during the warm-up frame stays
/// pending with the scheduler and is begun at the next vsync, so nothing is
/// left to do here.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_names_are_read_from_standard_method_calls() {
        // MethodCall('get') as StandardMethodCodec writes it: the name, then
        // null arguments.
        assert_eq!(method_name(&[7, 3, b'g', b'e', b't', 0]), Some("get"));
        assert_eq!(method_name(&[7, 3, b'g', b'e']), None);
        assert_eq!(method_name(&[3, 1, 0, 0, 0]), None);
        assert_eq!(method_name(&[]), None);
    }
}
