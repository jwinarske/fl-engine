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

/// A copy of the bytes of a typed-data object (`ByteData`, `Uint8List`, any
/// `TypedData`), or `None` for null or anything else.
///
/// # Safety
/// The isolate is current with an API scope open, and `h` is a live handle.
pub unsafe fn bytes(h: sys::Dart_Handle) -> Option<Vec<u8>> {
    let mut ty = sys::Dart_TypedData_Type::Dart_TypedData_kByteData;
    let mut data: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut len: isize = 0;
    // SAFETY: per this function's contract. Between acquire and release no
    // other API call is made; the bytes are copied before release.
    unsafe {
        if sys::Dart_IsNull(h)
            || sys::Dart_IsError(sys::Dart_TypedDataAcquireData(
                h, &mut ty, &mut data, &mut len,
            ))
        {
            return None;
        }
        // `len` counts elements; every element type is a whole number of
        // bytes, and ByteData's element is one.
        let size = match ty {
            sys::Dart_TypedData_Type::Dart_TypedData_kByteData
            | sys::Dart_TypedData_Type::Dart_TypedData_kInt8
            | sys::Dart_TypedData_Type::Dart_TypedData_kUint8
            | sys::Dart_TypedData_Type::Dart_TypedData_kUint8Clamped => 1,
            sys::Dart_TypedData_Type::Dart_TypedData_kInt16
            | sys::Dart_TypedData_Type::Dart_TypedData_kUint16 => 2,
            sys::Dart_TypedData_Type::Dart_TypedData_kInt32
            | sys::Dart_TypedData_Type::Dart_TypedData_kUint32
            | sys::Dart_TypedData_Type::Dart_TypedData_kFloat32 => 4,
            sys::Dart_TypedData_Type::Dart_TypedData_kInt64
            | sys::Dart_TypedData_Type::Dart_TypedData_kUint64
            | sys::Dart_TypedData_Type::Dart_TypedData_kFloat64 => 8,
            _ => 0,
        };
        let out = (size > 0 && !data.is_null() && len >= 0)
            .then(|| std::slice::from_raw_parts(data as *const u8, len as usize * size).to_vec());
        let _ = sys::Dart_TypedDataReleaseData(h);
        out
    }
}

/// The elements of a `Float32List`, or `None` for null or any other type.
///
/// # Safety
/// As [`bytes`].
pub unsafe fn f32s(h: sys::Dart_Handle) -> Option<Vec<f32>> {
    // SAFETY: per this function's contract.
    unsafe { typed::<4>(h, sys::Dart_TypedData_Type::Dart_TypedData_kFloat32) }
        .map(|b| b.iter().map(|c| f32::from_ne_bytes(*c)).collect())
}

/// The elements of a `Float64List`, or `None` for null or any other type.
///
/// # Safety
/// As [`bytes`].
pub unsafe fn f64s(h: sys::Dart_Handle) -> Option<Vec<f64>> {
    // SAFETY: per this function's contract.
    unsafe { typed::<8>(h, sys::Dart_TypedData_Type::Dart_TypedData_kFloat64) }
        .map(|b| b.iter().map(|c| f64::from_ne_bytes(*c)).collect())
}

/// The elements of typed data of type `want`, as `N`-byte chunks.
///
/// # Safety
/// As [`bytes`].
unsafe fn typed<const N: usize>(
    h: sys::Dart_Handle,
    want: sys::Dart_TypedData_Type,
) -> Option<Vec<[u8; N]>> {
    let mut ty = want;
    let mut data: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut len: isize = 0;
    // SAFETY: per this function's contract. Between acquire and release no
    // other API call is made; the elements are copied before release.
    unsafe {
        if sys::Dart_IsNull(h)
            || sys::Dart_IsError(sys::Dart_TypedDataAcquireData(
                h, &mut ty, &mut data, &mut len,
            ))
        {
            return None;
        }
        let out = (ty == want && !data.is_null() && len >= 0).then(|| {
            std::slice::from_raw_parts(data as *const u8, len as usize * N)
                .chunks_exact(N)
                .filter_map(|c| c.try_into().ok())
                .collect()
        });
        let _ = sys::Dart_TypedDataReleaseData(h);
        out
    }
}

/// A new `Float32List` holding `values`, for natives that return one. Dart
/// null if the VM cannot make it.
///
/// # Safety
/// The isolate is current with an API scope open.
pub unsafe fn new_f32s(values: &[f32]) -> sys::Dart_Handle {
    // SAFETY: per this function's contract; the copy is bounded by the
    // length the list was made with.
    unsafe {
        let h = sys::Dart_NewTypedData(
            sys::Dart_TypedData_Type::Dart_TypedData_kFloat32,
            values.len() as isize,
        );
        if sys::Dart_IsError(h) {
            return sys::Dart_Null();
        }
        let mut ty = sys::Dart_TypedData_Type::Dart_TypedData_kFloat32;
        let mut data: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut len: isize = 0;
        if sys::Dart_IsError(sys::Dart_TypedDataAcquireData(
            h, &mut ty, &mut data, &mut len,
        )) || data.is_null()
        {
            return sys::Dart_Null();
        }
        let n = values.len().min(len.max(0) as usize);
        std::ptr::copy_nonoverlapping(values.as_ptr(), data as *mut f32, n);
        let _ = sys::Dart_TypedDataReleaseData(h);
        h
    }
}

/// Whether `h` is a `List` with any element that is not null. Null, or
/// anything that is not a list, is false.
///
/// # Safety
/// As [`bytes`].
pub unsafe fn any_non_null(h: sys::Dart_Handle) -> bool {
    // SAFETY: per this function's contract; indices stay below the length
    // the VM reported.
    unsafe {
        if sys::Dart_IsNull(h) || !sys::Dart_IsList(h) {
            return false;
        }
        let mut len: isize = 0;
        if sys::Dart_IsError(sys::Dart_ListLength(h, &mut len)) {
            return false;
        }
        (0..len).any(|i| {
            let e = sys::Dart_ListGetAt(h, i);
            !sys::Dart_IsError(e) && !sys::Dart_IsNull(e)
        })
    }
}
