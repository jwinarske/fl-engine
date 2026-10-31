//! `ImageDescriptor`, `Codec`, `Image` and `ImageShader` (brief M3 §4).
//!
//! Decoding is fl-image's, on its one worker thread (decision D5). The UI
//! thread only reads headers, which is what `ImageDescriptor.encoded` needs
//! and what the pixel limit is checked against before anything is decoded
//! (brief M3 §15).
//!
//! The worker never touches a Dart handle. `getNextFrame` keeps its callback
//! as a persistent handle under the job's id; the finished job is queued
//! with its id and the isolate generation it was for, and resolved on the UI
//! thread by [`take_done`], between loop steps, through the reply queue.
//! Results for a generation that has ended are dropped, and its queued jobs
//! with them ([`end_generation`]).
//!
//! An `Image` peer is an [`Arc<ImageData>`]; drawing one records the `Arc`.
//! The raster thread uploads the pixels and keeps the GPU image while the
//! `Arc` lives.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use fl_dart_ffi::{args, guard, peer, sys};
use fl_image::{Done, Frames, Job, Limits, Raw, Source, Worker};

use crate::buffer::Buffer;
use crate::platform::{queue_persistent, queue_reply, wake_for_replies, Persistent, ReplyArg};

/// A decoded image: premultiplied RGBA8, `width * height` pixels.
///
/// The pixels are taken by the raster thread when it uploads them
/// ([`ImageData::take_pixels`]), so a decoded image is not held twice. Two
/// `ImageData` are equal only if they are the same one.
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pixels: Mutex<Option<Vec<u8>>>,
}

impl ImageData {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> ImageData {
        ImageData {
            width,
            height,
            pixels: Mutex::new(Some(pixels)),
        }
    }

    /// The pixels, once: the uploader's. `None` once taken.
    pub fn take_pixels(&self) -> Option<Vec<u8>> {
        self.pixels.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    /// The bytes the pixels take while held on this side.
    fn size(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

impl PartialEq for ImageData {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl std::fmt::Debug for ImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImageData({}x{})", self.width, self.height)
    }
}

/// An `ImageShader`: an image as a paint's fill.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageShader {
    pub image: Arc<ImageData>,
    /// `TileMode` indices, x and y.
    pub tile: [i32; 2],
    /// A `FilterQuality` index, or `None` to take the paint's.
    pub quality: Option<i32>,
    /// From the shader's space to the canvas's, column-major.
    pub matrix: [f64; 16],
}

/// What [`set_image_sink`] sets.
type ImageSink = Box<dyn Fn(Arc<ImageData>) + Send>;

static SINK: Mutex<Option<ImageSink>> = Mutex::new(None);

/// Sets what is told of each image as it is decoded, on the UI thread: the
/// raster thread's uploader, so an image is on the device before a frame
/// draws it (brief M3 §15).
pub fn set_image_sink(sink: impl Fn(Arc<ImageData>) + Send + 'static) {
    *SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(sink));
}

/// Clears the sink [`set_image_sink`] set.
pub fn clear_image_sink() {
    *SINK.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// The pixel limit every decode is held to.
static LIMITS: Mutex<Limits> = Mutex::new(Limits {
    max_pixels: fl_image::DEFAULT_MAX_PIXELS,
    max_frames: fl_image::DEFAULT_MAX_FRAMES,
});

/// Sets the most pixels an image may have, decoded or as a decode target.
pub fn set_max_image_pixels(max_pixels: u64) {
    LIMITS.lock().unwrap_or_else(|e| e.into_inner()).max_pixels = max_pixels;
}

fn limits() -> Limits {
    *LIMITS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The isolate generation: bumped when an isolate's run ends.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Finished jobs, from the worker, waiting for the UI thread.
static DONE: Mutex<Vec<Done>> = Mutex::new(Vec::new());

/// The callbacks waiting on each job, by id, and the codec to cache the
/// frame in if it still lives.
static PENDING: Mutex<Option<HashMap<u64, Pending>>> = Mutex::new(None);

struct Pending {
    callbacks: Vec<Persistent>,
    codec: Weak<CodecInner>,
}

/// The worker, started on the first decode. `None` if it could not be.
fn worker() -> Option<&'static Worker> {
    static WORKER: OnceLock<Option<Worker>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            Worker::spawn(|done| {
                DONE.lock().unwrap_or_else(|e| e.into_inner()).push(done);
                wake_for_replies();
            })
            .map_err(|e| eprintln!("fl-ui: the decode thread: {e}"))
            .ok()
        })
        .as_ref()
}

/// Whether a finished decode waits for [`take_done`].
pub(crate) fn has_done() -> bool {
    !DONE.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
}

/// Whether a decode is still to finish: the loop must not end while one is.
pub fn awaiting_decodes() -> bool {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|p| !p.is_empty())
}

/// Moves the finished jobs of this generation into the reply queue, and
/// caches each frame in its codec. On the UI thread, the isolate current.
pub(crate) fn take_done() {
    let done = std::mem::take(&mut *DONE.lock().unwrap_or_else(|e| e.into_inner()));
    let generation = GENERATION.load(Ordering::Acquire);
    for d in done {
        if d.tag != generation {
            continue;
        }
        let Some(p) = PENDING
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            .and_then(|m| m.remove(&d.id))
        else {
            continue;
        };
        let state = match d.result {
            Ok(f) => {
                let image = Arc::new(ImageData::new(f.width, f.height, f.pixels));
                if let Some(sink) = SINK.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                    sink(Arc::clone(&image));
                }
                State::Ready(image, f.duration_ms)
            }
            Err(e) => State::Failed(e.to_string()),
        };
        for cb in p.callbacks {
            queue_persistent(cb, state.answer());
        }
        // A still's frame is kept for every call; an animation's is not.
        if let Some(codec) = p.codec.upgrade().filter(|c| c.frames.is_none()) {
            *codec.state.lock().unwrap_or_else(|e| e.into_inner()) = state;
        }
    }
}

/// Ends the current generation, for when its isolate's run ends: its queued
/// jobs are dropped unstarted, its results as they arrive, and its waiting
/// callbacks, whose handles went with the isolate.
pub(crate) fn end_generation() {
    let old = GENERATION.fetch_add(1, Ordering::AcqRel);
    if let Some(w) = worker() {
        w.retire(old);
    }
    if let Some(p) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        // Their isolate freed the handles; forgetting them is all that is left.
        p.clear();
    }
    DONE.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// A native's error result: a Dart String, which dart:ui throws.
///
/// # Safety
/// The isolate is current with an API scope open.
unsafe fn error(msg: &str) -> sys::Dart_Handle {
    let msg = std::ffi::CString::new(msg).unwrap_or_default();
    // SAFETY: per this function's contract.
    unsafe { sys::Dart_NewStringFromCString(msg.as_ptr()) }
}

/// The `ImageDescriptor` peer: the bytes, what they are, and the size.
struct Descriptor {
    bytes: Arc<[u8]>,
    source: Source,
    width: u32,
    height: u32,
}

/// `Handle Function(Handle this, Pointer<Void> buffer, Handle callback)`:
/// reads the header, holds its size to the pixel limit, and answers the
/// callback with void. An error String is thrown by dart:ui.
pub(crate) extern "C" fn descriptor_init_encoded(
    wrapper: sys::Dart_Handle,
    buffer: *mut c_void,
    callback: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: a Handle-parameter native; the handles are live, buffer
        // is the ImmutableBuffer argument's native field.
        unsafe {
            if !sys::Dart_IsClosure(callback) {
                return error("Callback must be a function");
            }
            let Some(bytes) = peer::with::<Buffer, _>(buffer, |b| Arc::clone(&b.0)) else {
                return error("Buffer parameter must not be null");
            };
            let header = match fl_image::header(&bytes) {
                Ok(h) => h,
                Err(fl_image::ImageError::Unknown) => return error("Invalid image data"),
                Err(e) => return error(&format!("Invalid image data: {e}")),
            };
            if let Err(e) = limits().check(header.width, header.height) {
                return error(&e.to_string());
            }
            let d = Descriptor {
                bytes,
                source: Source::Encoded,
                width: header.width,
                height: header.height,
            };
            if let Err(e) = peer::attach(wrapper, d, 0) {
                return error(&format!("ImageDescriptor: {e}"));
            }
            queue_reply(callback, ReplyArg::Void);
            null
        }
    })
}

/// `Void Function(Handle this, Handle buffer, Int32 width, Int32 height,
/// Int32 rowBytes, Int32 pixelFormat)`: pixels as they are, premultiplied.
/// Checked when decoded: this native has no way to fail.
pub(crate) extern "C" fn descriptor_init_raw(
    wrapper: sys::Dart_Handle,
    buffer: sys::Dart_Handle,
    width: i32,
    height: i32,
    row_bytes: i32,
    format: i32,
) {
    guard((), || {
        // SAFETY: a Handle-parameter native; the handles are live.
        unsafe {
            let bytes = peer::field(buffer)
                .and_then(|f| peer::with::<Buffer, _>(f, |b| Arc::clone(&b.0)))
                .unwrap_or_else(|| Arc::from(&[][..]));
            let (w, h) = (width.max(0) as u32, height.max(0) as u32);
            let row = if row_bytes < 0 {
                w as usize * 4
            } else {
                row_bytes as usize
            };
            // rgba8888 and bgra8888; the float formats are refused when
            // decoded, by a width no row can hold.
            let raw = Raw {
                width: w,
                height: h,
                row_bytes: if format <= 1 { row } else { 0 },
                bgra: format == 1,
            };
            let d = Descriptor {
                bytes,
                source: Source::Raw(raw),
                width: w,
                height: h,
            };
            if let Err(e) = peer::attach(wrapper, d, 0) {
                eprintln!("fl-ui: ImageDescriptor.raw: {e}");
            }
        }
    });
}

/// `Int32 Function(Pointer<Void>)`, a leaf call.
pub(crate) extern "C" fn descriptor_width(d: *mut c_void) -> i32 {
    // SAFETY: d is the receiver's native field.
    guard(0, || {
        unsafe { peer::with::<Descriptor, _>(d, |d| d.width as i32) }.unwrap_or(0)
    })
}

/// `Int32 Function(Pointer<Void>)`, a leaf call.
pub(crate) extern "C" fn descriptor_height(d: *mut c_void) -> i32 {
    // SAFETY: d is the receiver's native field.
    guard(0, || {
        unsafe { peer::with::<Descriptor, _>(d, |d| d.height as i32) }.unwrap_or(0)
    })
}

/// `Int32 Function(Pointer<Void>)`, a leaf call: 4, RGBA8, what every
/// image here decodes to.
pub(crate) extern "C" fn descriptor_bytes_per_pixel(_d: *mut c_void) -> i32 {
    4
}

/// `Void Function(Pointer<Void>)`.
pub(crate) extern "C" fn descriptor_dispose(d: *mut c_void) {
    guard((), || {
        // SAFETY: d is the receiver's native field.
        let _ = unsafe { peer::take::<Descriptor>(d) };
    });
}

/// A codec's frame, once there is one.
enum State {
    Idle,
    Decoding(u64),
    /// The frame and its duration.
    Ready(Arc<ImageData>, u32),
    Failed(String),
}

impl State {
    /// What a `getNextFrame` callback is called with in this state.
    fn answer(&self) -> ReplyArg {
        match self {
            State::Ready(image, ms) => ReplyArg::Frame(Arc::clone(image), *ms),
            State::Failed(e) => ReplyArg::FrameError(e.clone()),
            State::Idle | State::Decoding(_) => ReplyArg::FrameError("not decoded".into()),
        }
    }
}

/// A codec. A still, as the engine's `SingleFrameCodec`: decoded once, on
/// the first `getNextFrame`; the frame is kept and given to every call.
/// An animation, as its `MultiFrameCodec`: each call decodes the next
/// frame on the worker, in turn, at the image's own size; nothing is kept
/// on this side (brief M3 §15).
struct CodecInner {
    bytes: Arc<[u8]>,
    source: Source,
    target: (u32, u32),
    /// An animation's frames, shared with the worker's jobs.
    frames: Option<Arc<Mutex<Frames>>>,
    frame_count: u32,
    repetition_count: i32,
    state: Mutex<State>,
}

/// `Void Function(Pointer<Void> descriptor, Handle outCodec, Int32
/// targetWidth, Int32 targetHeight, Int32 targetFormat)`. The target format
/// is not kept: every frame is RGBA8.
pub(crate) extern "C" fn descriptor_instantiate_codec(
    d: *mut c_void,
    out_codec: sys::Dart_Handle,
    target_width: i32,
    target_height: i32,
    _format: i32,
) {
    guard((), || {
        // SAFETY: d is the receiver's native field.
        let Some((bytes, source)) = (unsafe {
            peer::with::<Descriptor, _>(d, |d| (Arc::clone(&d.bytes), d.source.clone()))
        }) else {
            eprintln!("fl-ui: ImageDescriptor.instantiateCodec: not a live descriptor");
            return;
        };
        let mut codec = CodecInner {
            bytes,
            source,
            target: (target_width.max(0) as u32, target_height.max(0) as u32),
            frames: None,
            frame_count: 1,
            repetition_count: 0,
            state: Mutex::new(State::Idle),
        };
        animate(&mut codec);
        let codec = Arc::new(codec);
        // SAFETY: a Handle-parameter native; out_codec is live.
        if let Err(e) = unsafe { peer::attach(out_codec, codec, 0) } {
            eprintln!("fl-ui: ImageDescriptor.instantiateCodec: {e}");
        }
    });
}

/// Makes `codec` an animation's if its bytes are a GIF or WebP of more
/// than one frame: frames counted, none decoded. An animation over the
/// frame limit has no frames and answers every call with why, as the
/// engine's codec does with none. Anything else stays a still, its errors
/// its decode's.
fn animate(codec: &mut CodecInner) {
    let animated = matches!(
        fl_image::format_of(&codec.bytes),
        Some(fl_image::Format::Gif | fl_image::Format::Webp)
    );
    if !matches!(codec.source, Source::Encoded) || !animated {
        return;
    }
    match Frames::new(Arc::clone(&codec.bytes), limits()) {
        Ok(f) if f.frame_count() > 1 => {
            codec.frame_count = f.frame_count();
            codec.repetition_count = f.repetition_count();
            codec.frames = Some(Arc::new(Mutex::new(f)));
        }
        Err(e @ fl_image::ImageError::TooManyFrames { .. }) => {
            codec.frame_count = 0;
            codec.state = Mutex::new(State::Failed(e.to_string()));
        }
        Ok(_) | Err(_) => {}
    }
}

/// `Int32 Function(Pointer<Void>)`, a leaf call.
pub(crate) extern "C" fn codec_frame_count(c: *mut c_void) -> i32 {
    // SAFETY: c is the receiver's native field.
    guard(0, || {
        unsafe { peer::with::<Arc<CodecInner>, _>(c, |c| c.frame_count) }
            .map_or(0, |n| i32::try_from(n).unwrap_or(i32::MAX))
    })
}

/// `Int32 Function(Pointer<Void>)`, a leaf call: -1 for forever.
pub(crate) extern "C" fn codec_repetition_count(c: *mut c_void) -> i32 {
    // SAFETY: c is the receiver's native field.
    guard(0, || {
        unsafe { peer::with::<Arc<CodecInner>, _>(c, |c| c.repetition_count) }.unwrap_or(0)
    })
}

/// `Handle Function(Pointer<Void>, Handle callback)`. The callback is called
/// later with the image, 0 and "", or null, 0 and why; every call is
/// answered (brief M3 §15).
pub(crate) extern "C" fn codec_get_next_frame(
    c: *mut c_void,
    callback: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: as descriptor_init_encoded; c is the receiver's field.
        unsafe {
            if !sys::Dart_IsClosure(callback) {
                return error("Callback must be a function");
            }
            let Some(codec) = peer::with::<Arc<CodecInner>, _>(c, |c| Arc::clone(c)) else {
                return error("Codec has been disposed");
            };
            if let Some(frames) = &codec.frames {
                submit(&codec, Source::Next(Arc::clone(frames)), callback);
                return null;
            }
            let mut state = codec.state.lock().unwrap_or_else(|e| e.into_inner());
            match &*state {
                State::Ready(..) | State::Failed(_) => queue_reply(callback, state.answer()),
                State::Decoding(id) => {
                    if let Some(p) = PENDING
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .as_mut()
                        .and_then(|m| m.get_mut(id))
                    {
                        p.callbacks.push(Persistent::new(callback));
                    }
                }
                State::Idle => {
                    let source = codec.source.clone();
                    if let Some(id) = submit(&codec, source, callback) {
                        *state = State::Decoding(id);
                    } else {
                        *state = State::Failed(NOT_RUNNING.into());
                    }
                }
            }
            null
        }
    })
}

const NOT_RUNNING: &str = "the decode thread is not running";

/// Queues a decode of `codec`'s bytes as `source` says, `callback` waiting
/// on it: its id, or `None`, the callback answered with why, if the
/// worker is not running.
///
/// # Safety
/// The isolate is current with an API scope open; `callback` is live.
unsafe fn submit(
    codec: &Arc<CodecInner>,
    source: Source,
    callback: sys::Dart_Handle,
) -> Option<u64> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let job = Job {
        id,
        tag: GENERATION.load(Ordering::Acquire),
        bytes: Arc::clone(&codec.bytes),
        source,
        target: Some(codec.target).filter(|(w, h)| *w > 0 && *h > 0),
        limits: limits(),
    };
    // Registered before the job can finish, so its result always finds
    // its callback.
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(
            id,
            Pending {
                // SAFETY: per this function's contract.
                callbacks: vec![unsafe { Persistent::new(callback) }],
                codec: Arc::downgrade(codec),
            },
        );
    if worker().is_some_and(|w| w.submit(job)) {
        return Some(id);
    }
    let p = PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .and_then(|m| m.remove(&id));
    for cb in p.into_iter().flat_map(|p| p.callbacks) {
        queue_persistent(cb, ReplyArg::FrameError(NOT_RUNNING.into()));
    }
    None
}

/// `Void Function(Pointer<Void>)`. A decode in flight still answers its
/// callbacks; only the cached frame goes.
pub(crate) extern "C" fn codec_dispose(c: *mut c_void) {
    guard((), || {
        // SAFETY: c is the receiver's native field.
        let _ = unsafe { peer::take::<Arc<CodecInner>>(c) };
    });
}

/// A new `_Image` wrapping `image`, as the engine makes one: dart:ui's
/// private constructor, then the peer.
///
/// # Safety
/// The isolate is current with an API scope open.
pub(crate) unsafe fn new_image(image: &Arc<ImageData>) -> Result<sys::Dart_Handle, String> {
    // SAFETY: per this function's contract.
    unsafe {
        let ui = sys::Dart_LookupLibrary(sys::Dart_NewStringFromCString(c"dart:ui".as_ptr()));
        let class = sys::Dart_NewStringFromCString(c"_Image".as_ptr());
        let ty = sys::Dart_GetNonNullableType(ui, class, 0, std::ptr::null_mut());
        if sys::Dart_IsError(ty) {
            return Err("dart:ui has no _Image".into());
        }
        let ctor = sys::Dart_NewStringFromCString(c"_".as_ptr());
        let obj = sys::Dart_New(ty, ctor, 0, std::ptr::null_mut());
        if sys::Dart_IsError(obj) {
            return Err("_Image._() failed".into());
        }
        peer::attach(obj, Arc::clone(image), image.size())?;
        Ok(obj)
    }
}

/// The image behind an `Image` argument's native field.
///
/// # Safety
/// `raw` is the native field of an argument of the current call.
pub(crate) unsafe fn image_of(raw: *mut c_void) -> Option<Arc<ImageData>> {
    // SAFETY: per this function's contract.
    unsafe { peer::with::<Arc<ImageData>, _>(raw, |i| Arc::clone(i)) }
}

/// `Int32 Function(Pointer<Void>)`, a leaf call.
pub(crate) extern "C" fn image_width(i: *mut c_void) -> i32 {
    // SAFETY: i is the receiver's native field.
    guard(0, || unsafe { image_of(i) }.map_or(0, |i| i.width as i32))
}

/// `Int32 Function(Pointer<Void>)`, a leaf call.
pub(crate) extern "C" fn image_height(i: *mut c_void) -> i32 {
    // SAFETY: i is the receiver's native field.
    guard(0, || unsafe { image_of(i) }.map_or(0, |i| i.height as i32))
}

/// `Int32 Function(Pointer<Void>)`: 0, sRGB.
pub(crate) extern "C" fn image_color_space(_i: *mut c_void) -> i32 {
    0
}

/// `Handle Function(Pointer<Void>, Int32 format, Handle callback)`: an
/// error, thrown by dart:ui. GPU readback to Dart is not in M3 (brief §1).
pub(crate) extern "C" fn image_to_byte_data(
    _i: *mut c_void,
    _format: i32,
    _callback: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    unsafe { error("Image.toByteData is not supported by fl-engine yet") }
}

/// `Void Function(Pointer<Void>)`.
pub(crate) extern "C" fn image_dispose(i: *mut c_void) {
    guard((), || {
        // SAFETY: i is the receiver's native field.
        let _ = unsafe { peer::take::<Arc<ImageData>>(i) };
    });
}

/// `Void Function(Handle)`: an empty shader, given its image next.
pub(crate) extern "C" fn image_shader_create(wrapper: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: a Handle-parameter native: isolate current, scope open.
        if let Err(e) = unsafe { peer::attach(wrapper, Option::<Arc<ImageShader>>::None, 0) } {
            eprintln!("fl-ui: ImageShader::Create: {e}");
        }
    });
}

/// `Handle Function(Pointer<Void> this, Pointer<Void> image, Int32 tmx,
/// Int32 tmy, Int32 filterQuality, Handle Float64List matrix4)`.
pub(crate) extern "C" fn image_shader_init(
    s: *mut c_void,
    image: *mut c_void,
    tmx: i32,
    tmy: i32,
    quality: i32,
    matrix: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: image and s are native fields of this call's arguments;
        // matrix is a live handle.
        unsafe {
            let Some(image) = image_of(image) else {
                return error("ImageShader: the image is disposed");
            };
            let Some(m) = args::f64s(matrix).and_then(|m| <[f64; 16]>::try_from(m).ok()) else {
                return error("ImageShader: matrix4 must have 16 entries");
            };
            let shader = ImageShader {
                image,
                tile: [tmx, tmy],
                quality: (quality >= 0).then_some(quality),
                matrix: m,
            };
            if peer::with::<Option<Arc<ImageShader>>, _>(s, |v| *v = Some(Arc::new(shader)))
                .is_none()
            {
                return error("ImageShader: not a live shader");
            }
            null
        }
    })
}

/// `Void Function(Pointer<Void>)`.
pub(crate) extern "C" fn image_shader_dispose(s: *mut c_void) {
    guard((), || {
        // SAFETY: s is the receiver's native field.
        let _ = unsafe { peer::take::<Option<Arc<ImageShader>>>(s) };
    });
}

/// The image shader a paint's objects list holds as its shader, if that is
/// what it holds.
///
/// # Safety
/// The isolate is current with an API scope open; `shader` is a live handle.
pub(crate) unsafe fn image_shader_of(shader: sys::Dart_Handle) -> Option<Arc<ImageShader>> {
    // SAFETY: per this function's contract.
    unsafe {
        let field = peer::field(shader)?;
        peer::with::<Option<Arc<ImageShader>>, _>(field, |s| s.clone()).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_data_is_equal_only_to_itself_and_gives_its_pixels_once() {
        let a = ImageData::new(1, 1, vec![1, 2, 3, 4]);
        let b = ImageData::new(1, 1, vec![1, 2, 3, 4]);
        assert!(a == a && a != b);
        assert_eq!(a.take_pixels(), Some(vec![1, 2, 3, 4]));
        assert_eq!(a.take_pixels(), None);
    }

    /// Exit criterion 1: nothing here decodes; only the worker does. The
    /// UI thread reads headers.
    #[test]
    fn the_ui_thread_never_decodes() {
        let src = include_str!("image.rs");
        // The code, not these tests, which name what they look for.
        let code = src.split("#[cfg(test)]").next().unwrap_or(src);
        for line in code.lines().filter(|l| !l.trim_start().starts_with("//")) {
            if line.contains("fl_image::") {
                assert!(
                    !line.contains("decode(") && !line.contains("from_raw("),
                    "a decode on the UI thread: {line}"
                );
            }
        }
    }

    #[test]
    fn a_failed_codec_answers_with_why() {
        assert!(matches!(
            State::Failed("bad".into()).answer(),
            ReplyArg::FrameError(e) if e == "bad"
        ));
    }
}
