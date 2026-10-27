# M1 status

Decisions and findings since the brief (`docs/m1-fl-dart-vm.md`). Newer than
the brief where they disagree.

## Pin

| | |
|---|---|
| Flutter | `stable`, engine.version `cf636b1dfb818dd6a3ff16e390e93b1a036969c8` (2026-10-27) |
| Dart | **3.13.7**, `dart_revision` `416e33b0ffaef062a60ec966bb8c82ed2b83f128` |
| Kernel format | 138; a dill's SDK hash is the revision's first ten characters (`416e33b0ff`) |
| AOT snapshot hash | `0451907c2eaa8467e848c0067bfe8ed4` |
| Rust | 1.94.1 (emblema's pin, Yocto wrynose); `rust-version` 1.85 |

The pin is recorded in four places that a bump changes together:
`fl-dart-sys/build.rs`, `fl-dart-sys/src/lib.rs`, `xtask/src/main.rs` and
`third_party/dart-prebuilt/<version>/VERSION`. The build script refuses an
artifact directory whose `VERSION` disagrees.

## Corrections to the brief

**The AOT snapshot is two symbols, not four.** Dart 3.13's snapshot format
merged the VM and isolate snapshots: an ELF from `dart compile aot-snapshot`
exports `_kDartSnapshotData`, `_kDartSnapshotText` and `_kDartSnapshotBuildId`,
and `Dart_CreateIsolateGroup` takes `(snapshot_data, snapshot_text)`.
`Dart_InitializeParams` no longer has `vm_snapshot_data`/`_instructions` at
all, so `Vm::init` needs nothing from the program — the brief's §7 "four
snapshot symbols" and the dlopen description in §8 are superseded.

**The snapshot header also carries a feature string** right after the hash:
`product no-asan no-msan no-tsan ... no-compressed-pointers` for the stock SDK's output. The VM
rejects a snapshot whose features differ from its own build. The AOT artifact
must therefore be built in **product** mode to run snapshots from the stock
`dart compile aot-snapshot`; a debug or sanitizer build of the runtime needs
snapshots from a `gen_snapshot` of the same configuration. This bears on the
brief's "debug build of the artifact set" mitigation (§14) and on the ASan
lane (§11): those lanes need their own gen_snapshot.

**Bindings are generated ahead of time, not in the build script.** §4.1 had
the build script run bindgen; bindgen needs libclang, which contradicts exit
criterion 8 (builds with no C++ toolchain). `cargo xtask bindgen` regenerates
them from the checked-in headers; C enums are bound as integer newtypes, since
the VM may return values a Rust enum does not declare.

**`Isolate` carries the `Vm`'s lifetime** (`Isolate<'vm>`), so a `Vm` cannot be
dropped — and `Dart_Cleanup` cannot run — under a live isolate. A trybuild case
pins it. **`Isolate::enter` returns `Result`**: the types keep one isolate on one
thread, but cannot see that the thread is already inside a *different*
isolate, which `Dart_EnterIsolate` forbids; that is checked at run time.

**The repository** is `jwinarske/fl-engine`, which you create; this session's integration cannot create repositories.

**`isolate_from_kernel` copies the dill.** The VM reads kernel lazily for the
group's whole life, so the bytes are owned by the group's data and released in
the VM's group-cleanup callback; a borrowed `&[u8]` could not promise that.

## Bring-up list (§15)

Resolved from source at the pin; citations in `docs/m1-s15-findings.md`.
"Verify at first run" means the answer comes from code paths `dartvm` and
Flutter use, not yet from this embedder running.

| Item | Finding |
|---|---|
| GN targets | `//runtime:libdart{suffix}` is the one static library; `vm:libdart_vm`, `vm:libdart_lib`, `vm:libdart_compiler`, `platform:libdart_platform` and double-conversion are source sets inside it. Suffixes (`runtime/configs.gni`): **JIT = `libdart_jit.a`, AOT = `libdart_aotruntime_product.a`**. Plus `icuuc` (mandatory: regexp), `dl`, `pthread`. Nothing from `runtime/bin` is needed unless dart:io is (see Timers). GN args: exactly one of `is_debug`/`is_release`/`is_product`; JIT vs AOT is the target suffix, not `dart_runtime_mode`. |
| Kernel consumable? | **Yes.** `dart compile kernel` links the platform by default; `Dart_CreateIsolateGroupFromKernel(app.dill)` then `Dart_LoadScriptFromKernel(app.dill)` is `dartvm`'s own fallback path, and is what `isolate_from_kernel` does. Verify at first run. |
| Snapshot blobs | `Dart_Initialize` takes none. JIT from kernel needs none. A core snapshot (`gen_snapshot --snapshot_kind=core`) is an optional startup optimization, declined for M1. |
| `print()` | Implemented as `dartutils.cc` does it, via `dart:_builtin`'s `_getPrintClosure` and a Rust `Builtin_PrintString`. Verify at first run that `dart:_builtin` is present in AOT snapshots; the fallback is an entry-point tear-off from the program's own root library. Microtask hooks (`_setScheduleImmediateClosure`, `_setupHooks`) are installed too. `entropy_source` is provided (without it `Random.secure()` throws). Null file callbacks are safe. |
| Symbol visibility | `DynamicLibrary.process()` is `dlsym(RTLD_DEFAULT)` on every target. `-rdynamic` plus a `#[used]` table keeping the exports. Verify on all three targets. |
| Isolate flags | `Dart_IsolateFlagsInitialize` defaults are right. `null_safety` is forced on and ignored; AOT snapshots override group flags. Flutter sets only `snapshot_is_dontneed_safe`, an mmap optimization. |
| AOT cross-targeting | `dart compile aot-snapshot --target-os linux --target-arch arm64|riscv64` works and downloads the cross `gen_snapshot` from dart-archive (tested in this container). Output is an `ELF DYN` with no `DT_NEEDED`; glibc `dlopen` loads it. Its features string is `product ... no-compressed-pointers`. |

### Timers: dart:io is linked (decided 2026-10-27)

`Timer` posts its wakeup request through `VMLibraryHooks.eventHandlerSendData`,
which only dart:io's `_setupHooks` assigns. Decision: **link dart:io's
embedder support**, as `dartvm` and Flutter both do. dart:io's API
(`runtime/include/bin/dart_io_api.h`) is C++, so the pipeline compiles a
three-function C shim, `tools/dart-prebuilt/fl_dart/fl_dart_io.{h,cc}`, into
the artifact; `fl-dart-sys` binds it by hand. `Vm::init` calls
`fl_dart_io_bootstrap` before `Dart_Initialize` (starting the event handler
thread), `Drop` calls `fl_dart_io_cleanup` after `Dart_Cleanup`, and each new
isolate runs `fl_dart_io_setup_isolate` after the print and microtask hooks.
`print` stays Rust-owned: dart:io registers its resolver on `dart:io` only.

Cost: zlib and BoringSSL enter the artifact. `exit()` is left enabled, as in
Flutter.

### Candidate milestone: dart:io in Rust (recorded 2026-10-27, not scheduled)

Raised and declined for M1. The case for it: linking dart:io made the "one
C++ component" line into "one component and its two dependencies" (zlib,
BoringSSL). A Rust dart:io would shrink the C++ surface to libdart proper,
replace BoringSSL with rustls, and make the event handler ours -- all of
which help the Yocto story.

What it is: not an API reimplementation but a port of an internal ABI.
dart:io's Dart half (`sdk/lib/_internal/vm/bin/*_patch.dart`) is compiled
into the platform dill and every AOT snapshot, and binds to natives by name:
185 entries in `runtime/bin/io_natives.cc`, backed by ~25k lines of Linux C++
(event handler, process, file, socket, TLS filter, directory watch, stdio).
A port implements those natives with the exact names, argument conventions
and error behaviour that Dart side expects, since it is not ours to change.
`socket_patch.dart` and `secure_socket_patch.dart` are the tightly coupled
ones (epoll event-mask encoding, BoringSSL filter buffers).

Preconditions, in order:
1. M8 done, so there is a framework to break: Flutter's `Platform`, `File`,
   `HttpClient`, `Process` use all run on dart:io.
2. A conformance suite in the lanes before the first native is ported:
   `tests/standalone/io` in the SDK (~1,000 tests) run against the linked
   C++ as baseline, then against the port. Parity is measured, not asserted.
3. Native by native behind a feature, with the C++ archive still linkable
   until the suite is green on all three targets.

The cheap partial (a Rust event handler only, no `runtime/bin` at all) was
considered and rejected: it gets timers without BoringSSL, but leaves dart:io
present in the snapshot and throwing at run time, which M8 cannot accept.

### M2 finding (§13)

`@Native` resolution tries a `Dart_SetFfiNativeResolver` on the declaring
library first, then process `dlsym`. **M2 can be FFI-first**, and the resolver
variant needs no symbol export at all. Tonic's native-resolver machinery need
not be built.

## Artifact pipeline (decided: CI, with googlesource)

`.github/workflows/dart-prebuilt.yml` runs `tools/dart-prebuilt/build.sh` once
per (target, mode): x86_64 jit, and x86_64, aarch64 and riscv64 aot. Each job:
depot_tools, `gclient sync --no-history` at the pinned revision (DEPS brings
the toolchain and the multi-arch Debian sysroot), then the SDK's own
`tools/build.py`. JIT builds in `release` mode and AOT in `product` mode; the
stock SDK's snapshots declare `product`, and the VM refuses a snapshot whose
features differ from its own.

The script grafts `fl_dart/BUILD.gn` into the checkout. It defines one
`complete_static_lib` per mode (`libfl_dart_jit.a`, `libfl_dart_aot.a`), each
holding the VM, dart:io's embedder support, ICU, zlib, BoringSSL and the shim.
`link.txt` is therefore one archive plus `stdc++`, `dl`, `pthread`, `m`, and
link order never comes up. The script refuses to run unless the checkout's
HEAD and its recomputed snapshot hash both match `VERSION`.

Tarballs are deterministic, with mtime 0 and sorted entries. A
`workflow_dispatch` with `publish: true` attaches them and a `SHA256SUMS` to
release `dart-prebuilt-<version>`. Checking that `SHA256SUMS` into
`third_party/dart-prebuilt/<version>/` is what makes `cargo xtask
fetch-prebuilt` work. Until then, fetch-prebuilt refuses to download anything
it cannot verify.

Not yet run: the first CI run is the first time `fl_dart/BUILD.gn` meets gn.
Expect fixes there, most likely config names or `complete_static_lib` pulling
in something unwanted.

## What runs today

Without a VM (`FL_DART_SDK_OUT=none`): clippy on the whole workspace in both
modes, the header/port unit tests, and the seven compile-fail cases. With
artifacts: `cargo test -p fl-dart-cli` (needs `FL_DART_SDK_BIN`) runs the L2
suite: hello world, run-loop draining, exception with stack trace, both FFI
crossings, version mismatch.
