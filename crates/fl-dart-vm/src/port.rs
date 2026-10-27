//! Rust→Dart from any thread: native ports and `Dart_PostCObject`.
//!
//! This is the one deliberately `Send` type in the crate, and the mechanism
//! the engine's vsync and frame wakeups ride from M2 on. A message is copied
//! into the destination isolate's queue by the VM, so posting needs neither an
//! entered isolate nor a scope -- only a port id, which Dart hands out as
//! `ReceivePort.sendPort.nativePort`.

use std::ffi::CString;

use fl_dart_sys as sys;

use crate::error::PortClosed;

/// A Dart `SendPort`, by id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativePort {
    port: sys::Dart_Port,
}

/// A message, in the subset of `Dart_CObject` M1's tests need. It arrives in
/// Dart as the corresponding core type (`null`, `bool`, `int`, `double`,
/// `String`, `List`).
#[derive(Debug, Clone, PartialEq)]
pub enum CObject {
    Null,
    Bool(bool),
    Int(i64),
    Double(f64),
    String(String),
    Array(Vec<CObject>),
}

impl NativePort {
    /// A port id from Dart. Any value is safe to hold: the VM checks it on
    /// every post, and a port that does not exist is [`PortClosed`].
    pub fn from_raw(port: i64) -> NativePort {
        NativePort { port }
    }

    pub fn raw(&self) -> i64 {
        self.port
    }

    /// Posts `msg` to the port. Callable from any thread, with or without an
    /// isolate entered.
    pub fn post(&self, msg: &CObject) -> Result<(), PortClosed> {
        let mut storage = Storage::default();
        let mut root = storage.lower(msg);
        // SAFETY: root and everything it points to live in `storage` and
        // `root` until the call returns; the VM copies the message.
        if unsafe { sys::Dart_PostCObject(self.port, &mut root) } {
            Ok(())
        } else {
            Err(PortClosed)
        }
    }
}

/// Owns the memory a lowered `Dart_CObject` tree points into.
#[derive(Default)]
struct Storage {
    strings: Vec<CString>,
    // Boxed so that pushing more never moves an array a parent points to.
    arrays: Vec<Box<[sys::Dart_CObject]>>,
    pointers: Vec<Box<[*mut sys::Dart_CObject]>>,
}

impl Storage {
    fn lower(&mut self, msg: &CObject) -> sys::Dart_CObject {
        // SAFETY: an all-zero Dart_CObject is a valid (kNull) value.
        let mut o: sys::Dart_CObject = unsafe { std::mem::zeroed() };
        match msg {
            CObject::Null => o.type_ = sys::Dart_CObject_Type::Dart_CObject_kNull,
            CObject::Bool(b) => {
                o.type_ = sys::Dart_CObject_Type::Dart_CObject_kBool;
                o.value.as_bool = *b;
            }
            CObject::Int(i) => {
                o.type_ = sys::Dart_CObject_Type::Dart_CObject_kInt64;
                o.value.as_int64 = *i;
            }
            CObject::Double(d) => {
                o.type_ = sys::Dart_CObject_Type::Dart_CObject_kDouble;
                o.value.as_double = *d;
            }
            CObject::String(s) => {
                // An interior NUL would truncate; it is replaced instead, so
                // the message still arrives and the difference is visible.
                let c = CString::new(s.replace('\0', "\u{FFFD}")).unwrap_or_default();
                o.type_ = sys::Dart_CObject_Type::Dart_CObject_kString;
                o.value.as_string = c.as_ptr();
                self.strings.push(c);
            }
            CObject::Array(items) => {
                let mut children: Box<[sys::Dart_CObject]> =
                    items.iter().map(|i| self.lower(i)).collect();
                let mut ptrs: Box<[*mut sys::Dart_CObject]> = children
                    .iter_mut()
                    .map(|c| c as *mut sys::Dart_CObject)
                    .collect();
                o.type_ = sys::Dart_CObject_Type::Dart_CObject_kArray;
                o.value.as_array.length = ptrs.len() as isize;
                o.value.as_array.values = ptrs.as_mut_ptr();
                self.arrays.push(children);
                self.pointers.push(ptrs);
            }
        }
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowering_keeps_nested_pointers_valid() {
        let mut st = Storage::default();
        let msg = CObject::Array(vec![
            CObject::Int(7),
            CObject::String("hi".into()),
            CObject::Array(vec![CObject::Bool(true), CObject::Null]),
        ]);
        let root = st.lower(&msg);
        // SAFETY: reading back a tree this test just built and still owns.
        unsafe {
            assert_eq!(root.type_, sys::Dart_CObject_Type::Dart_CObject_kArray);
            let arr = root.value.as_array;
            assert_eq!(arr.length, 3);
            let v = std::slice::from_raw_parts(arr.values, 3);
            assert_eq!((*v[0]).value.as_int64, 7);
            assert_eq!(
                std::ffi::CStr::from_ptr((*v[1]).value.as_string)
                    .to_str()
                    .unwrap(),
                "hi"
            );
            let inner = (*v[2]).value.as_array;
            let iv = std::slice::from_raw_parts(inner.values, 2);
            assert!((*iv[0]).value.as_bool);
            assert_eq!((*iv[1]).type_, sys::Dart_CObject_Type::Dart_CObject_kNull);
        }
    }

    #[test]
    fn native_port_is_send_and_sync() {
        fn ok<T: Send + Sync>() {}
        ok::<NativePort>();
    }
}
