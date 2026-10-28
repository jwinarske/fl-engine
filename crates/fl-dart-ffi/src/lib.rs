//! Native bindings for Dart, FFI-first (brief M2 §6): what an embedder's
//! `@Native` functions need, and nothing about any one library.
//!
//! * [`Symbol`] tables, which an [`fl_dart_vm::IsolateConfig::natives`]
//!   resolver searches by name and argument count
//! * [`peer`]: Rust values attached to `NativeFieldWrapperClass1` objects,
//!   the convention dart:ui's `Pointer<Void>` parameters rely on, as a type
//! * [`guard`]: the `catch_unwind` every native body runs under
//! * [`args`]: reading native arguments
//!
//! A native is an `extern "C"` function whose signature matches its Dart
//! declaration: `Handle` is [`sys::Dart_Handle`], `Pointer<Void>` is
//! `*mut c_void`, `Double` is `f64`, `Int64` is `i64`, `Int32` is `i32`,
//! `Bool` is `bool`. The VM calls it on the mutator thread, with the isolate
//! current; one with a `Handle` parameter or result also runs inside an API
//! scope.

use std::ffi::c_void;

pub use fl_dart_vm::sys;

pub mod args;
pub mod peer;

/// One `@Native` symbol and the function that implements it.
#[derive(Debug, Clone, Copy)]
pub struct Symbol {
    /// As the Dart declaration names it, e.g. `"Canvas::drawRect"`.
    pub name: &'static str,
    /// The Dart signature's parameter count, receiver included.
    pub args: usize,
    /// The `extern "C"` function.
    pub func: NativeFn,
}

/// An `extern "C"` function's address. `Sync` so a [`Symbol`] table can be a
/// `static`: a function's address is immutable and valid for the whole
/// process, so sharing it between threads shares nothing mutable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeFn(pub *const ());

// SAFETY: see the type's documentation; it is never dereferenced as data.
unsafe impl Sync for NativeFn {}
// SAFETY: as above.
unsafe impl Send for NativeFn {}

/// `symbol!("Canvas::drawRect", 7, draw_rect)`.
#[macro_export]
macro_rules! symbol {
    ($name:literal, $args:literal, $func:expr) => {
        $crate::Symbol {
            name: $name,
            args: $args,
            func: $crate::NativeFn($func as *const ()),
        }
    };
}

/// The function for `name`, if `table` has it with `args` parameters.
///
/// A name with another argument count resolves to nothing: the Dart
/// declaration and the Rust function disagree about the signature, and
/// calling across that is undefined behavior, where an unresolved symbol is a
/// Dart error naming it.
pub fn lookup(table: &[Symbol], name: &str, args: usize) -> Option<*mut c_void> {
    let s = table.iter().find(|s| s.name == name)?;
    if s.args != args {
        eprintln!(
            "fl-dart-ffi: {name} takes {args} arguments in Dart, {} in Rust; left unresolved",
            s.args
        );
        return None;
    }
    Some(s.func.0 as *mut c_void)
}

/// Runs a native's body, returning `default` if it panics. A panic must not
/// unwind into the VM's frames; the panic hook has already reported it.
pub fn guard<R>(default: R, body: impl FnOnce() -> R) -> R {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn two(_: i64, _: i64) {}

    #[test]
    fn lookup_matches_name_and_arity() {
        let table = [symbol!("A::two", 2, two)];
        assert_eq!(
            lookup(&table, "A::two", 2),
            Some(two as *const () as *mut c_void)
        );
        assert_eq!(lookup(&table, "A::two", 3), None, "arity mismatch");
        assert_eq!(lookup(&table, "A::three", 2), None);
    }

    #[test]
    fn guard_contains_panics() {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let v = guard(7, || -> i32 { std::panic::panic_any("boom") });
        std::panic::set_hook(prev);
        assert_eq!(v, 7);
        assert_eq!(guard(0, || 3), 3);
    }
}
