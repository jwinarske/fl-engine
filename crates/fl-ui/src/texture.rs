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
//! Two kinds are served. A pixel buffer, copied. And a GPU surface of type
//! [`GPU_SURFACE_DMA_BUF`], an fl-engine extension beside the header's
//! Windows types and ivi-homescreen's GL texture: a dma-buf, described by
//! [`FlutterDesktopDmaBuf`] (`crates/fl-ui/include/flutter_texture_dmabuf.h`), which the
//! raster thread imports and samples where it lies. Other GPU-surface types
//! are refused at registration, with -1.
//!
//! The raster thread asks for a texture's frame ([`frame`]) when it draws a
//! scene that shows it and the producer has marked one available since the
//! last; the producer's callback runs there, as the engine runs it on its
//! raster thread, with no lock of the registry's held, so it may call back
//! into this API.
//!
//! Every entry point may be called from any thread, as the header says. A
//! texture is the registry's from registration until the raster thread has
//! let go of it: unregistering removes it at once and queues its completion
//! callback, which runs once the raster thread has destroyed its image
//! ([`take_unregistered`]), on the raster thread.

use std::collections::HashMap;
use std::ffi::c_void;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, MutexGuard};

/// `FlutterDesktopTextureType`.
const PIXEL_BUFFER_TEXTURE: i32 = 0;
const GPU_SURFACE_TEXTURE: i32 = 1;

/// The `FlutterDesktopGpuSurfaceType` of a dma-buf: fl-engine's, after the
/// header's Windows types (1, 2) and ivi-homescreen's GL texture (3).
pub const GPU_SURFACE_DMA_BUF: i32 = 4;

/// The planes a dma-buf may have, as DRM's framebuffers do.
pub const DMA_BUF_MAX_PLANES: usize = 4;

/// How long the raster thread waits on a frame's acquire fence before it
/// gives the frame up.
const ACQUIRE_TIMEOUT_MS: i32 = 1000;

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

/// `FlutterDesktopGpuSurfaceDescriptor`. For a dma-buf, `handle` points to
/// a [`FlutterDesktopDmaBuf`]; `width` and `height` are the buffer's, and
/// `visible_width` by `visible_height` from its top left corner is what is
/// drawn. `format` is not read: the dma-buf's fourcc says it.
/// `release_callback` runs once the engine has what it needs of the
/// descriptor and the dma-buf's: its own duplicates of the descriptors, and
/// the acquire fence waited on.
#[repr(C)]
pub struct FlutterDesktopGpuSurfaceDescriptor {
    pub struct_size: usize,
    pub handle: *mut c_void,
    pub width: usize,
    pub height: usize,
    pub visible_width: usize,
    pub visible_height: usize,
    pub format: i32,
    pub release_callback: Option<unsafe extern "C" fn(*mut c_void)>,
    pub release_context: *mut c_void,
}

/// `FlutterDesktopDmaBuf`: a dma-buf frame, premultiplied, its descriptors
/// lent for the callback's duration.
#[repr(C)]
pub struct FlutterDesktopDmaBuf {
    /// `sizeof(FlutterDesktopDmaBuf)`.
    pub struct_size: usize,
    /// `DRM_FORMAT_ARGB8888` or `DRM_FORMAT_ABGR8888`.
    pub fourcc: u32,
    /// The planes the modifier lays the buffer out in, 1 to 4.
    pub plane_count: u32,
    /// `DRM_FORMAT_MOD_LINEAR`, or the layout the buffer was allocated with.
    pub modifier: u64,
    pub fds: [i32; DMA_BUF_MAX_PLANES],
    pub offsets: [u32; DMA_BUF_MAX_PLANES],
    pub pitches: [u32; DMA_BUF_MAX_PLANES],
    /// A `sync_file` that signals when the frame is written, or -1 where it
    /// already is. Lent, as the planes' are.
    pub acquire_fence: i32,
}

/// `FlutterDesktopGpuSurfaceTextureCallback`.
pub type GpuSurfaceCallback =
    unsafe extern "C" fn(usize, usize, *mut c_void) -> *const FlutterDesktopGpuSurfaceDescriptor;

/// `FlutterDesktopGpuSurfaceTextureConfig`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FlutterDesktopGpuSurfaceTextureConfig {
    pub struct_size: usize,
    pub kind: i32,
    pub callback: Option<GpuSurfaceCallback>,
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

/// What a texture's producer hands over.
#[derive(Clone, Copy)]
enum Source {
    Pixels(PixelBufferCallback),
    DmaBuf(GpuSurfaceCallback),
}

/// A registered texture.
struct Entry {
    source: Source,
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
    /// The producer's dma-buf, to be imported, or found among those already.
    DmaBuf(DmaBufFrame),
    /// The callback gave nothing usable; the texture keeps its last frame.
    Refused(&'static str),
}

/// The newest frame of texture `id`, if one was marked available since the
/// last taken: the copy callback runs here, asked for `width` by `height`,
/// and its buffer is copied and released before this returns.
pub fn frame(id: i64, width: usize, height: usize) -> Frame {
    let (source, user_data) = {
        let mut reg = registry();
        let Some(entry) = reg.as_mut().and_then(|r| r.textures.get_mut(&id)) else {
            return Frame::Unknown;
        };
        if !entry.available {
            return Frame::Unchanged;
        }
        entry.available = false;
        (entry.source, entry.user_data)
    };
    let callback = match source {
        Source::Pixels(callback) => callback,
        // SAFETY: the producer's callback and its user data, as registered.
        Source::DmaBuf(callback) => {
            return unsafe { dma_buf_frame(callback, width, height, user_data) }
        }
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

/// A dma-buf frame, its descriptors the engine's own.
pub struct DmaBufFrame {
    /// The buffer's size.
    pub width: u32,
    pub height: u32,
    /// What is drawn of it, from its top left corner.
    pub visible_width: u32,
    pub visible_height: u32,
    pub fourcc: u32,
    pub modifier: u64,
    pub planes: Vec<DmaBufPlane>,
    /// Which buffer this is: the same for every frame the producer hands
    /// over in the same buffer, so its import can be kept.
    pub key: BufferKey,
}

/// One plane: a duplicate of the producer's descriptor, its offset and pitch.
pub struct DmaBufPlane {
    pub fd: OwnedFd,
    pub offset: u32,
    pub pitch: u32,
}

/// A dma-buf's identity, the first plane's inode, with the layout it was
/// handed over at: a buffer handed over again at another layout is imported
/// again.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BufferKey {
    pub dev: u64,
    pub ino: u64,
    pub fourcc: u32,
    pub modifier: u64,
    pub size: (u32, u32),
    pub layout: Vec<(u32, u32)>,
}

/// Runs a GPU-surface callback and takes what its descriptor lends: the
/// planes' descriptors duplicated, the acquire fence waited on, then the
/// producer's release.
///
/// # Safety
///
/// `callback` is a producer's, called with its `user_data`.
unsafe fn dma_buf_frame(
    callback: GpuSurfaceCallback,
    width: usize,
    height: usize,
    user_data: *mut c_void,
) -> Frame {
    // SAFETY: the producer's callback, as the header says.
    let d = unsafe { callback(width, height, user_data) };
    if d.is_null() {
        return Frame::Refused("its callback returned no GPU surface");
    }
    // SAFETY: a non-null descriptor the producer keeps valid until released.
    let d = unsafe { &*d };
    let taken = if d.struct_size != std::mem::size_of::<FlutterDesktopGpuSurfaceDescriptor>() {
        Err("its GPU surface descriptor has the wrong struct_size")
    } else if d.handle.is_null() {
        Err("its GPU surface descriptor has no dma-buf")
    } else {
        // SAFETY: a GPU_SURFACE_DMA_BUF descriptor's handle, as documented.
        take_dma_buf(d, unsafe { &*(d.handle as *const FlutterDesktopDmaBuf) })
    };
    if let Some(release) = d.release_callback {
        // SAFETY: the producer's release, once, after its descriptors are
        // duplicated and its fence waited on.
        unsafe { release(d.release_context) };
    }
    match taken {
        Ok(frame) => Frame::DmaBuf(frame),
        Err(why) => Frame::Refused(why),
    }
}

fn take_dma_buf(
    d: &FlutterDesktopGpuSurfaceDescriptor,
    b: &FlutterDesktopDmaBuf,
) -> Result<DmaBufFrame, &'static str> {
    use rustix::fd::BorrowedFd;
    if b.struct_size != std::mem::size_of::<FlutterDesktopDmaBuf>() {
        return Err("its dma-buf has the wrong struct_size");
    }
    let count = b.plane_count as usize;
    if count == 0 || count > DMA_BUF_MAX_PLANES {
        return Err("its dma-buf has no planes, or more than four");
    }
    if d.width == 0 || d.height == 0 || d.width > MAX_SIDE || d.height > MAX_SIDE {
        return Err("its dma-buf is empty or larger than 16384 a side");
    }
    if d.visible_width == 0
        || d.visible_height == 0
        || d.visible_width > d.width
        || d.visible_height > d.height
    {
        return Err("its visible size is empty or larger than the buffer");
    }
    if b.fds[..count].iter().any(|fd| *fd < 0) {
        return Err("a plane of its dma-buf has no descriptor");
    }
    if b.acquire_fence >= 0 {
        // SAFETY: the producer's fence, lent until it is released.
        let fence = unsafe { BorrowedFd::borrow_raw(b.acquire_fence) };
        if !signaled(fence) {
            return Err("its acquire fence did not signal within a second");
        }
    }
    let mut planes = Vec::with_capacity(count);
    for i in 0..count {
        // SAFETY: the producer's descriptor, lent until it is released.
        let lent = unsafe { BorrowedFd::borrow_raw(b.fds[i]) };
        let fd = lent
            .try_clone_to_owned()
            .map_err(|_| "a plane's descriptor could not be duplicated")?;
        planes.push(DmaBufPlane {
            fd,
            offset: b.offsets[i],
            pitch: b.pitches[i],
        });
    }
    let stat =
        rustix::fs::fstat(&planes[0].fd).map_err(|_| "its dma-buf could not be identified")?;
    let size = (d.width as u32, d.height as u32);
    Ok(DmaBufFrame {
        width: size.0,
        height: size.1,
        visible_width: d.visible_width as u32,
        visible_height: d.visible_height as u32,
        fourcc: b.fourcc,
        modifier: b.modifier,
        key: BufferKey {
            dev: stat.st_dev as u64,
            ino: stat.st_ino as u64,
            fourcc: b.fourcc,
            modifier: b.modifier,
            size,
            layout: planes.iter().map(|p| (p.offset, p.pitch)).collect(),
        },
        planes,
    })
}

/// Whether a `sync_file` signals within [`ACQUIRE_TIMEOUT_MS`]: it reads as
/// ready once it has.
fn signaled(fence: rustix::fd::BorrowedFd<'_>) -> bool {
    use rustix::event::{poll, PollFd, PollFlags, Timespec};
    let mut fds = [PollFd::new(&fence, PollFlags::IN)];
    let timeout = Timespec {
        tv_sec: (ACQUIRE_TIMEOUT_MS / 1000) as _,
        tv_nsec: ((ACQUIRE_TIMEOUT_MS % 1000) * 1_000_000) as _,
    };
    loop {
        match poll(&mut fds, Some(&timeout)) {
            Ok(n) => return n > 0 && fds[0].revents().contains(PollFlags::IN),
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => return false,
        }
    }
}

/// The textures unregistered since the last call, with their completions,
/// for the raster thread to destroy and then run [`Unregistered::complete`] on.
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
/// texture's id, or -1 for a malformed one or a GPU surface of a type other
/// than [`GPU_SURFACE_DMA_BUF`].
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
    let (source, user_data) = match info.kind {
        PIXEL_BUFFER_TEXTURE => {
            // SAFETY: a pixel-buffer texture's union member.
            let config = unsafe { info.config.pixel_buffer_config };
            let Some(callback) = config.callback else {
                return -1;
            };
            (Source::Pixels(callback), config.user_data)
        }
        GPU_SURFACE_TEXTURE => {
            // SAFETY: a GPU-surface texture's union member.
            let config = unsafe { info.config.gpu_surface_config };
            if config.struct_size != std::mem::size_of::<FlutterDesktopGpuSurfaceTextureConfig>() {
                return -1;
            }
            if config.kind != GPU_SURFACE_DMA_BUF {
                eprintln!(
                    "fl-ui: a GPU-surface texture of type {} was registered; only dma-bufs ({GPU_SURFACE_DMA_BUF}) are drawn",
                    config.kind
                );
                return -1;
            }
            let Some(callback) = config.callback else {
                return -1;
            };
            (Source::DmaBuf(callback), config.user_data)
        }
        _ => return -1,
    };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    registry()
        .get_or_insert_with(Registry::default)
        .textures
        .insert(
            id,
            Entry {
                source,
                user_data,
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

    unsafe extern "C" fn no_surface(
        _w: usize,
        _h: usize,
        _: *mut c_void,
    ) -> *const FlutterDesktopGpuSurfaceDescriptor {
        std::ptr::null()
    }

    unsafe extern "C" fn done(_: *mut c_void) {
        DONE.fetch_add(1, Ordering::SeqCst);
    }

    /// The structs a producer lays out in C, as the headers do on a 64-bit
    /// target.
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn the_gpu_surface_structs_have_the_headers_layout() {
        use std::mem::{offset_of, size_of};
        assert_eq!(size_of::<FlutterDesktopGpuSurfaceTextureConfig>(), 32);
        assert_eq!(size_of::<FlutterDesktopTextureInfo>(), 40);
        assert_eq!(size_of::<FlutterDesktopGpuSurfaceDescriptor>(), 72);
        assert_eq!(offset_of!(FlutterDesktopGpuSurfaceDescriptor, format), 48);
        assert_eq!(
            offset_of!(FlutterDesktopGpuSurfaceDescriptor, release_callback),
            56
        );
        assert_eq!(size_of::<FlutterDesktopDmaBuf>(), 80);
        assert_eq!(offset_of!(FlutterDesktopDmaBuf, modifier), 16);
        assert_eq!(offset_of!(FlutterDesktopDmaBuf, fds), 24);
        assert_eq!(offset_of!(FlutterDesktopDmaBuf, offsets), 40);
        assert_eq!(offset_of!(FlutterDesktopDmaBuf, pitches), 56);
        assert_eq!(offset_of!(FlutterDesktopDmaBuf, acquire_fence), 72);
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

        // A GPU surface of a type other than a dma-buf is refused: here,
        // ivi-homescreen's GL texture.
        let gpu = FlutterDesktopTextureInfo {
            kind: GPU_SURFACE_TEXTURE,
            config: TextureConfig {
                gpu_surface_config: FlutterDesktopGpuSurfaceTextureConfig {
                    struct_size: std::mem::size_of::<FlutterDesktopGpuSurfaceTextureConfig>(),
                    kind: 3,
                    callback: Some(no_surface),
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
