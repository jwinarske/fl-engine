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

/// The posters [`fl_dart_post_from_thread`] started and nobody has joined.
static POSTERS: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> =
    std::sync::Mutex::new(Vec::new());

/// Joins every poster [`fl_dart_post_from_thread`] started. The VM must
/// see no thread call into it during or after `Dart_Cleanup`, and a poster
/// is in `Dart_PostCObject` until it returns, after the message it posted
/// has already been received: so the VM is not cleaned up while one may
/// still be there. Before an isolate's VM goes, by [`Posters`].
pub fn join_posters() {
    let posters = std::mem::take(&mut *POSTERS.lock().unwrap_or_else(|e| e.into_inner()));
    for p in posters {
        let _ = p.join();
    }
}

/// Joins the posters when dropped: declared after the `Vm` it guards, so it
/// is dropped, and they joined, before the VM is cleaned up.
pub struct Posters;

impl Drop for Posters {
    fn drop(&mut self) {
        join_posters();
    }
}

/// Rust→Dart from a thread the VM has never seen: posts `[greeting, n]` to
/// `port` from a freshly spawned thread and returns at once. Dart awaits it
/// on the ReceivePort whose `sendPort.nativePort` it passed in. The thread
/// is kept, and joined before the VM is cleaned up ([`join_posters`]).
#[no_mangle]
pub extern "C" fn fl_dart_post_from_thread(port: i64, n: i64) -> bool {
    let port = NativePort::from_raw(port);
    let spawned = std::thread::Builder::new()
        .name("fl-dart-poster".into())
        .spawn(move || {
            let msg = CObject::Array(vec![CObject::String("from rust".into()), CObject::Int(n)]);
            if port.post(&msg).is_err() {
                eprintln!("fl-dart: port {} closed before the post", port.raw());
            }
        });
    match spawned {
        Ok(handle) => {
            POSTERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(handle);
            true
        }
        Err(_) => false,
    }
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
/// executable's unreferenced functions are garbage-collected before the
/// linker gets to export them (build.rs).
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

/// A test producer for `apps/textures`: the copy callback of a pixel-buffer
/// texture the app registers through the `FlutterDesktop*` C API, as a
/// plugin's library would. 64 by 64 texels, four by four cells of 16, each
/// one of [`PALETTE`]'s colors, shifted by the phase [`fl_dart_texture_next`]
/// advances.
const TEXTURE_SIDE: usize = 64;

/// Opaque, so premultiplied and straight agree.
const PALETTE: [[u8; 4]; 8] = [
    [229, 57, 53, 255],
    [255, 179, 0, 255],
    [67, 160, 71, 255],
    [30, 136, 229, 255],
    [142, 36, 170, 255],
    [0, 172, 193, 255],
    [255, 255, 255, 255],
    [33, 33, 33, 255],
];

struct TestTexture {
    phase: usize,
    pixels: Vec<u8>,
    buffer: fl_ui::texture::FlutterDesktopPixelBuffer,
}

// SAFETY: the buffer's pointer is into `pixels`, which moves with it.
unsafe impl Send for TestTexture {}

static TEST_TEXTURE: std::sync::Mutex<Option<Box<TestTexture>>> = std::sync::Mutex::new(None);

/// The pattern at `phase`, RGBA8: the cell at column `x`, row `y` is
/// `PALETTE[(x + 4 * y + 5 * phase) % 8]`.
fn pattern(phase: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(TEXTURE_SIDE * TEXTURE_SIDE * 4);
    for py in 0..TEXTURE_SIDE {
        for px in 0..TEXTURE_SIDE {
            let (x, y) = (px / 16, py / 16);
            out.extend_from_slice(&PALETTE[(x + 4 * y + 5 * phase) % 8]);
        }
    }
    out
}

/// `FlutterDesktopPixelBufferTextureCallback`: the pattern at the current
/// phase, valid until the next call; nothing to release.
#[no_mangle]
pub extern "C" fn fl_dart_texture_pattern(
    _width: usize,
    _height: usize,
    _user_data: *mut std::ffi::c_void,
) -> *const fl_ui::texture::FlutterDesktopPixelBuffer {
    let mut slot = TEST_TEXTURE.lock().unwrap_or_else(|e| e.into_inner());
    let phase = slot.as_ref().map_or(0, |t| t.phase);
    let mut t = Box::new(TestTexture {
        phase,
        pixels: pattern(phase),
        buffer: fl_ui::texture::FlutterDesktopPixelBuffer {
            buffer: std::ptr::null(),
            width: TEXTURE_SIDE,
            height: TEXTURE_SIDE,
            release_callback: None,
            release_context: std::ptr::null_mut(),
        },
    });
    t.buffer.buffer = t.pixels.as_ptr();
    let out: *const fl_ui::texture::FlutterDesktopPixelBuffer = &t.buffer;
    // The box stays where it is until the next call replaces it.
    *slot = Some(t);
    out
}

/// Advances the test pattern a phase and marks texture `id`'s frame
/// available, as a producer does with a new frame: whether it exists.
#[no_mangle]
pub extern "C" fn fl_dart_texture_next(id: i64) -> bool {
    {
        let mut slot = TEST_TEXTURE.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_mut() {
            Some(t) => t.phase += 1,
            None => {
                *slot = Some(Box::new(TestTexture {
                    phase: 1,
                    pixels: Vec::new(),
                    buffer: fl_ui::texture::FlutterDesktopPixelBuffer {
                        buffer: std::ptr::null(),
                        width: 0,
                        height: 0,
                        release_callback: None,
                        release_context: std::ptr::null_mut(),
                    },
                }))
            }
        }
    }
    fl_ui::texture::FlutterDesktopTextureRegistrarMarkExternalTextureFrameAvailable(
        std::ptr::null_mut(),
        id,
    )
}
