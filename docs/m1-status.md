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
| Symbol visibility | `DynamicLibrary.process()` is `dlsym(RTLD_DEFAULT)` on every target. `-rdynamic` plus a `#[used]` table keeping the exports. Verified: FFI test passes on x86_64 native, aarch64 and riscv64 under qemu. Boards still to confirm. |
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
and error behavior that Dart side expects, since it is not ours to change.
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

### Pipeline run 1 (2026-10-27): all four green; archives linked libc++ ABI v2

The first run built all four artifacts without a `BUILD.gn` fix (8-13 min
each). The x86_64 archives contain what they should (`Dart_Initialize`,
`fl_dart_io_*`, ICU `u_strlen_77`, BoringSSL `SSL_new`). Linking `fl-dart`
against them failed on undefined `std::__2::*` symbols: the SDK's toolchain
is Fuchsia clang, which builds and links libc++ with its **unstable ABI v2**
(`std::__2`), statically, into every SDK binary. A distro libc++ exports
`std::__1` and can never satisfy it; libstdc++ never could. So the C++
runtime is not a choice the consumer gets to make, and the brief's "one
platform-conditional" (§14, `stdc++` vs `c++`) does not exist here: build.sh
now merges the toolchain's `libc++.a` and `libc++abi.a` into the archive,
the consumer links no C++ runtime at all, and a board needs none installed
-- the same shape as `dartaotruntime`. Unwinding is the consumer's
`libgcc_s`; the toolchain's `libunwind.a` is left out so there is one
unwinder in the process.

Also found: a second `publish` run for the same pin must replace the
release's assets, since the tag exists; the job now does so. The checked-in
`SHA256SUMS` is what pins consumers, so a replaced asset is inert until that
file changes under review.

### Pipeline run 3 (2026-10-27): the merge's first run failed on all four, blind

The first run of the libc++ merge failed every job at the Build step, after
the full SDK build. The logs were unreachable from the session doing the
work: the Actions log endpoint redirects to a blob host its proxy refuses,
and the failure annotation says only "exit code 1". Two consequences, both
now in the tree:

- `die()` also emits a `::error::` annotation on Actions, because annotations
  are served by the API that *is* reachable. A pipeline whose failures cannot
  be read from where the work happens fails twice.
- The likely cause was found by reading rather than by logs — twice, by two
  sessions working the same failure concurrently, whose fixes met in a rebase.
  One asked the toolchain itself (`clang --print-file-name=libc++.a`), which
  is the locate that survives a package relayout and is what landed as the
  primary; the other contributed what the ask still missed: the riscv64
  toolchain triple drops the `gc` ISA suffix Rust's keeps, so clang must be
  asked with the triple it knows, and `libc++abi.a` is optional, since some
  packages merge it into `libc++.a`. A die now names the candidates it saw.

### The workspace's own lanes (`.github/workflows/ci.yml`, 2026-10-27)

Two jobs. `no-vm` is the documented lane set, verbatim: fmt, clippy in
both modes, the fl-dart-vm unit and compile-fail suites, docs under
`-D warnings`. `l2` fetches a published artifact by checksum, downloads the
pinned stock SDK from dart-archive (hosted runners reach it; the session's
proxy does not), asserts it is 3.13.7, and runs `cargo test -p fl-dart-cli`
in each mode. The L2 lane stays red until a green `publish` run's
`SHA256SUMS` is checked in — which is the gate working as designed, not a
lane to silence.

Found by the doc lane on its first local run: the generated bindings carry
upstream's doc comments, whose `[identifier]` emphasis and `List<dynamic>`
generics rustdoc reads as broken links and unclosed HTML under
`-D warnings`. The allows live in `fl-dart-sys/src/lib.rs` beside the other
bindgen allowances, not in the generated files, so a regenerate keeps them.

**The repository is private, which decides the fetch shape.** Three fetch
failures in a row were one fact wearing different masks: the browser download
URL serves a private release to nobody -- not anonymously, and not to an
Actions token, both 404. `fetch-prebuilt` now goes through the API asset
endpoint (octet-stream, 302 to a signed blob URL), requires a token by name
when it is absent, and carries it in a curl config file rather than argv so a
failed command echoed into an annotation cannot leak it. The asset id comes
from a scan of the release JSON that a unit test pins against the payload
shape GitHub serves. Found by ci's annotated failures; the alias commit, the
no-token attempt and this are its first three catches.

Still not in the lanes, deliberately sequenced after a green L2: the qemu
aarch64/riscv64 lanes, and the ASan/leak lane, which waits on a
`gen_snapshot` of its own configuration (the feature-string finding above).

## What runs today

Without a VM (`FL_DART_SDK_OUT=none`): clippy on the whole workspace in both
modes, the header/port unit tests, and the seven compile-fail cases. With
artifacts: `cargo test -p fl-dart-cli` (needs `FL_DART_SDK_BIN`) runs the L2
suite: hello world, run-loop draining, exception with stack trace, both FFI
crossings, version mismatch.

### First AOT run green; the ELF-loader premise was wrong (2026-10-27)

The AOT L2 suite passes 5/5 on x86_64 against the published run-7 archive:
hello world, run-loop draining (timers via dart:io's event handler),
exception with Dart stack trace, both FFI crossings, version mismatch.

Root cause of the first-native-call segfault was **not** the loader. Both
`dartaotruntime` (`main_impl.cc`) and the Flutter engine
(`kDartPrecompilationArgs`, `runtime/dart_vm.cc`) pass **`--precompilation`**
to `Dart_SetVMFlags` in the precompiled runtime; without it the VM runs AOT code
with JIT assumptions and crashes in `LinkNativeCall`. `Vm::init` now adds it in
AOT builds. Also needed: `Dart_SetVMFlags` must run once even with no flags.

Corrections to what commit 92beabf claimed: dlopen + dlsym of
`_kDartSnapshotData`/`_kDartSnapshotText` is fine, and is what Flutter does with
`libapp.so`, whose ELF has the same three symbols and 8-byte `.bss` stub as
`dart compile aot-snapshot` output. The `Dart_LoadELF` embedder crashed
identically for lack of `--precompilation`. `AotSnapshot` is back on dlopen,
with the use-after-dlclose (placeholder struct's `Drop`) fixed. The
`elf_loader` now in the archive is unused and can go at the next rebuild; the
shim's `enable_network_profiling = false` from the same commit *is* needed.

Flutter tooling: a `libapp.so` built the way `flutter_tools` builds it
(`frontend_server --target=flutter --aot --tfa` over
`flutter_patched_sdk_product`, then the release `gen_snapshot
--snapshot_kind=app-aot-elf`) runs through `fl-dart run-aot`. Its platform has
no `dart:_builtin`, so `print` comes from `dart:ui._getPrintClosure`, whose
`_print` calls `@Native` `DartRuntimeHooks::Logger_PrintString`; that symbol is
resolved through `Dart_SetFfiNativeResolver` on `dart:ui`. This is the first
engine native, done FFI-first (brief §13). Linux desktop Flutter does not use
compressed pointers (`tools/gn`: Android only), matching the archive.

### Cross lanes under qemu (2026-10-27)

* aarch64 and riscv64 archives run for the first time: L2 7/7 and fl-dart-vm unit tests 10/10 on both, under qemu-user
* FFI both directions passes on both, so `-rdynamic` export holds on all three targets
* `.github/workflows/cross.yml`: cross linker from `gcc-<triple>`, cargo target runner and `FL_DART_TEST_RUNNER` set to `qemu-<arch> -L /usr/<triple>`
* AOT programs compiled with `dart compile aot-snapshot --target-arch arm64|riscv64`, cross gen_snapshot fetched from dart-archive by the SDK
* Harness fix found on the way: tests compiling the same program raced on one output file. Compiles are now cached per process
* qemu is not a board. Pi 5 and VisionFive 2 runs still open
