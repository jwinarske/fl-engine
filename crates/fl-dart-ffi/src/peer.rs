//! Rust values attached to Dart objects.
//!
//! dart:ui's wrapper classes (`Canvas`, `Picture`, `SceneBuilder`, ...)
//! extend `NativeFieldWrapperClass1`. A native declared with a
//! `Pointer<Void>` parameter for one of them receives the object's native
//! field 0. So the native that constructs the object must set that field,
//! and every native that takes the object reads it back. In C++ that is a
//! convention; here [`attach`] is the only way to set it, and [`with`] and
//! [`take`] the only ways to read it, each checking that the field holds a
//! peer of the type asked for.
//!
//! A peer is freed once: by the object's finalizer when the GC collects it
//! (or its isolate group shuts down). An explicit `dispose` native [`take`]s
//! the value out first; the finalizer then frees an empty peer. Natives that
//! see a taken or mistyped peer get `None`, never a dangling or wrong value.

use std::any::TypeId;
use std::ffi::c_void;
use std::sync::Mutex;

use crate::sys;

/// What field 0 points at. `repr(C)` with the tag first, so the tag can be
/// read through a pointer to a peer of any type before trusting the rest.
#[repr(C)]
struct Peer<T> {
    tag: TypeId,
    // A Mutex, not a Cell: the finalizer may run off the mutator thread.
    value: Mutex<Option<T>>,
}

/// Attaches `value` to `wrapper`, a `NativeFieldWrapperClass1` whose field 0
/// is not yet set. `external_size` is the native memory `value` holds, which
/// the GC weighs when deciding to collect.
///
/// # Safety
/// The isolate is current with an API scope open, as it is in a native with
/// a `Handle` parameter, and `wrapper` is a live handle in it.
pub unsafe fn attach<T: Send + 'static>(
    wrapper: sys::Dart_Handle,
    value: T,
    external_size: usize,
) -> Result<(), String> {
    let raw = Box::into_raw(Box::new(Peer {
        tag: TypeId::of::<T>(),
        value: Mutex::new(Some(value)),
    }));
    // SAFETY: per this function's contract; raw is a live allocation.
    unsafe {
        let set = sys::Dart_SetNativeInstanceField(wrapper, 0, raw as isize);
        if sys::Dart_IsError(set) {
            drop(Box::from_raw(raw));
            return Err(error_text(set));
        }
        let fin = sys::Dart_NewFinalizableHandle(
            wrapper,
            raw as *mut c_void,
            external_size as isize,
            Some(finalize::<T>),
        );
        if fin.is_null() {
            // No finalizer, so nothing would ever free it: undo the field.
            let _ = sys::Dart_SetNativeInstanceField(wrapper, 0, 0);
            drop(Box::from_raw(raw));
            return Err("Dart_NewFinalizableHandle failed".into());
        }
    }
    Ok(())
}

/// Native field 0 of `wrapper`, for a native given the object as a `Handle`
/// rather than as a `Pointer<Void>`: what the VM would have passed, to hand
/// to [`with`] or [`take`]. `None` for null, or for an object with no
/// native fields.
///
/// # Safety
/// The isolate is current with an API scope open, and `wrapper` is a live
/// handle in it.
pub unsafe fn field(wrapper: sys::Dart_Handle) -> Option<*mut c_void> {
    let mut v: isize = 0;
    // SAFETY: per this function's contract; v is a valid out-pointer.
    unsafe {
        if sys::Dart_IsNull(wrapper)
            || sys::Dart_IsError(sys::Dart_GetNativeInstanceField(wrapper, 0, &mut v))
        {
            return None;
        }
    }
    Some(v as *mut c_void)
}

/// Runs `f` on the value attached to the object whose field 0 is `raw`.
/// `None` when `raw` is null, holds a peer of another type, or was taken.
///
/// # Safety
/// `raw` is a `Pointer<Void>` the VM passed to the current native call for a
/// `NativeFieldWrapperClass1` argument: zero, or a field set by [`attach`].
pub unsafe fn with<T: 'static, R>(raw: *mut c_void, f: impl FnOnce(&mut T) -> R) -> Option<R> {
    // SAFETY: per this function's contract.
    let peer = unsafe { peer::<T>(raw) }?;
    let mut value = peer.value.lock().unwrap_or_else(|e| e.into_inner());
    value.as_mut().map(f)
}

/// Takes the value out, for a `dispose` native. Later [`with`] and [`take`]
/// see `None`; the finalizer frees the emptied peer.
///
/// # Safety
/// As [`with`].
pub unsafe fn take<T: 'static>(raw: *mut c_void) -> Option<T> {
    // SAFETY: per this function's contract.
    let peer = unsafe { peer::<T>(raw) }?;
    let mut value = peer.value.lock().unwrap_or_else(|e| e.into_inner());
    value.take()
}

/// # Safety
/// As [`with`]. The object is an argument of the current call, so it, and
/// with it the peer its finalizer frees, outlives the returned reference's
/// use inside that call.
unsafe fn peer<'a, T: 'static>(raw: *mut c_void) -> Option<&'a Peer<T>> {
    if raw.is_null() {
        return None;
    }
    // SAFETY: every peer starts with its tag (repr(C)), whatever its type.
    let tag = unsafe { *(raw as *const TypeId) };
    if tag != TypeId::of::<T>() {
        return None;
    }
    // SAFETY: the tag says this is a Peer<T>, live for the call.
    Some(unsafe { &*(raw as *const Peer<T>) })
}

extern "C" fn finalize<T>(_isolate_callback_data: *mut c_void, peer: *mut c_void) {
    let _ = std::panic::catch_unwind(|| {
        // SAFETY: peer is the Box<Peer<T>> attach leaked, finalized exactly
        // once by the VM.
        drop(unsafe { Box::from_raw(peer as *mut Peer<T>) });
    });
}

/// # Safety
/// `h` is an error handle of the current isolate.
unsafe fn error_text(h: sys::Dart_Handle) -> String {
    // SAFETY: per this function's contract.
    let p = unsafe { sys::Dart_GetError(h) };
    if p.is_null() {
        return "Dart_SetNativeInstanceField failed".into();
    }
    // SAFETY: the VM returns a NUL-terminated string that lives as long as h.
    unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // `with` and `take` read the peer through the pointer a native receives;
    // here the pointer comes from a Box, as attach's does.
    fn raw<T: Send + 'static>(v: T) -> *mut c_void {
        Box::into_raw(Box::new(Peer {
            tag: TypeId::of::<T>(),
            value: Mutex::new(Some(v)),
        })) as *mut c_void
    }

    #[test]
    fn with_reads_the_value_of_its_own_type_only() {
        let p = raw(41u32);
        // SAFETY: p is a live peer, freed below.
        unsafe {
            assert_eq!(
                with::<u32, _>(p, |v| {
                    *v += 1;
                    *v
                }),
                Some(42)
            );
            assert_eq!(with::<i64, _>(p, |v| *v), None, "another type");
            assert_eq!(with::<u32, _>(std::ptr::null_mut(), |v| *v), None);
            finalize::<u32>(std::ptr::null_mut(), p);
        }
    }

    #[test]
    fn take_empties_the_peer_and_the_finalizer_frees_it_once() {
        struct Counted(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for Counted {
            fn drop(&mut self) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let p = raw(Counted(std::sync::Arc::clone(&drops)));
        // SAFETY: p is a live peer until finalize.
        unsafe {
            let taken = take::<Counted>(p);
            assert!(taken.is_some());
            assert!(take::<Counted>(p).is_none(), "taken once");
            assert!(with::<Counted, _>(p, |_| ()).is_none());
            drop(taken);
            assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
            finalize::<Counted>(std::ptr::null_mut(), p);
        }
        assert_eq!(
            drops.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "not again"
        );
    }
}
