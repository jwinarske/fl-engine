//! `DynamicLibrary.process()` resolves symbols with `dlsym` against the
//! running executable, which only sees the executable's own symbols if they
//! are in its dynamic symbol table. Executables do not export by default;
//! `-rdynamic` asks the linker to (brief §9, and the §15 visibility item).
fn main() {
    println!("cargo:rustc-link-arg-bins=-rdynamic");
}
