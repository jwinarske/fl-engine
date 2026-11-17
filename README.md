# fl-engine

Rust port of the Flutter engine for embedded Linux.

Status: milestone 4 (input). A Flutter app's libapp.so, drawn and given input, with no display server.

* AOT and JIT runtimes
* Runs libapp.so from the Flutter tooling
* FFI both directions: Dart to Rust, Rust to Dart from any thread
* dart:ui drawn by emblema (Vulkan): shapes, images, text. Frames checked against the stock engine's
* Input: keys, text input, mouse, touch, touchpad and scrolling, from the seat's devices or a script
* KMS on a board: display, seat, input devices and the mouse cursor through drmkit
* Tested on x86_64, on aarch64 and riscv64 under qemu, and on a Raspberry Pi 5. VisionFive 2 not yet

## Crates

* fl-dart-sys - bindings to the Dart embedding API, links a prebuilt VM
* fl-dart-vm - safe layer: Vm, Isolate, scopes, handles, ports, snapshots
* fl-dart-ffi - native binding layer: symbol tables, peers on native-field wrappers
* fl-ui - dart:ui natives
* fl-shell - embedder: views and the runtime hooks a framework isolate needs
* fl-input - the input boundary: events as data, what a source delivers them through, and the Linux embedder's pointer managers, key path and text input, with the platform's input method and clipboard behind traits
* fl-input-script - scripted input source, for CI and tests
* fl-platform-kms - the KMS platform through drmkit: the seat, the display as emblema's scanout output, the seat's input devices, the mouse cursor on the card's cursor plane, its XKB keymap and its compose tables for dead keys (links libseat, libinput, libudev and libxkbcommon)
* fl-raster - dart:ui scenes drawn by emblema
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

Toolchain is pinned in `rust-toolchain.toml` (1.94.1, Yocto wrynose). MSRV 1.94.1, drmkit's.

Fetching needs no token. If `GITHUB_TOKEN` (or `GH_TOKEN`) is set it is sent, which raises the GitHub API rate limit:

    cargo xtask fetch-prebuilt --target x86_64-unknown-linux-gnu --mode aot
    cargo build -p fl-dart-cli

JIT (x86_64 dev hosts only):

    cargo xtask fetch-prebuilt --target x86_64-unknown-linux-gnu --mode jit
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

Flutter app, through dart:ui. Built with the pinned Flutter release's compilers; one line per frame:

    tools/flutter-app/build.sh /path/to/flutter apps/colored_box out
    fl-dart run-app out/colored_box.so 800x600@2

Drawn by emblema offscreen (Vulkan; Mesa's lavapipe works) and written as a PNG:

    fl-dart run-app out/colored_box.so 800x600@2 --png frame.png

Drawn to a display with no compositor (KMS, on a board), at the display's mode and rate. Built with `--features kms`; needs a seat (seatd, or a login on the console), libseat, libinput and libudev. The seat's keyboards, mice and touch screens are its input, and a mouse's pointer is drawn from the machine's cursor theme:

    fl-dart run-app out/counter.so @2 --kms

With `--capture shot.png`, Print Screen writes what the display shows, the cursor with it, to a PNG (`shot.png`, then `shot-2.png`):

    fl-dart run-app out/cursors.so --kms --capture shot.png

Notes:

* A snapshot from another SDK is refused, with both versions named
* Opening a snapshot runs its code. Only open snapshots you would run as a binary

## Test

    FL_DART_SDK_BIN=/path/to/dart-sdk/bin/dart cargo test -p fl-dart-cli

Covers hello world, run loop (microtasks, timers, futures), uncaught exception with Dart stack trace, FFI both directions, snapshot version mismatch. CI runs AOT and JIT on x86_64, and AOT on aarch64 and riscv64 under qemu, on every push to main.

Soak, release build. 1,000 isolate cycles per program, 100 VM lifecycles, valgrind (AOT, needs `valgrind`):

    FL_DART_SDK_BIN=/path/to/dart-sdk/bin/dart cargo test --release -p fl-dart-cli --test soak -- --ignored

Sanitizers, x86_64 JIT, pinned nightly (see `.github/workflows/sanitize.yml`):

    cargo xtask fetch-prebuilt --mode jit-tsan
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

    tools/board-kit/build.sh aarch64-unknown-linux-gnu /path/to/dart-sdk/bin/dart kit [/path/to/flutter]

With the pinned Flutter release named, the kit has the display's check too: `fl-dart-kms` (fl-dart with `--features kms`, linking the target's libseat, libinput, libudev and libxkbcommon through pkg-config) and the apps it shows.

On the board:

    tar xJf fl-dart-board-kit-aarch64-unknown-linux-gnu.tar.xz
    cd fl-dart-board-kit-aarch64-unknown-linux-gnu
    ./run.sh | tee report.txt
    ./run.sh --input | tee report.txt    # with someone at the board
    ./run.sh --mode 1280x720             # the display at one of its other modes

* One PASS/FAIL line per check: criteria 2-6. Exit 0 only if all pass
* The display: 300 frames of an app on KMS, none dropped and no blank missed. It needs a seat (seatd, or a login on the console) and no compositor on the card; a board with no display or no seat is a SKIP, said and counted
* `--input`: a key pressed, and a click or a touch, each within 20 seconds, reach the framework
* POSIX sh; `fl-dart` depends on nothing else on the board, `fl-dart-kms` on the four libraries
* MANIFEST holds the commit and every file's sha256
* Under qemu (FL_DART_TEST_RUNNER) RSS is reported, not asserted: qemu-user keeps about 39 kB per guest thread

## Docs

* [docs/m1-fl-dart-vm.md](docs/m1-fl-dart-vm.md) - milestone 1 brief
* [docs/m2-dart-ui.md](docs/m2-dart-ui.md) - milestone 2 brief: dart:ui, first frame through emblema
* [docs/m2-status.md](docs/m2-status.md) - milestone 2 progress and findings
* [docs/m3-images-text.md](docs/m3-images-text.md) - milestone 3 brief: images and text
* [docs/m3-status.md](docs/m3-status.md) - milestone 3 progress and findings
* [docs/m3-exit.md](docs/m3-exit.md) - milestone 3 exit criteria and their evidence
* [docs/m4-input.md](docs/m4-input.md) - milestone 4 brief: input, and KMS on the boards
* [docs/m4-status.md](docs/m4-status.md) - milestone 4 progress and findings
* [docs/m4-exit.md](docs/m4-exit.md) - milestone 4 exit criteria and their evidence. Open: VisionFive 2
* [docs/m1-exit.md](docs/m1-exit.md) - milestone 1 exit criteria and their evidence. Open: board runs
* [docs/m1-status.md](docs/m1-status.md) - decisions and findings since the brief. Newer where they disagree
* [docs/m1-s15-findings.md](docs/m1-s15-findings.md) - open embedding questions, answered from Dart and Flutter source

## License

BSD-3-Clause, see [LICENSE](LICENSE)
