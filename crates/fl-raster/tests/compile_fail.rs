//! Thread placement (brief M2 §5): what holds the device stays on the
//! raster thread. These cases must not compile; if one starts to, the
//! placement is a convention again.
//!
//! They only type-check, never link, so they run without a VM or a GPU:
//! `FL_DART_SDK_OUT=none cargo test -p fl-raster --features aot --test compile_fail`.

#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
