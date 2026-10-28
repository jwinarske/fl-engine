//! The process-wide VM.

use std::ffi::CString;
use std::io::Read;
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, Ordering};

use fl_dart_sys as sys;

use crate::error::{take_c_error, VmError};
use crate::isolate;

/// Set by the first `Vm::init` to get past the check, and never cleared:
/// `Dart_Initialize` is once per process, before and after `Dart_Cleanup`.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// How to start the VM.
#[derive(Debug, Clone, Default)]
pub struct VmConfig {
    /// Passed to `Dart_SetVMFlags` before initialization, e.g.
    /// `--enable-asserts`. Empty is the normal case.
    pub flags: Vec<String>,
}

/// The Dart VM. One per process.
///
/// Isolates borrow it, so it cannot be dropped -- and `Dart_Cleanup` cannot
/// run -- while any `Isolate` this program created is still alive.
#[derive(Debug)]
pub struct Vm {
    // Not Send: `Dart_Cleanup` has to run somewhere, and the thread that
    // initialized the VM is the one embedders conventionally use for it.
    _priv: std::marker::PhantomData<*const ()>,
}

impl Vm {
    /// Initializes the VM with the service isolate and the kernel isolate both
    /// off. Neither is part of M1: there is no observatory to attach to and no
    /// source to compile at run time -- every program arrives as a dill or an
    /// AOT snapshot.
    pub fn init(cfg: VmConfig) -> Result<Vm, VmError> {
        if INITIALIZED.swap(true, Ordering::SeqCst) {
            return Err(VmError::AlreadyInitialized);
        }
        // A precompiled runtime must be told it is one: dartaotruntime and the
        // Flutter engine (`kDartPrecompilationArgs` in runtime/dart_vm.cc) both
        // pass --precompilation. Without it the VM runs AOT code with JIT
        // assumptions, and the first native call crashes in LinkNativeCall.
        let mut flags = Vec::with_capacity(cfg.flags.len() + 2);
        if cfg!(feature = "aot") {
            flags.push("--precompilation".to_owned());
        }
        // No timeline recorder (JIT only). The non-product VM defaults to the
        // ring recorder, which keeps metadata for every OS thread the VM ever
        // started and never drops it (`AddTrackMetadataBasedOnThread`,
        // timeline.cc): about 130 bytes per isolate cycle, unbounded. Its
        // events are only readable through the VM service, which is off, so
        // nothing is lost. The product VM (AOT) has no timeline and rejects
        // the flag as unrecognized. Before the caller's flags, so
        // `VmConfig::flags` can still choose a recorder (the last value wins).
        if cfg!(feature = "jit") {
            flags.push("--timeline_recorder=none".to_owned());
        }
        flags.extend(cfg.flags.iter().cloned());
        set_flags(&flags)?;

        // SAFETY: an all-zero Dart_InitializeParams is every callback null and
        // every flag false, which is a valid starting state for the struct.
        let mut params: sys::Dart_InitializeParams = unsafe { std::mem::zeroed() };
        params.version = sys::DART_INITIALIZE_PARAMS_CURRENT_VERSION as i32;
        // No create_group: isolates the VM would create on its own (spawn, the
        // service, the kernel isolate) are refused, which is what M1 wants.
        params.shutdown_isolate = Some(isolate::on_shutdown);
        params.cleanup_isolate = Some(isolate::on_cleanup);
        params.cleanup_group = Some(isolate::on_cleanup_group);
        params.entropy_source = Some(entropy_source);
        params.start_kernel_isolate = false;

        // dart:io's event handler, before the VM -- the order Flutter's
        // DartVM uses. It is what services `Timer` wakeups.
        // SAFETY: once per process, guarded by INITIALIZED above.
        unsafe { sys::fl_dart_io_bootstrap() };

        // SAFETY: params is fully initialized and outlives the call; the
        // callbacks it names are `extern "C"` and never unwind.
        let err = unsafe { sys::Dart_Initialize(&mut params) };
        if !err.is_null() {
            // SAFETY: undoes the bootstrap above; no VM exists to use it.
            unsafe { sys::fl_dart_io_cleanup() };
            // SAFETY: Dart_Initialize returns a malloc'd message we now own.
            return Err(VmError::Init(unsafe { take_c_error(err) }));
        }
        Ok(Vm {
            _priv: std::marker::PhantomData,
        })
    }

    /// `Dart_VersionString()` of the linked VM, e.g.
    /// `3.13.7 (stable) (...) on "linux_x64"`.
    pub fn version_string(&self) -> String {
        // SAFETY: returns a static string; callable once the VM is initialized.
        unsafe { crate::error::cstr(sys::Dart_VersionString()) }
    }
}

impl Drop for Vm {
    fn drop(&mut self) {
        // SAFETY: no Isolate outlives this Vm (they borrow it), so nothing of
        // ours is entered. The VM shuts down anything it still owns itself.
        let err = unsafe { sys::Dart_Cleanup() };
        if !err.is_null() {
            // A failed cleanup has nowhere to go from a Drop; it is reported
            // rather than lost, and the leak lanes are what catch its effect.
            // SAFETY: as in init.
            eprintln!("fl-dart-vm: Dart_Cleanup failed: {}", unsafe {
                take_c_error(err)
            });
        }
        // SAFETY: after Dart_Cleanup, paired with the bootstrap in init.
        unsafe { sys::fl_dart_io_cleanup() };
    }
}

fn set_flags(flags: &[String]) -> Result<(), VmError> {
    // Called even with no flags: Dart_Initialize fails with "VM Flags not
    // initialized" until Dart_SetVMFlags has run once, whatever it was passed.
    // (Confirmed against the linked VM: an empty argv is enough, skipping the
    // call is not.)
    let owned: Vec<CString> = flags
        .iter()
        .map(|f| CString::new(f.as_str()).map_err(|_| VmError::Flags(format!("NUL in flag {f:?}"))))
        .collect::<Result<_, _>>()?;
    let mut argv: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
    // SAFETY: argv points at `owned`, which lives across the call.
    let err = unsafe { sys::Dart_SetVMFlags(argv.len() as i32, argv.as_mut_ptr()) };
    if err.is_null() {
        Ok(())
    } else {
        // SAFETY: a malloc'd message we now own.
        Err(VmError::Flags(unsafe { take_c_error(err) }))
    }
}

/// Hash seeds and `Random.secure()`. The VM treats a `false` return as fatal
/// where it needs the bytes, so this fails only if the kernel's source does.
extern "C" fn entropy_source(buffer: *mut u8, length: isize) -> bool {
    std::panic::catch_unwind(|| {
        let Ok(len) = usize::try_from(length) else {
            return false;
        };
        if buffer.is_null() {
            return false;
        }
        // SAFETY: the VM passes a writable buffer of `length` bytes.
        let out = unsafe { std::slice::from_raw_parts_mut(buffer, len) };
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(out))
            .is_ok()
    })
    .unwrap_or(false)
}
