//! The safety story of this crate is these cases failing to compile. Each is a
//! convention C++ embedders keep by hand and this crate claims to encode as a
//! type; if one of them starts compiling, the claim is false.
//!
//! They only type-check, never link, so they run without a VM:
//! `FL_DART_SDK_OUT=none cargo test -p fl-dart-vm --features aot --test compile_fail`.

#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
