# fl-engine

Rust port of the Flutter engine for embedded Linux.

Status: milestone 1 (Dart VM hosted from Rust).

* AOT and JIT runtimes
* Runs libapp.so from the Flutter tooling
* FFI both directions: Dart to Rust, Rust to Dart from any thread
* Tested on x86_64, and on aarch64 and riscv64 under qemu. Real boards not yet
* No dart:ui beyond print, no rendering. A real Flutter app stops at its first other dart:ui call

## Crates

* fl-dart-sys - bindings to the Dart embedding API, links a prebuilt VM
* fl-dart-vm - safe layer: Vm, Isolate, scopes, handles, ports, snapshots
* fl-dart-cli - `fl-dart` binary
* xtask - `cargo xtask bindgen`, `cargo xtask fetch-prebuilt`

Isolate thread affinity, enter/exit pairing and handle lifetime are compile errors when misused. See `crates/fl-dart-vm/tests/compile_fail`.

## Dart VM

Built from dart-lang/sdk at a pinned revision, consumed as a prebuilt static archive. Cargo does not build C++.

* Pin: Dart 3.13.7 (`416e33b0`), the `dart_revision` in Flutter stable's engine DEPS
* Targets: x86_64 (jit, aot, jit-asan, jit-tsan), aarch64 (aot), riscv64 (aot)
* One archive per target/mode: VM, dart:io embedder, ICU, zlib, BoringSSL, libc++. No C++ runtime needed on target
* Built by `.github/workflows/dart-prebuilt.yml` (`tools/dart-prebuilt/build.sh`), published as a release
* Downloads are checked against `third_party/dart-prebuilt/<version>/SHA256SUMS`

## Build

Toolchain is pinned in `rust-toolchain.toml` (1.94.1, Yocto wrynose). MSRV 1.85.

The repo is private, so fetching needs a token:

    GITHUB_TOKEN=... cargo xtask fetch-prebuilt --target x86_64-unknown-linux-gnu --mode aot
    cargo build -p fl-dart-cli

JIT (x86_64 dev hosts only):

    GITHUB_TOKEN=... cargo xtask fetch-prebuilt --target x86_64-unknown-linux-gnu --mode jit
    cargo build -p fl-dart-cli --no-default-features --features jit

Without a VM (lint, docs, compile-fail tests):

    FL_DART_SDK_OUT=none cargo clippy --workspace --all-targets
    FL_DART_SDK_OUT=none cargo test -p fl-dart-vm --features aot --lib --test compile_fail

## Run

Use the Dart SDK at the pinned version. `fl-dart` is `target/debug/fl-dart`. The AOT and JIT builds write the same binary, so rebuild when switching.

    dart compile aot-snapshot hello.dart -o hello.aot
    fl-dart run-aot hello.aot

JIT build:

    dart compile kernel hello.dart -o hello.dill
    fl-dart run hello.dill

libapp.so from `flutter build linux --release`, using the Flutter release that pins the same Dart revision:

    fl-dart run-aot build/linux/x64/release/bundle/lib/libapp.so

Notes:

* A snapshot from another SDK is refused, with both versions named
* Opening a snapshot runs its code. Only open snapshots you would run as a binary

## Test

    FL_DART_SDK_BIN=/path/to/dart-sdk/bin/dart cargo test -p fl-dart-cli

Covers hello world, run loop (microtasks, timers, futures), uncaught exception with Dart stack trace, FFI both directions, snapshot version mismatch. CI runs AOT and JIT on x86_64, and AOT on aarch64 and riscv64 under qemu, on every push to main.

Soak, release build. 1,000 isolate cycles per program, 100 VM lifecycles, valgrind (AOT, needs `valgrind`):

    FL_DART_SDK_BIN=/path/to/dart-sdk/bin/dart cargo test --release -p fl-dart-cli --test soak -- --ignored

Sanitizers, x86_64 JIT, pinned nightly (see `.github/workflows/sanitize.yml`):

    GITHUB_TOKEN=... cargo xtask fetch-prebuilt --mode jit-tsan
    FL_DART_SDK_BIN=/path/to/dart-sdk/bin/dart RUSTFLAGS=-Zsanitizer=thread \
      cargo +nightly-2026-10-13 test -Zbuild-std \
      --target x86_64-unknown-linux-gnu -p fl-dart-cli --no-default-features --features jit

`-Zsanitizer=address` with `jit-asan` for ASan.

Cross, under qemu (see `.github/workflows/cross.yml`):

    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER="qemu-aarch64 -L /usr/aarch64-linux-gnu" \
    FL_DART_TEST_RUNNER="qemu-aarch64 -L /usr/aarch64-linux-gnu" \
    FL_DART_SDK_BIN=/path/to/dart-sdk/bin/dart \
    cargo test -p fl-dart-cli --target aarch64-unknown-linux-gnu

## Board run

Exit criterion 2 on Raspberry Pi 5 and VisionFive 2. The cross lane builds a kit per target and keeps it 90 days (artifact `fl-dart-board-kit-<target>`), or build it:

    tools/board-kit/build.sh aarch64-unknown-linux-gnu /path/to/dart-sdk/bin/dart kit

On the board:

    tar xJf fl-dart-board-kit-aarch64-unknown-linux-gnu.tar.xz
    cd fl-dart-board-kit-aarch64-unknown-linux-gnu
    ./run.sh | tee report.txt

* One PASS/FAIL line per check: criteria 2-6. Exit 0 only if all pass
* POSIX sh, no other dependency on the board
* MANIFEST holds the commit and every file's sha256
* Under qemu (FL_DART_TEST_RUNNER) RSS is reported, not asserted: qemu-user keeps about 39 kB per guest thread

## Docs

* [docs/m1-fl-dart-vm.md](docs/m1-fl-dart-vm.md) - milestone 1 brief
* [docs/m2-dart-ui.md](docs/m2-dart-ui.md) - milestone 2 brief, draft: dart:ui, first frame through emblema. Open decisions in §13
* [docs/m1-exit.md](docs/m1-exit.md) - milestone 1 exit criteria and their evidence. Open: board runs
* [docs/m1-status.md](docs/m1-status.md) - decisions and findings since the brief. Newer where they disagree
* [docs/m1-s15-findings.md](docs/m1-s15-findings.md) - open embedding questions, answered from Dart and Flutter source

## License

BSD-3-Clause, see [LICENSE](LICENSE)
