//! What Dart calls in this process, through `DynamicLibrary.process()`.
//!
//! The M1 proof points (brief §9), deliberately minimal: plain exported C
//! symbols, no resolver and no generated code. Whether this stays the shape
//! of engine natives at M2 is what proving it on all three targets decides.

use std::ffi::{c_char, CStr};

use fl_dart_vm::{CObject, NativePort};

/// Scalars in and out.
#[no_mangle]
pub extern "C" fn fl_dart_add(a: i64, b: i64) -> i64 {
    a.wrapping_add(b)
}

/// A `Pointer<Utf8>` in: the string's length in bytes, or -1 for null.
///
/// # Safety
/// `s` is null or a NUL-terminated string valid for the call, which is what
/// `toNativeUtf8()` produces.
#[no_mangle]
pub unsafe extern "C" fn fl_dart_utf8_len(s: *const c_char) -> i64 {
    if s.is_null() {
        return -1;
    }
    // SAFETY: per this function's contract.
    unsafe { CStr::from_ptr(s) }.to_bytes().len() as i64
}

/// Rust→Dart from a thread the VM has never seen: posts `[greeting, n]` to
/// `port` from a freshly spawned thread and returns at once. Dart awaits it
/// on the ReceivePort whose `sendPort.nativePort` it passed in.
#[no_mangle]
pub extern "C" fn fl_dart_post_from_thread(port: i64, n: i64) -> bool {
    let port = NativePort::from_raw(port);
    std::thread::Builder::new()
        .name("fl-dart-poster".into())
        .spawn(move || {
            let msg = CObject::Array(vec![CObject::String("from rust".into()), CObject::Int(n)]);
            if port.post(&msg).is_err() {
                eprintln!("fl-dart: port {} closed before the post", port.raw());
            }
        })
        .is_ok()
}

/// Referenced from `main` so the linker keeps the functions above: an
/// executable's unreferenced functions are garbage-collected before
/// `-rdynamic` gets to export them.
#[used]
pub static KEEP: (
    extern "C" fn(i64, i64) -> i64,
    unsafe extern "C" fn(*const c_char) -> i64,
    extern "C" fn(i64, i64) -> bool,
) = (fl_dart_add, fl_dart_utf8_len, fl_dart_post_from_thread);
