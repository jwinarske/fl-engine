//! External textures: what a `Texture` widget shows, registered by a plugin
//! through Flutter's desktop C API (`flutter_texture_registrar.h`), as
//! ivi-homescreen's in-tree plugins and Flutter's desktop ones register
//! theirs.
//!
//! The registry is process-wide: fl-engine runs one engine, so
//! [`FlutterDesktopRegistrarGetTextureRegistrar`] answers the same registrar
//! for any plugin registrar it is given, and for none, which is what a Dart
//! FFI plugin, having no plugin registrar, passes.
//!
//! Pixel-buffer textures are what this serves. The raster thread asks for a
//! texture's frame ([`frame`]) when it draws a scene that shows it and the
//! producer has marked one available since the last; the copy callback runs
//! there, as the engine runs it on its raster thread, with no lock of the
//! registry's held, so it may call back into this API. GPU-surface textures
//! are refused at registration, with -1.
//!
//! Every entry point may be called from any thread, as the header says. A
//! texture is the registry's from registration until the raster thread has
//! let go of it: unregistering removes it at once and queues its completion
//! callback, which runs once the raster thread has destroyed its image
//! ([`take_unregistered`]), on the raster thread.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, MutexGuard};

/// `FlutterDesktopTextureType`.
const PIXEL_BUFFER_TEXTURE: i32 = 0;

/// The largest side a frame may have, emblema's own limit for an image.
const MAX_SIDE: usize = 16_384;

/// `FlutterDesktopPixelBuffer`: tightly packed RGBA8, premultiplied, as the
/// engine samples a pixel buffer, `width * height` pixels.
#[repr(C)]
pub struct FlutterDesktopPixelBuffer {
    pub buffer: *const u8,
    pub width: usize,
    pub height: usize,
    pub release_callback: Option<unsafe extern "C" fn(*mut c_void)>,
    pub release_context: *mut c_void,
}

/// `FlutterDesktopPixelBufferTextureCallback`.
pub type PixelBufferCallback =
    unsafe extern "C" fn(usize, usize, *mut c_void) -> *const FlutterDesktopPixelBuffer;

/// `FlutterDesktopPixelBufferTextureConfig`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FlutterDesktopPixelBufferTextureConfig {
    pub callback: Option<PixelBufferCallback>,
    pub user_data: *mut c_void,
}

/// `FlutterDesktopGpuSurfaceTextureConfig`, read only to be refused.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FlutterDesktopGpuSurfaceTextureConfig {
    pub struct_size: usize,
    pub kind: i32,
    pub callback: *const c_void,
    pub user_data: *mut c_void,
}

/// The union in `FlutterDesktopTextureInfo`.
#[repr(C)]
pub union TextureConfig {
    pub pixel_buffer_config: FlutterDesktopPixelBufferTextureConfig,
    pub gpu_surface_config: FlutterDesktopGpuSurfaceTextureConfig,
}

/// `FlutterDesktopTextureInfo`.
#[repr(C)]
pub struct FlutterDesktopTextureInfo {
    pub kind: i32,
    pub config: TextureConfig,
}

/// `void (*)(void* user_data)`, an unregister's completion.
pub type Completion = unsafe extern "C" fn(*mut c_void);

/// A registered pixel-buffer texture.
struct Entry {
    callback: PixelBufferCallback,
    user_data: *mut c_void,
    /// A frame marked available that the raster thread has not taken; true
    /// from registration, so the first scene that shows it takes one.
    available: bool,
}

/// A completion queued by an unregister, for the raster thread to run once
/// the texture's image is gone.
struct Pending {
    id: i64,
    completion: Option<(Completion, *mut c_void)>,
}

#[derive(Default)]
struct Registry {
    textures: HashMap<i64, Entry>,
    unregistered: Vec<Pending>,
}

// SAFETY: the raw pointers are the producer's: its user data and contexts,
// which the C API hands back on whatever thread it is called from, as the
// header allows. The registry only stores and passes them.
unsafe impl Send for Registry {}

static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);
static NEXT_ID: AtomicI64 = AtomicI64::new(1);

/// Wakes the raster thread: a frame was marked available, or a texture was
/// unregistered. Set by whoever runs the raster thread.
type Wake = Box<dyn Fn() + Send + Sync>;
static WAKE: Mutex<Option<Wake>> = Mutex::new(None);

/// The one registrar handle this answers. Its address is all a plugin sees.
static REGISTRAR: u8 = 0;

fn registry() -> MutexGuard<'static, Option<Registry>> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

fn wake() {
    if let Some(w) = WAKE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        w();
    }
}

/// What wakes the raster thread when a texture has a new frame or has been
/// unregistered; `None` to stop.
pub fn set_texture_wake(wake: Option<Wake>) {
    *WAKE.lock().unwrap_or_else(|e| e.into_inner()) = wake;
}

/// A texture's frame, as the raster thread asks for it.
pub enum Frame {
    /// No texture by that id: never registered, or unregistered since.
    Unknown,
    /// No frame marked available since the last one taken.
    Unchanged,
    /// The producer's pixels, copied: premultiplied RGBA8, `width * height`.
    Pixels {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    /// The callback gave nothing usable; the texture keeps its last frame.
    Refused(&'static str),
}

/// The newest frame of texture `id`, if one was marked available since the
/// last taken: the copy callback runs here, asked for `width` by `height`,
/// and its buffer is copied and released before this returns.
pub fn frame(id: i64, width: usize, height: usize) -> Frame {
    let (callback, user_data) = {
        let mut reg = registry();
        let Some(entry) = reg.as_mut().and_then(|r| r.textures.get_mut(&id)) else {
            return Frame::Unknown;
        };
        if !entry.available {
            return Frame::Unchanged;
        }
        entry.available = false;
        (entry.callback, entry.user_data)
    };
    // SAFETY: the producer's callback, called as the header says: the size
    // asked for and its own user data. Outside the lock, so it may mark
    // frames or register textures itself.
    let buffer = unsafe { callback(width, height, user_data) };
    if buffer.is_null() {
        return Frame::Refused("its callback returned no pixel buffer");
    }
    // SAFETY: a non-null buffer the producer keeps valid until released.
    let b = unsafe { &*buffer };
    let copied = copy_pixels(b);
    if let Some(release) = b.release_callback {
        // SAFETY: the producer's release, once, after the copy, as promised.
        unsafe { release(b.release_context) };
    }
    copied
}

fn copy_pixels(b: &FlutterDesktopPixelBuffer) -> Frame {
    if b.buffer.is_null() {
        return Frame::Refused("its pixel buffer has no pixels");
    }
    if b.width == 0 || b.height == 0 || b.width > MAX_SIDE || b.height > MAX_SIDE {
        return Frame::Refused("its pixel buffer is empty or larger than 16384 a side");
    }
    let len = b.width * b.height * 4;
    // SAFETY: the header's contract: `width * height` RGBA8 pixels at
    // `buffer`, valid until released.
    let rgba = unsafe { std::slice::from_raw_parts(b.buffer, len) }.to_vec();
    Frame::Pixels {
        width: b.width as u32,
        height: b.height as u32,
        rgba,
    }
}

/// The textures unregistered since the last call, with their completions,
/// for the raster thread to destroy and then run [`complete`] on.
pub fn take_unregistered() -> Vec<Unregistered> {
    registry()
        .as_mut()
        .map(|r| std::mem::take(&mut r.unregistered))
        .unwrap_or_default()
        .into_iter()
        .map(|p| Unregistered {
            id: p.id,
            completion: p.completion,
        })
        .collect()
}

/// A texture unregistered, its image still to be destroyed.
pub struct Unregistered {
    pub id: i64,
    completion: Option<(Completion, *mut c_void)>,
}

// SAFETY: as Registry's: the producer's context, passed back to it.
unsafe impl Send for Unregistered {}

impl Unregistered {
    /// Runs the producer's completion: its texture is gone from the device.
    pub fn complete(self) {
        if let Some((f, data)) = self.completion {
            // SAFETY: the producer's callback and its data, once.
            unsafe { f(data) };
        }
    }
}

/// `FlutterDesktopRegistrarGetTextureRegistrar`: the process's registrar,
/// whatever plugin registrar is given, a null one included.
#[no_mangle]
pub extern "C" fn FlutterDesktopRegistrarGetTextureRegistrar(
    _registrar: *mut c_void,
) -> *mut c_void {
    std::ptr::addr_of!(REGISTRAR) as *mut c_void
}

/// `FlutterDesktopTextureRegistrarRegisterExternalTexture`: the new
/// texture's id, or -1 for a GPU-surface texture or a malformed one.
///
/// # Safety
///
/// `info` is null or points to a `FlutterDesktopTextureInfo`.
#[no_mangle]
pub unsafe extern "C" fn FlutterDesktopTextureRegistrarRegisterExternalTexture(
    _registrar: *mut c_void,
    info: *const FlutterDesktopTextureInfo,
) -> i64 {
    if info.is_null() {
        return -1;
    }
    // SAFETY: the caller's info, as the header defines it.
    let info = unsafe { &*info };
    if info.kind != PIXEL_BUFFER_TEXTURE {
        eprintln!("fl-ui: a GPU-surface texture was registered; only pixel buffers are drawn");
        return -1;
    }
    // SAFETY: a pixel-buffer texture's union member.
    let config = unsafe { info.config.pixel_buffer_config };
    let Some(callback) = config.callback else {
        return -1;
    };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    registry()
        .get_or_insert_with(Registry::default)
        .textures
        .insert(
            id,
            Entry {
                callback,
                user_data: config.user_data,
                available: true,
            },
        );
    id
}

/// `FlutterDesktopTextureRegistrarUnregisterExternalTexture`: the texture
/// is gone at once; `callback` runs once the raster thread has destroyed its
/// image.
///
/// # Safety
///
/// `callback`, if given, is safe to call with `user_data` from another
/// thread, once.
#[no_mangle]
pub unsafe extern "C" fn FlutterDesktopTextureRegistrarUnregisterExternalTexture(
    _registrar: *mut c_void,
    texture_id: i64,
    callback: Option<Completion>,
    user_data: *mut c_void,
) {
    // With no raster thread to wake there is no image to wait for either:
    // whatever drew the texture has gone with the thread.
    let raster = WAKE.lock().unwrap_or_else(|e| e.into_inner()).is_some();
    let queued = {
        let mut reg = registry();
        let r = reg.get_or_insert_with(Registry::default);
        let queued = r.textures.remove(&texture_id).is_some() && raster;
        if queued {
            r.unregistered.push(Pending {
                id: texture_id,
                completion: callback.map(|f| (f, user_data)),
            });
        }
        queued
    };
    if queued {
        wake();
    } else if let Some(f) = callback {
        // Nothing to wait for: the header's completion still comes.
        // SAFETY: the caller's callback and data, once.
        unsafe { f(user_data) };
    }
}

/// `FlutterDesktopTextureRegistrarMarkExternalTextureFrameAvailable`:
/// whether the texture exists. Its next frame is taken when the raster
/// thread next draws a scene that shows it, which this asks for.
#[no_mangle]
pub extern "C" fn FlutterDesktopTextureRegistrarMarkExternalTextureFrameAvailable(
    _registrar: *mut c_void,
    texture_id: i64,
) -> bool {
    let known = registry()
        .as_mut()
        .and_then(|r| r.textures.get_mut(&texture_id))
        .map(|e| e.available = true)
        .is_some();
    if known {
        wake();
    }
    known
}

/// `FlutterDesktopTextureMakeCurrent`: false, as there is no GL context.
#[no_mangle]
pub extern "C" fn FlutterDesktopTextureMakeCurrent(_registrar: *mut c_void) -> bool {
    false
}

/// `FlutterDesktopTextureClearCurrent`: false, as there is no GL context.
#[no_mangle]
pub extern "C" fn FlutterDesktopTextureClearCurrent(_registrar: *mut c_void) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    const W: usize = 2;
    const H: usize = 3;
    static PIXELS: [u8; W * H * 4] = [7; W * H * 4];
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    static RELEASES: AtomicUsize = AtomicUsize::new(0);
    static DONE: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn release(_: *mut c_void) {
        RELEASES.fetch_add(1, Ordering::SeqCst);
    }

    unsafe extern "C" fn produce(
        _w: usize,
        _h: usize,
        _: *mut c_void,
    ) -> *const FlutterDesktopPixelBuffer {
        CALLS.fetch_add(1, Ordering::SeqCst);
        static mut BUF: FlutterDesktopPixelBuffer = FlutterDesktopPixelBuffer {
            buffer: std::ptr::null(),
            width: W,
            height: H,
            release_callback: Some(release),
            release_context: std::ptr::null_mut(),
        };
        // SAFETY: test-only, one test touches it.
        unsafe {
            BUF.buffer = PIXELS.as_ptr();
            std::ptr::addr_of!(BUF)
        }
    }

    unsafe extern "C" fn done(_: *mut c_void) {
        DONE.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn a_pixel_buffer_texture_is_copied_when_marked_and_completed_once_gone() {
        // A raster thread to wake, so an unregister waits for it.
        static WAKES: AtomicUsize = AtomicUsize::new(0);
        set_texture_wake(Some(Box::new(|| {
            WAKES.fetch_add(1, Ordering::SeqCst);
        })));
        let reg = FlutterDesktopRegistrarGetTextureRegistrar(std::ptr::null_mut());
        assert!(!reg.is_null());
        let info = FlutterDesktopTextureInfo {
            kind: PIXEL_BUFFER_TEXTURE,
            config: TextureConfig {
                pixel_buffer_config: FlutterDesktopPixelBufferTextureConfig {
                    callback: Some(produce),
                    user_data: std::ptr::null_mut(),
                },
            },
        };
        // SAFETY: a well-formed info.
        let id = unsafe { FlutterDesktopTextureRegistrarRegisterExternalTexture(reg, &info) };
        assert!(id > 0);

        // The first frame comes without a mark.
        match frame(id, 10, 10) {
            Frame::Pixels {
                width,
                height,
                rgba,
            } => {
                assert_eq!((width, height), (W as u32, H as u32));
                assert_eq!(rgba, PIXELS.to_vec());
            }
            _ => panic!("no first frame"),
        }
        assert!(matches!(frame(id, 10, 10), Frame::Unchanged));
        assert!(FlutterDesktopTextureRegistrarMarkExternalTextureFrameAvailable(reg, id));
        assert!(matches!(frame(id, 10, 10), Frame::Pixels { .. }));
        assert_eq!(CALLS.load(Ordering::SeqCst), 2);
        assert_eq!(RELEASES.load(Ordering::SeqCst), 2);

        // SAFETY: `done` may run on any thread.
        unsafe {
            FlutterDesktopTextureRegistrarUnregisterExternalTexture(
                reg,
                id,
                Some(done),
                std::ptr::null_mut(),
            )
        };
        assert!(matches!(frame(id, 10, 10), Frame::Unknown));
        assert!(!FlutterDesktopTextureRegistrarMarkExternalTextureFrameAvailable(reg, id));
        assert_eq!(
            DONE.load(Ordering::SeqCst),
            0,
            "not before the raster thread lets go"
        );
        let gone = take_unregistered();
        assert_eq!(gone.iter().map(|u| u.id).collect::<Vec<_>>(), vec![id]);
        gone.into_iter().for_each(Unregistered::complete);
        assert_eq!(DONE.load(Ordering::SeqCst), 1);
        // A mark woke it once, the unregister once more.
        assert_eq!(WAKES.load(Ordering::SeqCst), 2);

        // With no raster thread, the completion comes at once.
        set_texture_wake(None);
        // SAFETY: a well-formed info.
        let again = unsafe { FlutterDesktopTextureRegistrarRegisterExternalTexture(reg, &info) };
        // SAFETY: `done` may run on any thread.
        unsafe {
            FlutterDesktopTextureRegistrarUnregisterExternalTexture(
                reg,
                again,
                Some(done),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(DONE.load(Ordering::SeqCst), 2);
        assert!(take_unregistered().is_empty());

        // A GPU-surface texture is refused.
        let gpu = FlutterDesktopTextureInfo {
            kind: 1,
            config: TextureConfig {
                gpu_surface_config: FlutterDesktopGpuSurfaceTextureConfig {
                    struct_size: std::mem::size_of::<FlutterDesktopGpuSurfaceTextureConfig>(),
                    kind: 4,
                    callback: std::ptr::null(),
                    user_data: std::ptr::null_mut(),
                },
            },
        };
        // SAFETY: a well-formed info.
        assert_eq!(
            unsafe { FlutterDesktopTextureRegistrarRegisterExternalTexture(reg, &gpu) },
            -1
        );
    }
}
