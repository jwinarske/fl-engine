//! Thread placement (brief M4 §6, exit criterion 6): the packet
//! converter's state is the UI thread's. This case must not compile; if it
//! starts to, the placement is a convention again.
//!
//! It only type-checks, never links, so it runs without a VM:
//! `FL_DART_SDK_OUT=none cargo test -p fl-ui --features aot --test compile_fail`.

#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
