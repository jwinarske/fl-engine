//! `ImmutableBuffer`, and the asset bundle it and `flutter/assets` read.
//!
//! A buffer's bytes are an `Arc<[u8]>` peer, so the image natives (stage 2)
//! can share them with the decode thread without copying. Each init native
//! answers its callback later, through the reply queue, as the engine's do:
//! `init` with `void` (null would be failure to `_futurize`),
//! `initFromAsset` and `initFromFile` with the length,
//! or -1 when there is nothing to read (painting.dart's `fromAsset`).
//!
//! Reads are synchronous on the UI thread for now; the decode worker takes
//! them in stage 2 (brief M3 §12).

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use fl_assets::Bundle;
use fl_dart_ffi::{args, guard, peer, sys};

use crate::platform::{queue_reply, ReplyArg};

static BUNDLE: Mutex<Option<Arc<Bundle>>> = Mutex::new(None);

/// Sets the asset bundle `flutter/assets` and `ImmutableBuffer.fromAsset`
/// read. One per process, as the VM is; `None` is no bundle, and every
/// asset is then not found.
pub fn set_asset_bundle(bundle: Option<Arc<Bundle>>) {
    *BUNDLE.lock().unwrap_or_else(|e| e.into_inner()) = bundle;
}

/// The bundle, if one is set.
pub fn asset_bundle() -> Option<Arc<Bundle>> {
    BUNDLE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The asset at `key`, or `None`: not found, or refused by the bundle's
/// rules. A refusal is said, since it is not an ordinary miss.
pub(crate) fn load_asset(key: &str) -> Option<Vec<u8>> {
    let bundle = asset_bundle()?;
    match bundle.load(key) {
        Ok(bytes) => Some(bytes),
        Err(fl_assets::AssetError::NotFound(_)) => None,
        Err(e) => {
            eprintln!("fl-ui: asset: {e}");
            None
        }
    }
}

/// The largest file `ImmutableBuffer.fromFilePath` reads.
const FILE_LIMIT: u64 = fl_assets::DEFAULT_LIMIT;

/// The `ImmutableBuffer` peer.
pub(crate) struct Buffer(pub(crate) Arc<[u8]>);

/// A native's error result: a Dart String, which dart:ui throws.
///
/// # Safety
/// The isolate is current with an API scope open.
unsafe fn error(msg: &str) -> sys::Dart_Handle {
    let msg = std::ffi::CString::new(msg).unwrap_or_default();
    // SAFETY: per this function's contract.
    unsafe { sys::Dart_NewStringFromCString(msg.as_ptr()) }
}

/// Attaches `bytes` to `wrapper` and queues the callback with `answer`.
///
/// # Safety
/// A Handle-parameter native: isolate current, scope open; both handles
/// live.
unsafe fn attach_and_answer(
    wrapper: sys::Dart_Handle,
    callback: sys::Dart_Handle,
    bytes: Vec<u8>,
    answer: ReplyArg,
) -> sys::Dart_Handle {
    let size = bytes.len();
    // SAFETY: per this function's contract.
    unsafe {
        if let Err(e) = peer::attach(wrapper, Buffer(bytes.into()), size) {
            return error(&format!("ImmutableBuffer: {e}"));
        }
        queue_reply(callback, answer);
        args::null()
    }
}

/// Checks the callback is a closure, as the engine does before anything
/// else.
///
/// # Safety
/// As [`attach_and_answer`].
unsafe fn closure(callback: sys::Dart_Handle) -> bool {
    // SAFETY: per this function's contract.
    unsafe { sys::Dart_IsClosure(callback) }
}

/// `Handle Function(Handle this, Handle Uint8List list, Handle callback)`.
pub(crate) extern "C" fn init(
    wrapper: sys::Dart_Handle,
    list: sys::Dart_Handle,
    callback: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: a Handle-parameter native; the three are live handles.
        unsafe {
            if !closure(callback) {
                return error("Callback must be a function");
            }
            let Some(bytes) = args::bytes(list) else {
                return error("ImmutableBuffer: not a Uint8List");
            };
            attach_and_answer(wrapper, callback, bytes, ReplyArg::Void)
        }
    })
}

/// `Handle Function(Handle this, Handle String key, Handle callback)`.
/// The length on success, -1 when the bundle has no such asset.
pub(crate) extern "C" fn init_from_asset(
    wrapper: sys::Dart_Handle,
    key: sys::Dart_Handle,
    callback: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: as init.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: as init.
        unsafe {
            if !closure(callback) {
                return error("Callback must be a function");
            }
            let Some(key) = args::string(key) else {
                return error("Asset name must be valid UTF8");
            };
            match load_asset(&key) {
                Some(bytes) => {
                    let n = bytes.len() as i64;
                    attach_and_answer(wrapper, callback, bytes, ReplyArg::Int(n))
                }
                None => {
                    queue_reply(callback, ReplyArg::Int(-1));
                    args::null()
                }
            }
        }
    })
}

/// `Handle Function(Handle this, Handle String path, Handle callback)`.
/// Any path, as `dart:io` allows the app; regular files up to
/// [`FILE_LIMIT`] only.
pub(crate) extern "C" fn init_from_file(
    wrapper: sys::Dart_Handle,
    path: sys::Dart_Handle,
    callback: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: as init.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: as init.
        unsafe {
            if !closure(callback) {
                return error("Callback must be a function");
            }
            let Some(path) = args::string(path) else {
                return error("File path must be valid UTF8");
            };
            match fl_assets::read_file(std::path::Path::new(&path), FILE_LIMIT) {
                Ok(bytes) => {
                    let n = bytes.len() as i64;
                    attach_and_answer(wrapper, callback, bytes, ReplyArg::Int(n))
                }
                Err(e) => {
                    if !matches!(e, fl_assets::AssetError::NotFound(_)) {
                        eprintln!("fl-ui: ImmutableBuffer.fromFilePath: {e}");
                    }
                    queue_reply(callback, ReplyArg::Int(-1));
                    args::null()
                }
            }
        }
    })
}

/// `Void Function(Pointer<Void>)`.
pub(crate) extern "C" fn dispose(buffer: *mut c_void) {
    guard((), || {
        // SAFETY: buffer is the receiver's native field.
        drop(unsafe { peer::take::<Buffer>(buffer) });
    });
}
