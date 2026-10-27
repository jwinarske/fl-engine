# M1: fl-dart-vm — Embedding the Dart VM in Rust

> Saved from the milestone brief (2026-10-27), condensed from §4 on;
> the section numbering and every decision are unchanged. Decisions taken and
> findings recorded during bring-up live in `docs/m1-status.md`; where the two
> disagree, the status file is newer.

**The first Dart milestone of the Flutter engine port. A Rust binary that hosts the Dart VM, loads a snapshot, runs `main()`, and crosses the FFI boundary in both directions. No rendering — emblema is set aside for this milestone and rejoined at M2.**

This is the highest-risk unbuilt piece of the engine port. The renderer risk is retired (emblema's parity table is the evidence); the platform/display risk is retired (emblema-present, drmkit); what remains unproven is the Dart column, and M1 is where its integration issues — build system, snapshot formats, isolate threading, handle lifetimes — surface before they can tangle with anything else.

One framing correction to the original engine-port plan, applied throughout: **tonic is not ported, it is replaced.** M1 builds the VM-hosting layer (`fl-dart-sys`, `fl-dart-vm`); the tonic-equivalent binding layer (`fl-dart-ffi`) is M2's enabling tech and only previewed here (§13).

---

## 1. What M1 Is and Is Not

**Is:** the Dart VM linked into a Rust binary; VM init and clean shutdown; isolate creation from a kernel snapshot (dev) and an AOT snapshot (target); `main()` invoked; the message loop drained; Dart→Rust and Rust→Dart crossings each demonstrated once; errors surfaced as typed Rust errors with Dart stack traces; all of it leak-clean and enforced-at-compile-time thread-correct.

**Is not:** dart:ui (M2), any rendering, multi-isolate beyond what shutdown testing needs, hot reload, the VM service / DDS / observatory (deliberately disabled — debugging arrives at M9 of the engine plan), dart:io beyond whatever the platform snapshot carries, and any plugin story.

The deliverable is a CLI:

```
fl-dart run hello.dill         # JIT, dev hosts only (feature "jit")
fl-dart run-aot hello.aot      # precompiled runtime, all targets
```

plus the two library crates it is a thin shell over.

## 2. The Embedding API, Precisely

Three headers in `dart-lang/sdk` matter, and they are not interchangeable:

- **`runtime/include/dart_api.h`** — the full embedding API: `Dart_Initialize`, `Dart_CreateIsolateGroup*`, enter/exit, scopes, handles, `Dart_Invoke`, `Dart_RunLoop`, error inspection. This is what a *host* links. It requires linking the VM libraries themselves; there is no stable-ABI shared library that exports it.
- **`runtime/include/dart_api_dl.h`** — the *dynamic linking* subset: a function-pointer table initialized with `Dart_InitializeApiDL`. It exists for FFI libraries loaded *into* someone else's VM. It cannot initialize or host a VM. fl-dart-vm does not use it.
- **`runtime/include/dart_native_api.h`** — native ports and `Dart_PostCObject`, the any-thread Rust→Dart message path. Linked by the host alongside dart_api.h.

So M1 links static VM libraries and binds `dart_api.h` + `dart_native_api.h`. `dart_tools_api.h` is bound but behind a feature and unused in M1.

## 3. Runtime Mode Decision: AOT-Primary

**AOT is the primary mode.** Every emblema board target (Pi 5, VisionFive 2, SA8155P-class, RK3588 when it lands) runs the precompiled runtime.

**JIT survives as a dev-host feature only**, behind `--features jit`, x86_64 Linux hosts, for iteration speed. No hot reload (service isolate off), no promise of existing on any board.

Consequence for the sys crate: two link configurations, `libdart_jit`-flavored and `libdart_precompiled_runtime`-flavored, mutually exclusive per build, selected by feature. The safe API above them is identical except for which snapshot loaders exist.

## 4. libdart Sourcing and Build Strategy

The hard constraint: **the Dart SDK publishes no embeddable libdart artifact.** The VM is built from `dart-lang/sdk` source with depot_tools + GN + ninja. The engine workspace carries exactly one C++ runtime component (the VM). The discipline is keeping the C++ build *out of cargo*.

### 4.1 The contract: cargo never builds C++

`fl-dart-sys`'s build script does not run GN, does not fetch sources, does not require a C++ toolchain. It consumes a **prebuilt artifact directory** named by `FL_DART_SDK_OUT` (or a checked-in default under `third_party/dart-prebuilt/<version>/<target>/`):

```
<artifact>/
  include/dart_api.h, dart_native_api.h, dart_tools_api.h, dart_version.h
  lib/<target-triple>/
    jit/        libdart_jit*.a + support libs        (feature "jit")
    aot/        libdart_precompiled_runtime*.a + support libs
  snapshots/
    vm_platform_strong.dill                           (JIT compile input)
    [vm snapshot blobs as the JIT config requires — see §15]
  bin/host/
    gen_snapshot                                      (host-runs, targets cross)
  VERSION                                             (Dart version + git hash)
```

The build script's whole job: locate, verify VERSION against the pinned expectation, emit `cargo:rustc-link-search`/`link-lib` lines (including the C++ runtime — `stdc++` or `c++` per target), and run bindgen over the headers. A missing or mismatched artifact is a build error that names the fetch command.

### 4.2 Producing the artifacts

A separate pipeline (CI-driven, not cargo-driven): `gclient` checkout of `dart-lang/sdk` at the pinned revision; GN+ninja per target; tarball per (version, target-triple, mode), checksummed, uploaded to GitHub Releases; `fl-dart x fetch-prebuilt` (xtask) downloads and unpacks.

Targets at M1: `x86_64-unknown-linux-gnu` (jit+aot), `aarch64-unknown-linux-gnu` (aot), `riscv64gc-unknown-linux-gnu` (aot).

### 4.3 Version pinning and the Yocto alignment

Pin the Dart revision to **what the tracked Flutter release's engine DEPS pins**, not to a standalone Dart stable. Yocto: meta-flutter already builds the engine (VM included) per release branch — either ship fl-dart prebuilt artifacts through a recipe of their own, or extract the runtime libs from the engine build. Decide when the first board recipe is written.

## 5. Crate Structure

```
crates/
  fl-dart-sys     bindings over dart_api.h + dart_native_api.h (+ tools, feature-gated);
                  links prebuilt libdart; features: jit, aot (one required)
  fl-dart-vm      the safe layer: Vm, Isolate, IsolateScope, HandleScope,
                  typed errors, snapshot loaders, message pump, native ports
  fl-dart-cli     `fl-dart run` / `run-aot` / `x fetch-prebuilt`; leak/stress harness entry points
```

## 6. The Safe Wrapper Surface

Design rule: **encode in types what C++ embedders hold by convention** — isolate thread affinity, handle/scope lifetime, enter/exit pairing each become a compile error.

- `Vm` — process-wide; `Dart_Initialize` once per process; a second `Vm` is `Err`. Drop = `Dart_Cleanup`.
- `Isolate` — `!Send + !Sync`. `enter(&mut self) -> IsolateScope<'_>` (guard `!Send`, exit on drop). `shutdown(self)`.
- `IsolateScope::with(|&mut HandleScope<'_>| ..)` — `Dart_EnterScope/ExitScope` RAII; handles borrow the frame.
- `HandleScope`: `invoke_main`, `run_loop`, `new_persistent`, conversions as tests need.
- `DartError { Api, UnhandledException{message,stacktrace}, Compilation, SnapshotVersionMismatch{expected,found}, Fatal }`.
- `PersistentHandle` deleted explicitly (no Drop); `FinalizableHandle<T>` proved only to fire in M1.
- `NativePort` — `Send + Sync`, `post(CObject) -> Result<(), PortClosed>` via `Dart_PostCObject`; proved under TSan.
- Panic policy: every `extern "C"` callback wraps its body in `catch_unwind`.

## 7. Snapshot Formats and Version Locking

- **Kernel (`.dill`)** — JIT input from `dart compile kernel`.
- **AOT ELF** — from `gen_snapshot`; loaded dlopen-first via the four `_kDart*Snapshot*` symbols; ELF loader as fallback.
- **AppJIT** — declined. **Core/VM snapshot blobs** — opaque bytes beside the libs.

Version locking is absolute: CLI checks `Dart_VersionString()` against artifact VERSION; mismatched dill/aot surfaces as `DartError::SnapshotVersionMismatch` naming both strings.

## 8. Hello World, Both Modes

```
dart compile kernel hello.dart -o hello.dill && fl-dart run hello.dill
dart compile aot-snapshot hello.dart -o hello.aot && fl-dart run-aot hello.aot
```

Boot sequence: Vm::init → load AOT → isolate_from_aot → enter → with scope: invoke_main → run_loop → exit → shutdown → Vm drop. Exit criterion: *Hello World reaches a Rust-owned sink*.

## 9. FFI Proof Points

- **Dart→Rust:** `DynamicLibrary.process()` + `#[no_mangle] pub extern "C"`; scalars in/out; one `Pointer<Utf8>`.
- **Rust→Dart:** Dart `ReceivePort`, native port id passed out via the first crossing; Rust thread posts via `NativePort::post`; Dart completes and exits.

## 10. Reference Embedders to Read First

`runtime/bin/main.cc` + `dartaotruntime`; `flutter_tester` and engine `DartIsolate`/`RuntimeController`; `runtime/bin/elf_loader.*`; engine DEPS.

## 11. Test Plan

- **L0 unit:** config, artifact/VERSION validation, error mapping, CObject conversions.
- **Compile-fail (trybuild):** `Isolate` crossing threads; `Handle` escaping `HandleScope`; `IsolateScope` outliving `Isolate`.
- **L2 integration:** hello both modes; exception→`DartError` with stacktrace; version-mismatch; both FFI crossings; run_loop drains timers/microtasks.
- **L5 leak/soak:** 1,000 isolate cycles, 100 full Vm process runs under ASan + valgrind, flat RSS/fd; TSan lane on `NativePort::post`.
- **Cross lanes:** qemu aarch64 + riscv64; AOT hello on real Pi 5 and VisionFive 2.
- **CI shape:** artifacts fetched by checksum; merge-blocking x86_64 (jit+aot) + both qemu lanes; zero-retry; skip census — an absent artifact must fail loudly.

## 12. Exit Criteria

1. `fl-dart run hello.dill` prints Hello World, exits 0 on x86_64 (jit).
2. `fl-dart run-aot hello.aot` same on x86_64, aarch64 (Pi 5), riscv64 (VF2) — real boards.
3. Dart→Rust FFI round-trips scalars and a string; Rust→Dart native-port message completes a Dart future, posted from a non-isolate thread.
4. Throwing `main` → `DartError::UnhandledException` with Dart stack trace; nonzero exit, no UB.
5. Mismatched-SDK dill/aot → `SnapshotVersionMismatch` naming both versions.
6. Leak and soak lanes green, incl. full-process init/cleanup cycles.
7. trybuild suite pins all three lifetime/affinity guarantees.
8. Artifact pipeline produces all three targets reproducibly from the pin; `fl-dart-sys` builds with no C++ toolchain present.
9. §15 list empty — every item resolved and recorded.

## 13. What M1 Hands M2

`HandleScope` lifetime discipline, `FinalizableHandle<T>`, `NativePort`. Open question M1 informs: if `DynamicLibrary.process()` resolution is clean on all three targets, fl-dart-ffi can be FFI-first (`#[dart_native]` → exported symbols + Dart `@Native`), and tonic's resolver pattern may never be built. Record the finding.

## 14. Risks

GN build maintenance cost (CI pipeline, deliberate pin PRs); riscv64 least-trafficked port (exit criterion, qemu lane, upstream filing); C++ stdlib linkage portability (one platform-conditional in fl-dart-sys); service-off debugging blindness (reference embedders, debug artifact build); half-remembered API details (§15 + §12.9).

## 15. Bring-Up Verification List

- Exact GN targets/output paths for JIT and precompiled-runtime static libs.
- Whether `dart compile kernel` output is directly consumable by `Dart_CreateIsolateGroupFromKernel` or needs platform-dill linking.
- Which VM snapshot blobs `Dart_Initialize` requires in JIT config, and where the build emits them.
- The print/stdout hook for `print()` in a bare embedder; what `Dart_Initialize` IO/entropy callbacks must provide.
- Symbol visibility flags for `DynamicLibrary.process()` resolution of Rust exports on all three targets.
- `IsolateConfig` flags actually required at the pinned version.
- Whether `dart compile aot-snapshot` cross-targets directly or the pipeline drives `gen_snapshot`.
