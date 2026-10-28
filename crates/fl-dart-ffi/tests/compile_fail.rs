//! A peer is borrowed for the length of one native call (brief M2 §7):
//! `peer::with` lends it to a closure that must take a borrow of any
//! lifetime, so nothing borrowed from it can be returned. These cases must
//! not compile; if one starts to, a native could keep a reference to a
//! value the finalizer or `dispose` is free to drop.
//!
//! They only type-check, never link, so they run without a VM:
//! `FL_DART_SDK_OUT=none cargo test -p fl-dart-ffi --features aot --test compile_fail`.

#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
