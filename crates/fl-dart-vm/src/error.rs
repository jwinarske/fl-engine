//! Errors, and the one place a `Dart_Handle` error becomes a Rust value.

use std::ffi::CStr;
use std::os::raw::c_char;

use fl_dart_sys as sys;

/// Something the VM reported about Dart code or the embedding calls that ran it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DartError {
    /// The embedding API refused a call: wrong state, bad argument, missing
    /// library or member. The string is the VM's own message.
    #[error("Dart API error: {0}")]
    Api(String),
    /// Dart code threw and nothing caught it.
    #[error("unhandled exception: {message}\n{stacktrace}")]
    UnhandledException { message: String, stacktrace: String },
    /// The program could not be loaded or compiled.
    #[error("compilation error: {0}")]
    Compilation(String),
    /// The snapshot was produced by a different SDK than the VM linked here.
    /// A snapshot is valid only for the exact VM build it was made by, and the
    /// VM's own report of this is rarely recognizable as that; this one names
    /// both sides.
    #[error("snapshot version mismatch: this VM expects {expected}, the snapshot is {found}")]
    SnapshotVersionMismatch { expected: String, found: String },
    /// The VM can no longer run this isolate.
    #[error("fatal error: {0}")]
    Fatal(String),
}

/// Failures of the process-wide VM itself, as opposed to the code it runs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VmError {
    /// `Dart_Initialize` has already succeeded in this process. The VM is not
    /// re-initializable after `Dart_Cleanup` either, so this holds for the rest
    /// of the process's life, not only while the first `Vm` is alive.
    #[error("the Dart VM has already been initialized in this process")]
    AlreadyInitialized,
    /// A VM flag was rejected by `Dart_SetVMFlags`.
    #[error("Dart VM flags rejected: {0}")]
    Flags(String),
    /// `Dart_Initialize` failed.
    #[error("Dart_Initialize failed: {0}")]
    Init(String),
}

/// The destination of a `NativePort::post` no longer exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the destination port is closed")]
pub struct PortClosed;

/// Copies a VM-allocated (`malloc`ed) error string and frees it.
///
/// # Safety
/// `p` is null or a NUL-terminated string the caller owns and must `free`.
pub(crate) unsafe fn take_c_error(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no message>".to_owned();
    }
    // SAFETY: non-null and NUL-terminated, per this function's contract.
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    // SAFETY: the VM allocated this with malloc and handed ownership to us.
    unsafe { libc_free(p.cast()) };
    s
}

extern "C" {
    #[link_name = "free"]
    fn libc_free(p: *mut std::ffi::c_void);
}

/// Converts a handle into `Err` if it is an error handle.
///
/// # Safety
/// There is a current isolate with an active API scope, and `h` was produced in
/// it.
pub(crate) unsafe fn check(h: sys::Dart_Handle) -> Result<sys::Dart_Handle, DartError> {
    // SAFETY: the caller's contract is exactly what these calls require.
    unsafe {
        if !sys::Dart_IsError(h) {
            return Ok(h);
        }
        Err(classify(h))
    }
}

/// # Safety
/// As [`check`], and `h` is an error handle.
unsafe fn classify(h: sys::Dart_Handle) -> DartError {
    // SAFETY: per this function's contract.
    unsafe {
        let message = cstr(sys::Dart_GetError(h));
        if sys::Dart_IsUnhandledExceptionError(h) {
            let exc = sys::Dart_ErrorGetException(h);
            let st = sys::Dart_ErrorGetStackTrace(h);
            let message = to_string(exc).unwrap_or(message);
            let stacktrace = to_string(st).unwrap_or_default();
            return DartError::UnhandledException {
                message,
                stacktrace,
            };
        }
        if sys::Dart_IsCompilationError(h) {
            return match crate::snapshot::mismatch_from_message(&message) {
                Some(m) => m,
                None => DartError::Compilation(message),
            };
        }
        if sys::Dart_IsFatalError(h) {
            return DartError::Fatal(message);
        }
        DartError::Api(message)
    }
}

/// `toString()` of a Dart object, if it has one that does not itself throw.
///
/// # Safety
/// As [`check`].
pub(crate) unsafe fn to_string(obj: sys::Dart_Handle) -> Option<String> {
    // SAFETY: per this function's contract.
    unsafe {
        if sys::Dart_IsError(obj) || sys::Dart_IsNull(obj) {
            return None;
        }
        let s = sys::Dart_ToString(obj);
        if sys::Dart_IsError(s) {
            return None;
        }
        string_value(s).ok()
    }
}

/// The contents of a Dart `String`.
///
/// # Safety
/// As [`check`].
pub(crate) unsafe fn string_value(s: sys::Dart_Handle) -> Result<String, DartError> {
    let mut p: *const c_char = std::ptr::null();
    // SAFETY: per this function's contract; `p` is a valid out-pointer.
    unsafe {
        check(sys::Dart_StringToCString(s, &mut p))?;
        Ok(cstr(p))
    }
}

/// # Safety
/// `p` is null or NUL-terminated and valid for the duration of the call.
pub(crate) unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: per this function's contract.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
