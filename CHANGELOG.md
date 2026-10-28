# Changelog

Not published, no versions yet.

## Unreleased

### Milestone 2: dart:ui (in progress)

* `fl-dart-ffi`, `fl-ui`, `fl-shell`: dart:ui natives FFI-first, peers on native-field wrappers, the embedder's runtime hooks
* `fl-dart run-app`: a Flutter app's libapp.so to its first frame
* `fl-raster` and `run-app --png`: the frame drawn by emblema offscreen. colored_box exact in every pixel
* Paint decoded from dart:ui's ByteData
* `tools/flutter-app/build.sh`: libapp.so from the pinned Flutter release, no desktop shell
* Flutter app lane in CI
* JIT timeline recorder off: isolate cycles no longer grow about 130 bytes each
* The drawing set: Canvas, Path, clip and opacity layers; 79 natives in all. `shapes` matches the stock engine's golden within its recorded tolerance
* `fl-dart run-app --ops`: a frame's layers and draws
* Every dart:ui native called by an app, checked (`run-app --natives`, `apps/natives`); `Path::contains`; peer-borrow compile-fail case
* M2 apps on the JIT VM from kernel (`build.sh ... kernel`), frames identical to AOT; under ASan and TSan, clean
* `fl-dart app-cycle` and `run-app --hz`: M2 apps in the soak lane, 1,000 lifecycles and 10,000 ticker frames flat; three under valgrind memcheck, clean
* `material`: MaterialApp to its golden; `flutter/restoration` answered asynchronously; `SceneBuilder::addRetained`. 83 natives
* The frame loop: `ScheduleFrame`, a 60 Hz vsync clock, `_beginFrame`/microtasks/`_drawFrame`; a raster thread owning the device. `ticker` and `frame_order` apps; `run-app --frames N`

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
* ASan and TSan lanes: sanitizer VM builds (`jit-asan`, `jit-tsan`), Rust built with `-Zsanitizer`
* No-C++-toolchain lane (exit criterion 8)
* Board kit for Pi 5 and VisionFive 2: `tools/board-kit/`, PASS/FAIL per exit criterion
* Finalizable peer test
* Exit report: docs/m1-exit.md

### Fixed

* AOT runtime needs `--precompilation`, otherwise the first native call crashes
* `Dart_SetVMFlags` must be called once, even with no flags, before `Dart_Initialize`
* Snapshot open closed its own library on return, VM read zeros
* VM toolchain uses libc++ ABI v2 (`std::__2`); archive now carries its own libc++
* dart:io network profiling disabled, needs the VM service which is not in milestone 1
* `fl-dart run-aot hello.aot` failed for a bare file name (dlopen searched the loader path)
* L2 tests compiling the same program raced on one output file
* `main(List<String> args)` run with no arguments got null and threw; now an empty list
* Archives no longer carry the unused ELF loader
* Run loop test assumed an order between two due timers that Dart does not promise

### Not yet

* Real-board runs (Raspberry Pi 5, VisionFive 2)
* dart:ui and rendering (milestone 2)
