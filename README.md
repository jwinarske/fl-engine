# fl-engine

A Rust port of the Flutter engine, for embedded Linux.

> **Status: milestone 1 of the port, in progress.** The Dart VM is hosted from
> Rust: it starts, loads a program, runs `main()`, drains its message loop and
> shuts down cleanly, in both the precompiled (AOT) and JIT runtimes. A
> `libapp.so` built by the stock Flutter tooling runs and prints. Dart calls
> into Rust over FFI, and Rust posts messages back into Dart from any thread.
> All of that is tested on x86_64. The aarch64 and riscv64 VM builds exist but
> have not been run yet. There is no `dart:ui` beyond `print` and no rendering,
> so a real Flutter app stops at its first other `dart:ui` call, with an error
> that names it. That is the next milestone.

## What is here

```
crates/
  fl-dart-sys   bindings to the Dart VM's embedding API, linked from a prebuilt VM
  fl-dart-vm    the safe layer: Vm, Isolate, scopes, handles, ports, snapshots
  fl-dart-cli   the `fl-dart` binary, a thin shell over fl-dart-vm
xtask/          repository tooling: `cargo xtask bindgen | fetch-prebuilt`
tools/dart-prebuilt/
                the pipeline that builds the VM from dart-lang/sdk source
```

`fl-dart-vm` turns the embedding API's three crash-prone conventions into
types:

- An isolate is entered by one thread at a time, and `Isolate` is neither
  `Send` nor `Sync`.
- Entering returns a scope that exits on drop and cannot outlive the isolate.
- A handle borrows the API scope that made it, so it cannot escape that scope.

Each of these has a case under `crates/fl-dart-vm/tests/compile_fail` that must
not compile.

## The one C++ component

The Dart VM is C++ and the Dart SDK publishes no library to link it from, so it
is built from source at a pinned revision and consumed as a prebuilt archive.
Cargo never builds C++. `fl-dart-sys` locates the archive, checks its `VERSION`
against the revision its bindings were generated from, and links it, and that
is all it does. Each archive is self-contained: the VM, `dart:io`'s embedder
support (which `Timer` needs), ICU, zlib, BoringSSL and the toolchain's own
libc++. Linking one needs no C++ runtime on the target.

The archives come from `.github/workflows/dart-prebuilt.yml`, which runs
`tools/dart-prebuilt/build.sh` for x86_64 (JIT and AOT), aarch64 (AOT) and
riscv64 (AOT). They are published as a release. The checksums checked in at
`third_party/dart-prebuilt/<version>/SHA256SUMS` are what decide whether a
download is accepted.

The pin follows the Flutter release this tracks: the `dart_revision` in that
release's engine `DEPS`. It is currently Dart 3.13.7 (`416e33b0`), Flutter
stable.

## Building

The toolchain is pinned in `rust-toolchain.toml` (Rust 1.94.1, the version
Yocto's wrynose ships). The minimum supported version is 1.85.

Fetch the prebuilt VM for your host. The repository is private, so the fetch
needs a token that can read it:

```
GITHUB_TOKEN=... cargo xtask fetch-prebuilt --target x86_64-unknown-linux-gnu --mode aot
cargo build -p fl-dart-cli
```

AOT is the default. For the JIT runtime (x86_64 development hosts only), fetch
`--mode jit` and build with `--no-default-features --features jit`.

Everything that does not need to link the VM also builds without it.
`FL_DART_SDK_OUT=none` asks for that explicitly, and anything that then tries to
link fails loudly:

```
FL_DART_SDK_OUT=none cargo clippy --workspace --all-targets
FL_DART_SDK_OUT=none cargo test -p fl-dart-vm --features aot --lib --test compile_fail
```

## Running

Compile with the Dart SDK at the pinned version, then run:

```
dart compile aot-snapshot hello.dart -o hello.aot
fl-dart run-aot hello.aot

dart compile kernel hello.dart -o hello.dill     # JIT build of fl-dart
fl-dart run hello.dill
```

A `libapp.so` from the Flutter tooling runs the same way. That means the one
`flutter build linux --release` puts in the bundle's `lib/`, built with the
Flutter release whose engine pins the same Dart revision:

```
fl-dart run-aot build/linux/x64/release/bundle/lib/libapp.so
```

A snapshot from any other SDK is refused before it runs, with an error naming
both versions. Opening a snapshot runs its code, so only open snapshots you
would run as a binary.

## Tests

```
FL_DART_SDK_BIN=/path/to/dart-sdk/bin/dart cargo test -p fl-dart-cli
```

This compiles the programs under `crates/fl-dart-cli/tests/dart` with that
`dart` and runs them through `fl-dart`. The suite covers:

- hello world
- the run loop draining microtasks, timers and futures
- an uncaught exception arriving with its Dart stack trace
- Dart calling Rust and Rust posting to Dart from another thread
- a snapshot from another SDK being refused by name

CI runs the suite in both modes on every push to `main`.

## Documents

- [`docs/m1-fl-dart-vm.md`](docs/m1-fl-dart-vm.md): the milestone's brief.
- [`docs/m1-status.md`](docs/m1-status.md): what has been decided and found
  since. This is newer than the brief where they disagree.
- [`docs/m1-s15-findings.md`](docs/m1-s15-findings.md): the embedding questions
  the brief left open, answered from the Dart and Flutter source at the pin.

## License

BSD-3-Clause, matching the Flutter engine and the Dart SDK. Code ported from
either keeps its own copyright notice.
