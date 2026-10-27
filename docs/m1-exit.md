# Milestone 1 exit report

State of the §12 exit criteria and the §15 list of the brief (docs/m1-fl-dart-vm.md), with the evidence for each. Evidence is a CI lane, a test, or a recorded finding. Pin: Dart 3.13.7, `416e33b0`.

Open: real-board runs (criterion 2). Everything else is closed.

## §12 exit criteria

| # | Criterion | State | Evidence |
|---|---|---|---|
| 1 | `run hello.dill`, x86_64 jit | done | ci.yml `L2 x86_64 jit`: `hello_world_reaches_the_rust_sink_and_exits_zero` |
| 2 | `run-aot hello.aot`, x86_64, Pi 5, VF2 | x86_64 done, boards open | ci.yml `L2 x86_64 aot`. aarch64, riscv64 under qemu: cross.yml. Boards: board kit, see below |
| 3 | FFI both directions, port from a non-isolate thread | done | `ffi_crossings_both_directions`, `racing_posts_all_arrive_in_order`. x86_64, and aarch64, riscv64 under qemu |
| 4 | Throwing `main`: `UnhandledException`, stack trace, nonzero, no UB | done | `unhandled_exception_carries_the_dart_stack_trace` (exit 255). Clean under valgrind, ASan, TSan |
| 5 | Mismatched SDK: `SnapshotVersionMismatch`, both versions | done | `snapshot_from_another_sdk_is_named_as_a_version_mismatch` (exit 254), fl-dart-vm `snapshot::tests` |
| 6 | Leak and soak lanes, full-process init/cleanup | done | soak.yml: 1,000 isolate cycles x 4 programs, 100 processes, valgrind (aot). sanitize.yml: ASan, TSan |
| 7 | trybuild pins all three guarantees | done | `crates/fl-dart-vm/tests/compile_fail`, 7 cases, see below |
| 8 | Pipeline reproducible from the pin; no C++ toolchain to build | done | dart-prebuilt.yml runs 9 and 11: five unchanged archives, identical sums. ci.yml `no C++ toolchain` |
| 9 | §15 list empty | done | below |

Criterion 7, by guarantee:

* Isolate thread affinity: `isolate_crosses_threads`, `isolate_scope_crosses_threads`, `persistent_handle_crosses_threads`
* Enter/exit pairing: `scope_outlives_isolate`, `isolate_outlives_vm`
* Handle-scope lifetime: `handle_escapes_scope`, `handle_stored_outside_scope`

Criterion 8, no C++ toolchain: bare `ubuntu:24.04`, gcc as rustc's linker driver only, `g++`/`c++`/`clang++`/`clang` absent. Fetches the aot archive, L2 9/9, `fl-dart` needs only libm, libgcc_s, libc and the loader (`ci/no-cxx.sh`).

## Board run (criterion 2)

    tar xJf fl-dart-board-kit-<target>.tar.xz && cd fl-dart-board-kit-<target>
    ./run.sh | tee report.txt

* Kit: cross.yml artifact `fl-dart-board-kit-<target>`, 90 days, or `tools/board-kit/build.sh`
* Checks criteria 2-6 on the board, one PASS/FAIL line each
* Tested as packaged under qemu, both targets, `dash`: 9 passed
* Done when both boards print 0 failed. Record the two reports here

## §15 bring-up list

| Item | Answer | Verified by |
|---|---|---|
| GN targets, output paths | `//runtime:libdart_jit`, `libdart_aotruntime_product`, `//runtime/bin:common_embedder_dart_io{,_product}`, ICU, one `complete_static_lib` per mode (`tools/dart-prebuilt/fl_dart/BUILD.gn`), `out/<conf>/obj/fl_dart/libfl_dart_<mode>.a` | dart-prebuilt.yml, every target and mode |
| `dart compile kernel` consumable directly | Yes. `--link-platform` defaults on; `Dart_CreateIsolateGroupFromKernel` takes the dill as is | L2 jit lane |
| JIT VM snapshot blobs | None. Kernel-only JIT; 3.13 has no `vm_snapshot_*` in `Dart_InitializeParams`. AOT is one data/text pair from the program | L2 jit and aot lanes |
| print hook, IO and entropy callbacks | `dart:_builtin` `_getPrintClosure`, or `dart:ui` `@Native` `Logger_PrintString` via `Dart_SetFfiNativeResolver` (libapp.so). IO through the dart:io shim, `tools/dart-prebuilt/fl_dart/` | L2 hello; libapp.so run, m1-status.md |
| Symbol visibility for `DynamicLibrary.process()` | `-rdynamic` (fl-dart-cli build.rs) plus a `#[used]` table so the exports survive gc-sections | `ffi_crossings_both_directions` on x86_64, aarch64, riscv64 (qemu) |
| `IsolateConfig` flags needed | `Dart_IsolateFlagsInitialize` defaults. VM side: `Dart_SetVMFlags` once, even empty; `--precompilation` for AOT | L2 aot lane; m1-status.md |
| AOT cross-targeting | `dart compile aot-snapshot --target-os linux --target-arch arm64\|riscv64` directly; the SDK fetches the cross gen_snapshot. The pipeline does not drive gen_snapshot | cross.yml |

Detail: docs/m1-s15-findings.md, docs/m1-status.md.

## Handed to milestone 2 (§13)

* `HandleScope<'s>`: handles cannot outlive their scope (compile-fail cases above)
* `NativePort`: `Send + Sync`, posts from any thread; 8 racing threads, TSan clean
* Finalizable peers: `HandleScope::new_finalizable(handle, peer, size)`. No `FinalizableHandle<T>` value is exposed: a finalizable handle is unsafe to use once its object may be collected (dart_api.h), so the API keeps it and promises the peer's lifetime instead. Dropped exactly once, by collection or isolate-group shutdown (`tests/finalizer.rs`, also under ASan and TSan)
* The FFI-first question: `DynamicLibrary.process()` resolution is clean on x86_64, and on aarch64 and riscv64 under qemu. The first engine native, dart:ui print, is already FFI-first (`@Native` plus `Dart_SetFfiNativeResolver`). fl-dart-ffi can be FFI-first; a tonic-style resolver is not needed. Board confirmation: kit check 3

## Findings worth keeping

* `--precompilation` is required by the AOT runtime; without it the first native call crashes
* TSan builds use the SDK's own libc++ (`std::_d`), added only to executables by BUILDCONFIG.gn; the archive depends on it explicitly
* qemu-user 8.2 keeps about 39 kB per guest thread it ever ran. RSS under qemu says nothing about leaks; soak RSS limits are asserted natively and on boards only
* glibc's default malloc arenas step RSS at random under many threads; soak runs with `MALLOC_ARENA_MAX=1`
* Two due timers fire in either order; tests must not assume one
