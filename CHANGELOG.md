# Changelog

Not published, no versions yet.

## Unreleased

### Wonderous

* Dart plugins bound before `main`, as the engine binds them: `build.sh` compiles in the registrant `flutter build` generates, and fl-shell runs `_PluginRegistrant.register()`. A plugin whose Linux implementation is Dart alone (`path_provider_linux`, `shared_preferences_linux`) works
* `Isolate.spawn`, `Isolate.run` and Flutter's `compute`: an isolate in the program's group, set up as the root isolate is but without the embedder's dart:ui, as the engine refuses UI calls off the root isolate. `Isolate.spawnUri` is still refused
* `Uri.base`, the working directory, as the engine gives it to dart:core
* `apps/plugins` in the app lane
* `ColorFilter`, every kind, on a paint, an image, text and `saveLayer`, and `SceneBuilder.pushColorFilter`; `apps/color_filters` against the stock engine's golden
* `ImageFilter` (blur, dilate, erode, color, compositions) on a paint and through `saveLayer`, `SceneBuilder.pushImageFilter` and `pushBackdropFilter`; `apps/image_filters` against the stock engine's golden
* `SceneBuilder.pushShaderMask`; `Canvas.drawVertices` and `Vertices`; `apps/masks_and_meshes` against the stock engine's golden
* Every platform message is answered: what the Linux embedder has no handler or method for with an empty reply, which an optional channel reads as null, so a haptic, an orientation or a route report no longer waits forever. `flutter/platform`'s `SystemSound.play`, `System.initializationComplete`, `SystemNavigator.pop` and `System.exitApplication`, a cancelable exit put to the app first; leaving ends the loop. `apps/platform_calls` in the app lane
* `NetworkImage` over HTTPS, with the board's own trusted roots, as the stock engine's: the VM's TLS needed nothing. `apps/network_images` in the app lane, a loopback server with a certificate made by the test
* `PlatformDispatcher.requestDartPerformanceMode`: the VM's mode set, the one before returned, no idle time told the VM in the latency mode, as the engine does. Wonderous's screens to the details' four tabs, by a scripted tour in `docs/wonderous.md`
* `Path.combine` (`Path::op`): the paths flattened and combined by `i_overlay`, the result even-odd; `apps/path_ops` against the stock engine's golden
* emblema 0.4.0: a Raspberry Pi 4's display, a GBM buffer imported one row taller where v3dv reads past it; the advanced blend modes blended in the shader where the device's blend unit has none, which fl-raster asks for. Wonderous runs on the Pi 4's touch panel
* fl-raster says its first failed frame when it happens: a display run that stopped drawing looked hung until it ended
* `url_launcher_linux`'s two calls answered by fl-ui through a platform's launcher, by ivi-homescreen-plugins' rules: six schemes, `xdg-open` and its exit status, waited for off the UI thread with the reply sent when it exits. `run-app --open-urls` sets it; without it none is opened
* `build.sh` writes the app's `version.json` and lays the app out as a Linux bundle, which `run-app` takes: `package_info_plus` finds the app's version beside a copy of `fl-dart` in it
* Every Wonderous screen drawn, by scripted tours: the timeline, the menu, the collection, the About dialog with the app's version, the artifact search and details, the photo viewer. Its videos fail on any Linux, the app making a web view controller with no Linux implementation (`docs/wonderous.md`)
* `fl-dart prepare` builds the advanced blend modes' in-shader pipelines too, on a device whose blend unit has none: a Pi 4's first `colorBurn` no longer stalls about 630 ms building one. emblema 0.4.1 builds the ones the draws use, where 0.4.0 built source-over ones
* `apps/blend_modes`, the fifteen advanced blend modes on a solid paint, a gradient's rounded rectangle and a layer, against the stock engine's golden. emblema 0.4.2: lavapipe's advanced blend ops left unused, since they made every varying flat and a circle under `multiply` drew nothing; CI blends those modes in the shader, as the boards do
* `docs/platform-views.md`: a design note for platform views, proposed. A view drawn as a texture, `SceneBuilder.addTexture` and a texture registry, `flutter/platform_views` with view factories, then a web view on CEF for Wonderous's videos

### Milestone 4: input

* Engine-to-framework messages, each answered once: by the framework, or with null at shutdown
* `fl-input`: events as data in `embedder.h`'s shape, sent from any thread; the Linux embedder's pointer, scrolling and touch managers, its key path and its text input model, ported with their unit tests
* `fl_ui::pointer`: `PointerData` packets and the engine's packet converter; `flutter/keyevent`, `flutter/keyboard`, `flutter/textinput`, the clipboard on `flutter/platform`, `flutter/mousecursor`
* `fl-input-script` and `run-app --input`: scripted input for CI. `counter` tapped, `text_field`, `keys`, `pointer` and `scroll` match the stock framework given the same input
* `fl-platform-kms` and `run-app --kms`, behind the `kms` feature: the seat (libseat), the display as emblema's scanout output, frames begun at the display's blanks, the seat's keyboards, mice, touchpads and touch screens (libinput), XKB keymaps and compose tables, the mouse cursor on the card's cursor plane, terminal switches, hotplug. All through drmkit 0.1.6
* A touchpad's pinch as a zoom and a rotation; a mouse unplugged takes its pointer
* Board kit: `fl-dart-kms` and the display's check; `run.sh --input`
* Lanes: soak with input, input latency reported and held to two frames, fuzz targets for the script, the input state and the channel decoders, the apps with input under ASan and TSan
* The no-C++ lane builds `fl-dart` with `kms`: the four system libraries linked, no C built
* `apps/cursors`, a cell for every system mouse cursor, in the app lane; `run-app --kms --capture`: Print Screen writes what the display shows, the cursor with it, to a PNG
* Ten-bit scanout with red and blue in their places (emblema 0.3.0): they were exchanged on the Pi 5's screen
* A display controller apart from the GPU: where it takes no buffer of the renderer's, its own card's through GBM. `run-app --kms --mode WxH[@HZ]`
* A switch of terminals under an app at rest brings its frame back; a cursor near the top and left edges of a display that reads its buffer as it scans is whole (drmkit 0.1.6)
* VisionFive 2 (RISC-V): the board kit's ten checks pass, the display at 1280x720 and 60 Hz with no blank missed
* Raspberry Pi 5: 60 Hz with no blank missed, input latency under one frame
* `run-app --times`: where the frames' time went, the UI thread's builds and the raster thread's stages
* emblema 0.3.1: a frame's draw on the VisionFive 2 from 12 to 14 ms to 9, the pass's multisample and stencil buffers kept between frames; `text_churn` holds 60 Hz there
* A display's frame is handed to the GPU and not waited for (emblema 0.3.2): its fence goes with the commit, and the next frame is recorded while it is drawn. The raster thread's 9 ms a frame on the VisionFive 2 is 3.5. `--times` counts the commits the thread waited for itself
* The pipelines the driver compiles are kept between runs (emblema 0.3.3), in `fl-engine/pipelines` under the user's cache directory, for `run-app --kms` and `prepare`: on the VisionFive 2, whose driver keeps nothing, the first hover and click no longer wait most of a second each
* `fl-dart prepare` builds what an app's first frames draw with (emblema 0.3.4): rounded rectangles, shadows and layers besides solid colors and text, a clip's mask as it is drawn, for the offscreen format and a display's two. Run once on a VisionFive 2 with nothing kept, 13.9 s, a first run's longest frame is 11.5 ms; it was 830
* A release build is built for size: link-time optimized, `opt-level = "s"`, stripped. `fl-dart` on x86_64 from 15.0 MB to 10.4, 10.8 with `kms`
* The JPEG, GIF and WebP decoders are features of `fl-dart`, on by default; PNG is always read. An image in a format left out is refused by name. With none, `fl-dart` on x86_64 is 10.07 MB, 0.3 MB less
* The dictionaries Thai, Lao, Khmer and Burmese break by are the feature `dictionary` of `fl-dart`, on by default, 1.1 MB. With the decoders, the dictionaries and secure sockets all left out, `fl-dart` on x86_64 is 7.35 MB
* A Dart VM prebuilt without dart:io's secure sockets, `aot-nossl` for each target, and the feature `no-secure-sockets` that links it: no BoringSSL in `fl-dart`, 8.75 MB on x86_64 where the default is 10.37, 8.45 MB with the image decoders left out too. A TLS connection from Dart throws `ArgumentError`. Published with the pin's release, `cargo xtask fetch-prebuilt --mode aot-nossl`; the L2 suite runs on it too
* A board kit on that VM, `fl-dart-board-kit-<target>-nossl` (`tools/board-kit/build.sh ... nossl`), built and run under qemu by the cross lane for aarch64 and riscv64
* On that VM a refused TLS connection closes the socket it had opened: `SecureSocket.connect` and HTTPS threw `ArgumentError` and left it open, a descriptor for every attempt, and a script that tried did not exit. `SecurityContext()` no longer throws by itself; putting a key or a certificate into one does
* An app's first frame is recorded in 7 ms on the VisionFive 2 where it took 45: the glyph atlas is put on the device when the raster thread starts, a device's first image write being 40 ms there. A frame with no new glyph no longer sets up a font's hinting for each run of text, 1.2 ms of each of the counter's frames on that board
* A device takes its memory in blocks the size of its target (emblema 0.3.5): 64 MiB at 1280x720 where every device took 256 and 64 MiB whatever it used. On the VisionFive 2, whose driver backs a block with pages when it is made, a running app takes about 100 MB of the board's memory where it took 355, and the raster thread starts 130 ms sooner
* On a driver with one kind of memory, the Pi 5's, those blocks hold its images too (emblema 0.3.6): with 0.3.5 each came from a small block or had an allocation of its own, and a first draw took 9.6 ms. It takes 0.9, and a hover's longest draw 1 ms where it was 21 before either
* A display's run keeps what the app needed of the device's memory, in `fl-engine/memory` beside the pipelines, and the next run of it takes a block of that and one frame more (emblema 0.3.7): 96 MiB where it was 128 for an app with no layers on a Pi 5 at 1280x1440. `--times` prints a `memory:` line
* The frames in flight share one multisample and one stencil buffer (emblema 0.3.8), where each had its own: on a Pi 5 an app holds about half the GPU memory it did, 64 MiB where it was 122 for one with no layers at 1280x1440, and at 3840x2160 a hover makes one memory block in its frames where it made three
* An app that needs more than a memory block holds makes the pass's multisample and stencil buffers once it is first idle, and the block they need with them: at 3840x2160 on a Pi 5 the counter's hover drew in 2.2 ms at the longest where it took 78. `memory:` says how long it took, `ahead_ms`
* A run with no figure for the app takes a block of twelve of its target's frames where it took eighteen: no app in the tree allocates more than 8.3 since emblema 0.3.8. 88 MiB at 1280x1440 where it was 128, 44 at 1280x720 where it was 64

### Milestone 3: images and text (in progress)

* `fl-assets` and `build.sh` bundles: `flutter/assets`, `ImmutableBuffer`, asset keys confined to the bundle. `flutter create`'s counter app renders with no Dart error, text drawn as nothing
* `fl-image`: PNG, JPEG, GIF, WebP on one decode thread, behind a pixel limit; 22 image natives; GPU image cache in fl-raster. `images` matches its golden
* Impeller parity: goldens rendered with `--enable-impeller`; `drawShadow` as Impeller draws it; `FilterQuality.high` as mipmapped linear. `shadows` matches its golden

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

* dart:ui and rendering (milestone 2)
