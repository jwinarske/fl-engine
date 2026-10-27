# Changelog

Nothing has been released yet. The workspace is `publish = false` and has no
version on a registry to anchor entries to, so everything so far sits under
**Unreleased**. A version heading starts when there is something to tag. The
commit history has the detail; each commit says why a change is shaped the way
it is, not just what moved.

## Unreleased

### Milestone 1: the Dart VM, hosted from Rust

**The VM runs Dart programs from Rust, in both runtimes.** `fl-dart run-aot`
runs a precompiled snapshot, and `fl-dart run` runs a kernel dill on a JIT
build. Either way the VM starts, runs `main()`, drains the message loop
(microtasks, timers, futures) and shuts down. Pinned to Dart 3.13.7, the
revision Flutter stable's engine pins.

**A `libapp.so` from the Flutter tooling runs.** It is loaded with `dlopen`,
the way the Flutter engine loads it. Its `print` reaches Rust through the
`dart:ui` `@Native` symbol `DartRuntimeHooks::Logger_PrintString`, resolved by
`Dart_SetFfiNativeResolver`. That is the first engine native, and it is done
the FFI-first way the next milestone's `dart:ui` bindings will use. No other
`dart:ui` native exists yet.

**Isolate rules are types.** `Isolate` is neither `Send` nor `Sync`. Entering
it returns a scope that exits on drop. Handles borrow their API scope, and
isolates borrow the `Vm`. Seven compile-fail cases pin these rules.

**FFI in both directions.** Dart calls exported Rust functions through
`DynamicLibrary.process()`. `NativePort` is the one `Send + Sync` type: it posts
messages into Dart from any thread.

**Errors are typed.** An uncaught exception arrives as
`DartError::UnhandledException` carrying the Dart stack trace. A snapshot built
by another SDK is refused before it runs, as `SnapshotVersionMismatch` naming
both versions. Kernel and AOT headers are checked up front for that reason.

**The VM is a prebuilt archive, and cargo never builds C++.** The VM is built
from `dart-lang/sdk` at the pin by a CI pipeline for x86_64 (JIT and AOT),
aarch64 and riscv64 (AOT). It ships as one self-contained archive per target
and mode: the VM, `dart:io`'s embedder support, ICU, zlib, BoringSSL and the
toolchain's static libc++. `cargo xtask fetch-prebuilt` installs one only if
it matches the checked-in `SHA256SUMS`.

**`dart:io` is linked.** `Timer` needs its event handler, as in the standalone
VM and in Flutter. A three-function C shim, compiled by the pipeline, exposes
its C++ embedding API to Rust.

**Bindings are generated ahead of time.** `cargo xtask bindgen` regenerates
them from the headers checked in at the pin, so building needs no libclang. C
enums are bound as integer newtypes, because the VM can return values a Rust
enum would not declare.

### Fixed during bring-up

These were found by running the VM rather than by reading about it:

- The precompiled runtime must be started with `--precompilation`, as
  `dartaotruntime` and the Flutter engine both start it. Without the flag, AOT
  code runs under JIT assumptions and crashes at its first native call.
- `Dart_Initialize` refuses to run until `Dart_SetVMFlags` has been called once,
  even with no flags.
- Opening a snapshot built a placeholder whose `Drop` closed the library on the
  way out, so the VM read zeros.
- The VM toolchain links libc++ with ABI v2 (`std::__2`), which no system C++
  runtime provides. The archive now carries its own libc++.
- `dart:io`'s network profiling registers a VM-service extension, which fails
  without the service. It is turned off; the service is not part of milestone 1.

### Not yet

- Running on aarch64 and riscv64. The archives exist, but the qemu and
  real-board runs (Raspberry Pi 5, VisionFive 2) are still to do.
- The leak and soak lanes. `fl-dart cycle` is the harness's entry point.
- `dart:ui`, and any rendering. That is the next milestone.
