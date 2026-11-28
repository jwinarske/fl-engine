//! `FragmentProgram` and `FragmentShader`: a `.frag` asset loaded, and the
//! uniforms an app sets on one, as `lib/ui/painting/fragment_program.cc` and
//! `fragment_shader.cc` keep them.
//!
//! The stage read is the asset's Vulkan one, on either of emblema's backends:
//! it is SPIR-V, which the raster side translates for GLES as well, so one
//! stage gives both the same uniforms. Its uniform list is the engine's on
//! Vulkan, where the non-sampler uniforms are one `Struct` with named fields.
//!
//! Drawing with a fragment shader is not wired up yet: a paint holding one
//! draws as if it held no shader.

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::sync::Arc;

use fl_dart_ffi::{args, guard, peer, sys};

use crate::buffer::load_asset;
use crate::image::{image_of, ImageData};
use crate::runtime_stage::{self, Backend, Stage, UniformType};

/// The most floats a program's uniform block may take: emblema's
/// `RUNTIME_FLOATS`, the paint block every effect shares. fl-raster checks
/// the two agree.
pub const RUNTIME_FLOATS: usize = 64;

/// The most images a program may sample: emblema's `MAX_EFFECT_TEXTURES`.
/// fl-raster checks the two agree.
pub const RUNTIME_IMAGES: usize = 4;

/// The stage every program is read from.
const BACKEND: Backend = Backend::Vulkan;

/// A loaded program: its asset key and the stage the raster side draws.
#[derive(Debug)]
pub struct Program {
    // Read when the raster side registers it, once per asset (plan step 4).
    #[allow(dead_code)]
    pub asset: String,
    pub(crate) stage: Stage,
}

/// The `FragmentProgram` peer: the object itself, weakly, so that
/// `initFromAsset` (which is handed only the native field) can set the
/// fields dart:ui reads back; and the program once loaded.
struct ProgramPeer {
    wrapper: Weak,
    program: Option<Arc<Program>>,
}

/// A weak persistent handle to a Dart object, freed when the object is.
struct Weak(sys::Dart_WeakPersistentHandle);

// SAFETY: the handle is only dereferenced by natives on the mutator thread
// whose receiver is the object it refers to; the VM deletes it (through
// `delete_weak`) on whichever thread finalizes.
unsafe impl Send for Weak {}

impl Weak {
    /// # Safety
    /// The isolate is current with an API scope open, and `object` is a
    /// live handle in it.
    unsafe fn new(object: sys::Dart_Handle) -> Option<Weak> {
        // The finalizer is given the slot the handle is stored in, since it
        // is the handle that has to be deleted.
        let slot: *mut sys::Dart_WeakPersistentHandle =
            Box::into_raw(Box::new(std::ptr::null_mut()));
        // SAFETY: per this function's contract; slot is a live allocation.
        unsafe {
            let h = sys::Dart_NewWeakPersistentHandle(object, slot.cast(), 0, Some(delete_weak));
            if h.is_null() {
                drop(Box::from_raw(slot));
                return None;
            }
            *slot = h;
            Some(Weak(h))
        }
    }

    /// The object, while a native of its is running.
    ///
    /// # Safety
    /// The isolate is current with an API scope open, and the object is
    /// alive (the receiver of the current call).
    unsafe fn get(&self) -> sys::Dart_Handle {
        // SAFETY: per this function's contract.
        unsafe { sys::Dart_HandleFromWeakPersistent(self.0) }
    }
}

/// Deletes a [`Weak`]'s handle once its object is collected, as the VM
/// allows a finalizer to.
unsafe extern "C" fn delete_weak(_: *mut c_void, slot: *mut c_void) {
    // SAFETY: slot is the Box Weak::new leaked for this handle, and the VM
    // calls this once.
    unsafe {
        let slot = Box::from_raw(slot.cast::<sys::Dart_WeakPersistentHandle>());
        sys::Dart_DeleteWeakPersistentHandle(*slot);
    }
}

/// A shader's uniform floats: the memory its Dart `Float32List` views, so
/// `setFloat` writes here directly, as the engine's external list does. The
/// floats come first and each sampler's width and height after them.
struct Floats(Box<[UnsafeCell<f32>]>);

// SAFETY: Dart writes the floats only on the mutator thread, and natives
// read them only there, during a call; other threads only drop the
// allocation, which the list's finalizer and the shader's peer share
// through an Arc.
unsafe impl Send for Floats {}
// SAFETY: as Send.
unsafe impl Sync for Floats {}

impl Floats {
    fn new(len: usize) -> Floats {
        Floats((0..len).map(|_| UnsafeCell::new(0.0)).collect())
    }

    fn set(&self, i: usize, v: f32) {
        if let Some(cell) = self.0.get(i) {
            // SAFETY: on the mutator thread, during a native call, so Dart
            // is not writing the list at the same time.
            unsafe { *cell.get() = v };
        }
    }
}

/// The `FragmentShader` peer.
struct ShaderPeer {
    // Read when a paint draws with the shader (plan step 4).
    #[allow(dead_code)]
    program: Arc<Program>,
    floats: Arc<Floats>,
    float_count: usize,
    samplers: Vec<Option<Sampler>>,
}

/// An image a shader samples, and the filter quality it was set with.
// Read when images are drawn (plan step 6).
#[allow(dead_code)]
struct Sampler {
    image: Arc<ImageData>,
    quality: i32,
}

/// A native's error result: a Dart String.
///
/// # Safety
/// The isolate is current with an API scope open.
unsafe fn string(msg: &str) -> sys::Dart_Handle {
    // SAFETY: per this function's contract.
    unsafe { sys::Dart_NewStringFromUTF8(msg.as_ptr(), msg.len() as isize) }
}

/// Throws `msg`, as `Dart_ThrowException(tonic::ToDart(msg))` does.
///
/// # Safety
/// As [`string`], in a native called from Dart.
unsafe fn throw(msg: &str) {
    // SAFETY: per this function's contract.
    unsafe {
        sys::Dart_ThrowException(string(msg));
    }
}

/// `Void Function(Handle)`.
pub(crate) extern "C" fn program_create(wrapper: sys::Dart_Handle) {
    guard((), || {
        // SAFETY: a Handle-parameter native: isolate current, scope open.
        unsafe {
            let Some(weak) = Weak::new(wrapper) else {
                eprintln!("fl-ui: FragmentProgram::Create: no weak handle");
                return;
            };
            let p = ProgramPeer {
                wrapper: weak,
                program: None,
            };
            if let Err(e) = peer::attach(wrapper, p, 0) {
                eprintln!("fl-ui: FragmentProgram::Create: {e}");
            }
        }
    });
}

/// What `initFromAsset` sets on the object, before it is set.
#[derive(Debug)]
struct Loaded {
    program: Program,
    float_count: usize,
    sampler_count: usize,
}

/// Loads `key` as `initFromAsset` does, or the engine's message for why
/// not. Also refuses a program emblema cannot run: more floats or images
/// than an effect may have.
fn load(key: &str) -> Result<Loaded, String> {
    let Some(bytes) = load_asset(key) else {
        return Err(format!("Asset '{key}' not found"));
    };
    let mut stages = runtime_stage::decode(&bytes)
        .map_err(|e| format!("Asset '{key}' manifest could not be decoded: {e}"))?;
    if stages.is_empty() {
        return Err(format!("Asset '{key}' does not contain any shader data."));
    }
    let Some(stage) = stages.take(BACKEND) else {
        let mut found = String::new();
        for b in stages.found() {
            found.push_str(b.name());
            found.push(' ');
        }
        return Err(format!(
            "Asset '{key}' does not contain appropriate runtime stage data for current \
             backend ({}).\nFound stages: {found}",
            BACKEND.name()
        ));
    };
    let mut samplers = 0usize;
    let mut dart_bytes = 0u64;
    let mut block_bytes = 0u64;
    for u in &stage.uniforms {
        if u.kind == UniformType::SampledImage {
            samplers += 1;
        } else {
            dart_bytes = dart_bytes.saturating_add(u.dart_size());
            block_bytes = block_bytes.saturating_add(u.gpu_size());
        }
    }
    if stage.shader.is_empty() {
        return Err(format!("Asset '{key}' does not contain any shader data."));
    }
    // D3: refused here, where an app can see why, not at its first draw.
    let block_floats = block_bytes.div_ceil(4);
    if block_floats > RUNTIME_FLOATS as u64 {
        return Err(format!(
            "Asset '{key}' needs {block_floats} floats of uniforms; fl-engine runs at most \
             {RUNTIME_FLOATS}."
        ));
    }
    if samplers > RUNTIME_IMAGES {
        return Err(format!(
            "Asset '{key}' samples {samplers} images; fl-engine runs at most {RUNTIME_IMAGES}."
        ));
    }
    Ok(Loaded {
        program: Program {
            asset: key.to_owned(),
            stage,
        },
        float_count: usize::try_from(dart_bytes.div_ceil(4)).unwrap_or(usize::MAX),
        sampler_count: samplers,
    })
}

/// `_uniformInfo`: one `Map<String, Object>` per uniform, with the keys the
/// engine's `ConvertUniformDescriptionToMap` gives.
///
/// # Safety
/// The isolate is current with an API scope open.
unsafe fn uniform_info(stage: &Stage) -> Result<sys::Dart_Handle, String> {
    let check = |h: sys::Dart_Handle| -> Result<sys::Dart_Handle, String> {
        // SAFETY: per this function's contract.
        if unsafe { sys::Dart_IsError(h) } {
            Err("Failed to set uniform info for fragment program.".to_owned())
        } else {
            Ok(h)
        }
    };
    // SAFETY: per this function's contract; every handle is checked.
    unsafe {
        let list = check(sys::Dart_NewList(stage.uniforms.len() as isize))?;
        for (i, u) in stage.uniforms.iter().enumerate() {
            let keys = check(sys::Dart_NewList(5))?;
            let values = check(sys::Dart_NewList(5))?;
            let names = check(sys::Dart_NewList(u.struct_fields.len() as isize))?;
            let sizes = check(sys::Dart_NewList(u.struct_fields.len() as isize))?;
            for (k, f) in u.struct_fields.iter().enumerate() {
                check(sys::Dart_ListSetAt(names, k as isize, string(&f.name)))?;
                let n = i64::try_from(f.byte_size).unwrap_or(i64::MAX);
                check(sys::Dart_ListSetAt(
                    sizes,
                    k as isize,
                    sys::Dart_NewInteger(n),
                ))?;
            }
            let size = i64::try_from(u.dart_size()).unwrap_or(i64::MAX);
            let entries: [(&str, sys::Dart_Handle); 5] = [
                ("name", string(&u.name)),
                ("type", string(u.kind.name())),
                ("size", sys::Dart_NewInteger(size)),
                ("struct_field_names", names),
                ("struct_field_bytes", sizes),
            ];
            for (k, (key, value)) in entries.into_iter().enumerate() {
                check(sys::Dart_ListSetAt(keys, k as isize, string(key)))?;
                check(sys::Dart_ListSetAt(values, k as isize, value))?;
            }
            let map = check(sys::Dart_NewMap(
                sys::Dart_TypeString(),
                keys,
                sys::Dart_TypeObject(),
                values,
            ))?;
            check(sys::Dart_ListSetAt(list, i as isize, map))?;
        }
        Ok(list)
    }
}

/// `Handle Function(Pointer<Void> this, Handle String assetKey)`: "" when
/// loaded, else why not, which dart:ui throws.
pub(crate) extern "C" fn program_init_from_asset(
    p: *mut c_void,
    key: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: p is the receiver's native field; key is a live handle.
        unsafe {
            let Some(key) = args::string(key) else {
                return string("Asset key must be a String");
            };
            let loaded = match load(&key) {
                Ok(l) => l,
                Err(e) => return string(&e),
            };
            let info = match uniform_info(&loaded.program.stage) {
                Ok(h) => h,
                Err(e) => return string(&e),
            };
            let Some(Some(this)) = peer::with::<ProgramPeer, _>(p, |pp| {
                let h = pp.wrapper.get();
                (!sys::Dart_IsNull(h) && !sys::Dart_IsError(h)).then_some(h)
            }) else {
                return string("FragmentProgram: not a live program");
            };
            let set = |field: &str, value: sys::Dart_Handle| {
                !sys::Dart_IsError(sys::Dart_SetField(this, string(field), value))
            };
            if !set(
                "_samplerCount",
                sys::Dart_NewInteger(loaded.sampler_count as i64),
            ) {
                return string("Failed to set sampler count for fragment program.");
            }
            if !set(
                "_uniformFloatCount",
                sys::Dart_NewInteger(loaded.float_count as i64),
            ) {
                return string("Failed to set uniform float count for fragment program.");
            }
            if !set("_uniformInfo", info) {
                return string("Failed to set uniform info for fragment program.");
            }
            let program = Arc::new(loaded.program);
            peer::with::<ProgramPeer, _>(p, |pp| pp.program = Some(program));
            string("")
        }
    })
}

/// The `Float32List`'s finalizer: drops its share of the floats.
unsafe extern "C" fn release_floats(_: *mut c_void, floats: *mut c_void) {
    // SAFETY: floats is the Arc::into_raw shader_create made for the list,
    // and the VM calls this once.
    drop(unsafe { Arc::from_raw(floats.cast::<Floats>()) });
}

/// `Handle Function(Handle this, Handle program, Handle floatCount,
/// Handle samplerCount)`: the `Float32List` `setFloat` writes into.
pub(crate) extern "C" fn shader_create(
    wrapper: sys::Dart_Handle,
    program: sys::Dart_Handle,
    float_count: sys::Dart_Handle,
    sampler_count: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native: isolate current, scope open.
    let null = unsafe { args::null() };
    guard(null, || {
        // SAFETY: a Handle-parameter native; the four are live handles.
        unsafe {
            let int = |h: sys::Dart_Handle| {
                let mut v: i64 = 0;
                (!sys::Dart_IsError(sys::Dart_IntegerToInt64(h, &mut v)))
                    .then(|| usize::try_from(v).ok())
                    .flatten()
            };
            let (Some(floats_n), Some(samplers_n)) = (int(float_count), int(sampler_count)) else {
                return sys::Dart_NewApiError(c"FragmentShader: counts must be integers".as_ptr());
            };
            let Some(program) = peer::field(program)
                .and_then(|f| peer::with::<ProgramPeer, _>(f, |pp| pp.program.clone()))
                .flatten()
            else {
                return sys::Dart_NewApiError(
                    c"FragmentShader: the program is not loaded".as_ptr(),
                );
            };
            // dart:ui passes the program's own counts; anything larger
            // than it loaded with is not from dart:ui.
            if floats_n > RUNTIME_FLOATS || samplers_n > RUNTIME_IMAGES {
                return sys::Dart_NewApiError(c"FragmentShader: too many uniforms".as_ptr());
            }
            let floats = Arc::new(Floats::new(floats_n + 2 * samplers_n));
            let data = floats.0.as_ptr().cast_mut().cast::<c_void>();
            let shared = Arc::into_raw(Arc::clone(&floats))
                .cast_mut()
                .cast::<c_void>();
            let list = sys::Dart_NewExternalTypedDataWithFinalizer(
                sys::Dart_TypedData_Type::Dart_TypedData_kFloat32,
                data,
                floats_n as isize,
                shared,
                (floats.0.len() * 4) as isize,
                Some(release_floats),
            );
            if sys::Dart_IsError(list) {
                drop(Arc::from_raw(shared.cast::<Floats>()));
                return list;
            }
            let s = ShaderPeer {
                program,
                floats,
                float_count: floats_n,
                samplers: (0..samplers_n).map(|_| None).collect(),
            };
            // Hot reload constructs again on the same object
            // (`_reinitialize`): a live peer is replaced, not a second one
            // attached beside it.
            let mut s = Some(s);
            if let Some(f) = peer::field(wrapper) {
                peer::with::<ShaderPeer, _>(f, |old| {
                    if let Some(new) = s.take() {
                        *old = new;
                    }
                });
            }
            if let Some(s) = s {
                if let Err(e) = peer::attach(wrapper, s, 0) {
                    eprintln!("fl-ui: ReusableFragmentShader::Create: {e}");
                }
            }
            list
        }
    })
}

/// `Void Function(Pointer<Void> this, Handle index, Handle image,
/// Int32 filterQuality)`.
pub(crate) extern "C" fn shader_set_image_sampler(
    s: *mut c_void,
    index: sys::Dart_Handle,
    image: sys::Dart_Handle,
    quality: i32,
) {
    guard((), || {
        // SAFETY: s is the receiver's native field; index and image are
        // live handles.
        unsafe {
            let mut i: i64 = 0;
            if sys::Dart_IsError(sys::Dart_IntegerToInt64(index, &mut i)) {
                return throw("Sampler index out of bounds");
            }
            let Some(img) = peer::field(image).and_then(|f| image_of(f)) else {
                return throw("Image has been disposed");
            };
            let ok = peer::with::<ShaderPeer, _>(s, |sp| {
                let Some(slot) = usize::try_from(i).ok().filter(|i| *i < sp.samplers.len()) else {
                    return false;
                };
                let at = sp.float_count + 2 * slot;
                sp.floats.set(at, img.width as f32);
                sp.floats.set(at + 1, img.height as f32);
                sp.samplers[slot] = Some(Sampler {
                    image: img,
                    quality,
                });
                true
            });
            if ok != Some(true) {
                throw("Sampler index out of bounds");
            }
        }
    });
}

/// `Bool Function(Pointer<Void>)`: every sampler set.
pub(crate) extern "C" fn shader_validate_samplers(s: *mut c_void) -> bool {
    guard(false, || {
        // SAFETY: s is the receiver's native field.
        unsafe { peer::with::<ShaderPeer, _>(s, |sp| sp.samplers.iter().all(Option::is_some)) }
            .unwrap_or(false)
    })
}

/// `Bool Function(Pointer<Void>)`: a sampler for the filtered layer, and
/// every sampler after it set.
pub(crate) extern "C" fn shader_validate_image_filter(s: *mut c_void) -> bool {
    guard(false, || {
        // SAFETY: s is the receiver's native field.
        unsafe {
            peer::with::<ShaderPeer, _>(s, |sp| {
                !sp.samplers.is_empty() && sp.samplers[1..].iter().all(Option::is_some)
            })
        }
        .unwrap_or(false)
    })
}

/// `Void Function(Pointer<Void>)`.
pub(crate) extern "C" fn shader_dispose(s: *mut c_void) {
    guard((), || {
        // SAFETY: s is the receiver's native field.
        let _ = unsafe { peer::take::<ShaderPeer>(s) };
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::sync::Arc;

    /// The bundle is one per process, so the tests that set it take turns.
    fn with_fixtures<R>(f: impl FnOnce() -> R) -> R {
        static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let bundle = fl_assets::Bundle::open(&dir).unwrap();
        crate::buffer::set_asset_bundle(Some(Arc::new(bundle)));
        let r = f();
        crate::buffer::set_asset_bundle(None);
        r
    }

    #[test]
    fn a_program_loads_with_the_engines_counts() {
        with_fixtures(|| {
            let time = load("shaders/time.iplr").unwrap();
            assert_eq!((time.float_count, time.sampler_count), (3, 0));
            let layout = load("shaders/layout.iplr").unwrap();
            assert_eq!((layout.float_count, layout.sampler_count), (22, 2));
            let ink = load("shaders/ink_sparkle.iplr").unwrap();
            assert_eq!((ink.float_count, ink.sampler_count), (28, 0));
            let stretch = load("shaders/stretch_effect.iplr").unwrap();
            assert_eq!((stretch.float_count, stretch.sampler_count), (6, 1));
        });
    }

    #[test]
    fn a_missing_asset_is_the_engines_message() {
        with_fixtures(|| {
            assert_eq!(
                load("shaders/nope.frag").unwrap_err(),
                "Asset 'shaders/nope.frag' not found"
            );
            let e = load("shaders/layout.frag").unwrap_err();
            assert!(
                e.starts_with(
                    "Asset 'shaders/layout.frag' manifest could not be decoded: INVALID_ARGUMENT:"
                ),
                "{e}"
            );
        });
    }
}
