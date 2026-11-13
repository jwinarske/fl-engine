//! `DynamicLibrary.process()` resolves symbols with `dlsym` against the
//! running executable, which only sees the executable's own symbols if they
//! are in its dynamic symbol table. Executables do not export by default.
//! Exported are the `fl_dart_*` FFI functions alone
//! (`--export-dynamic-symbol`, GNU ld 2.35 and lld), not every global
//! symbol as `-rdynamic` exports them: an exported symbol is a root the
//! linker's gc-sections keeps, so `-rdynamic` kept every library's public
//! functions and the data they reach: 6.8 MB of the 17.9 MB stripped
//! release binary on x86_64 (brief §9, and the §15 visibility item).
fn main() {
    println!("cargo:rustc-link-arg-bins=-Wl,--export-dynamic-symbol=fl_dart_*");
}
