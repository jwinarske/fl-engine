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

/// Upper bound on `fl_dart_post_burst`'s threads: the export is reachable
/// from any program the binary runs.
const BURST_MAX_THREADS: i64 = 64;

/// Racing posts: `threads` threads, released together, each post `each`
/// messages `[thread, seq]` to `port`. Returns once every thread has posted,
/// so Dart can await exactly `threads * each`. The soak and sanitizer lanes'
/// target for `NativePort::post` from many threads at once.
#[no_mangle]
pub extern "C" fn fl_dart_post_burst(port: i64, threads: i64, each: i64) -> bool {
    use std::sync::{Arc, Condvar, Mutex};

    if !(0..=BURST_MAX_THREADS).contains(&threads) || each < 0 {
        return false;
    }
    let port = NativePort::from_raw(port);
    // Not a Barrier: if a spawn fails, the threads already waiting must be
    // released (told not to post) rather than wait for one that never comes.
    let go = Arc::new((Mutex::new(None::<bool>), Condvar::new()));
    let handles: Vec<_> = (0..threads)
        .map(|t| {
            let go = Arc::clone(&go);
            std::thread::Builder::new()
                .name(format!("fl-dart-burst-{t}"))
                .spawn(move || {
                    let (lock, cv) = &*go;
                    let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                    let guard = cv
                        .wait_while(guard, |g| g.is_none())
                        .unwrap_or_else(|e| e.into_inner());
                    let start = *guard == Some(true);
                    drop(guard);
                    start
                        && (0..each).all(|seq| {
                            port.post(&CObject::Array(vec![CObject::Int(t), CObject::Int(seq)]))
                                .is_ok()
                        })
                })
        })
        .collect();
    let spawned = handles.iter().all(Result::is_ok);
    {
        let (lock, cv) = &*go;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = Some(spawned);
        cv.notify_all();
    }
    // Every handle is joined, even after a failure, so no poster outlives
    // the call.
    handles
        .into_iter()
        .map(|h| h.map(|h| h.join().unwrap_or(false)).unwrap_or(false))
        .fold(spawned, |ok, posted| ok & posted)
}

/// Referenced from `main` so the linker keeps the functions above: an
/// executable's unreferenced functions are garbage-collected before
/// `-rdynamic` gets to export them.
type Exports = (
    extern "C" fn(i64, i64) -> i64,
    unsafe extern "C" fn(*const c_char) -> i64,
    extern "C" fn(i64, i64) -> bool,
    extern "C" fn(i64, i64, i64) -> bool,
);

#[used]
pub static KEEP: Exports = (
    fl_dart_add,
    fl_dart_utf8_len,
    fl_dart_post_from_thread,
    fl_dart_post_burst,
);
