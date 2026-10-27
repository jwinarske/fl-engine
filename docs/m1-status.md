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

**The snapshot header also carries a feature string** right after the hash —
`product no-asan no-msan no-tsan ...` for the stock SDK's output. The VM
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

**`isolate_from_kernel` copies the dill.** The VM reads kernel lazily for the
group's whole life, so the bytes are owned by the group's data and released in
the VM's group-cleanup callback; a borrowed `&[u8]` could not promise that.

## Bring-up list (§15)

| Item | State |
|---|---|
| GN targets for the JIT / precompiled-runtime static libs | open — research in progress |
| Is `dart compile kernel` output directly consumable? | It includes the platform (8.6 MB for hello world; the platform dill's own SDK hash is `0000000000`). `isolate_from_kernel` creates the group from it and loads it as the script with `Dart_LoadScriptFromKernel`. To verify at first run. |
| VM snapshot blobs `Dart_Initialize` needs (JIT) | `Dart_Initialize` takes none in 3.13 (see above). Whether a JIT VM needs a core snapshot for the *group*: open |
| `print()` hook | `dart:_internal`'s `_printClosure` throws until set. Set, as `runtime/bin/dartutils.cc` does, to `dart:_builtin`'s `_getPrintClosure()`, with `Builtin_PrintString` resolved to Rust. Implemented; to verify that `dart:_builtin` survives AOT tree-shaking. |
| Symbol visibility for `DynamicLibrary.process()` | `-rdynamic` on the CLI's link, plus a `#[used]` table so the linker keeps the exports. To verify on all three targets. |
| Isolate flags | `Dart_IsolateFlagsInitialize` defaults, nothing set explicitly yet. `null_safety` is still a field (version 14 of the struct). Open. |
| `dart compile aot-snapshot` cross-targeting | open |

## Artifact pipeline

Not yet producible from this environment: `gclient` needs
`*.googlesource.com` and CIPD (`chrome-infra-packages.appspot.com`), both
refused by the session's network policy. `github.com` (git) and
`storage.googleapis.com` work, so the pinned SDK *source* and the pinned SDK
*binaries* are both available; only the dependency sync is not.

## What runs today

Without a VM (`FL_DART_SDK_OUT=none`): clippy on the whole workspace in both
modes, the header/port unit tests, and the seven compile-fail cases. With
artifacts: `cargo test -p fl-dart-cli` (needs `FL_DART_SDK_BIN`) runs the L2
suite — hello world, run-loop draining, exception with stack trace, both FFI
crossings, version mismatch.
