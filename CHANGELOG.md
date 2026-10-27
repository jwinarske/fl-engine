# Changelog

Not published, no versions yet.

## Unreleased

### Milestone 1: Dart VM hosted from Rust

* AOT (`fl-dart run-aot`) and JIT (`fl-dart run`) runtimes
* Dart 3.13.7, matching Flutter stable's engine pin
* libapp.so from the Flutter tooling, loaded with dlopen as the Flutter engine does
* First engine native: dart:ui print via `@Native` `DartRuntimeHooks::Logger_PrintString` and `Dart_SetFfiNativeResolver`
* Isolate thread affinity, enter/exit pairing and handle lifetime enforced by types, 7 compile-fail cases
* FFI: Dart to Rust via `DynamicLibrary.process()`, Rust to Dart via `NativePort` from any thread
* Typed errors: `UnhandledException` with Dart stack trace, `SnapshotVersionMismatch` naming both versions
* Prebuilt VM archives for x86_64 (jit, aot), aarch64 and riscv64 (aot), built in CI, checksum-verified on fetch
* dart:io linked through a C shim, needed for Timer
* Bindings generated ahead of time (`cargo xtask bindgen`), no libclang at build time
* aarch64 and riscv64 lanes under qemu: L2 and unit tests in CI
* Leak and soak lanes: 1,000 isolate cycles, 100 VM lifecycles, valgrind memcheck (AOT)
* Racing posts to one `NativePort` from 8 threads, in L2 and soak

### Fixed

* AOT runtime needs `--precompilation`, otherwise the first native call crashes
* `Dart_SetVMFlags` must be called once, even with no flags, before `Dart_Initialize`
* Snapshot open closed its own library on return, VM read zeros
* VM toolchain uses libc++ ABI v2 (`std::__2`); archive now carries its own libc++
* dart:io network profiling disabled, needs the VM service which is not in milestone 1
* `fl-dart run-aot hello.aot` failed for a bare file name (dlopen searched the loader path)
* L2 tests compiling the same program raced on one output file
* `main(List<String> args)` run with no arguments got null and threw; now an empty list

### Not yet

* Real-board runs (Raspberry Pi 5, VisionFive 2)
* ASan, and TSan on `NativePort::post` (need a sanitizer VM build)
* dart:ui and rendering (milestone 2)
