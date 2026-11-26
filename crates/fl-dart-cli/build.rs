//! `DynamicLibrary.process()` resolves symbols with `dlsym` against the
//! running executable, which only sees the executable's own symbols if they
//! are in its dynamic symbol table. Executables do not export by default.
//! Exported are the `fl_dart_*` FFI functions and the `FlutterDesktop*`
//! C API a plugin's library calls (fl-ui's texture registrar) alone
//! (`--export-dynamic-symbol`, GNU ld 2.35 and lld), not every global
//! symbol as `-rdynamic` exports them: an exported symbol is a root the
//! linker's gc-sections keeps, so `-rdynamic` kept every library's public
//! functions and the data they reach: 6.8 MB of the 17.9 MB stripped
//! release binary on x86_64 (brief §9, and the §15 visibility item).
//!
//! On riscv64, linker relaxation is off. The VM's objects call through
//! `auipc`+`jalr` pairs (`R_RISCV_CALL_PLT`), which GNU ld relaxes to a
//! `jal` (±1 MiB) when the target looks near. libc++'s `operator new` sits
//! in its own `__lcxx_override` section, placed after `.text`, and GNU ld
//! 2.42 (Ubuntu 24.04) relaxed calls to it that the final layout put out of
//! `jal`'s reach: "relocation truncated to fit: R_RISCV_JAL against symbol
//! `operator new`" linking the debug test binary once fl-text took ICU4X's
//! segmenter. Unrelaxed, a call costs one instruction more; it always links.
fn main() {
    println!("cargo:rustc-link-arg-bins=-Wl,--export-dynamic-symbol=fl_dart_*");
    println!("cargo:rustc-link-arg-bins=-Wl,--export-dynamic-symbol=FlutterDesktop*");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("riscv64") {
        println!("cargo:rustc-link-arg-bins=-Wl,--no-relax");
    }
}
