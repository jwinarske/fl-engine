//! Reading native arguments.

use crate::sys;

/// The UTF-8 of a Dart `String`, or `None` for null or a non-string.
///
/// # Safety
/// The isolate is current with an API scope open, and `h` is a live handle.
pub unsafe fn string(h: sys::Dart_Handle) -> Option<String> {
    // SAFETY: per this function's contract.
    unsafe {
        if sys::Dart_IsNull(h) || !sys::Dart_IsString(h) {
            return None;
        }
        let mut p: *mut u8 = std::ptr::null_mut();
        let mut len: isize = 0;
        if sys::Dart_IsError(sys::Dart_StringToUTF8(h, &mut p, &mut len)) || p.is_null() {
            return None;
        }
        // The VM owns the bytes for the scope's lifetime; copy them out.
        let bytes = std::slice::from_raw_parts(p, len as usize);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// Dart `null`, for natives that return a `Handle`.
///
/// # Safety
/// The isolate is current.
pub unsafe fn null() -> sys::Dart_Handle {
    // SAFETY: per this function's contract.
    unsafe { sys::Dart_Null() }
}
