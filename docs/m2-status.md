# M2 status

Decisions and findings since the brief (docs/m2-dart-ui.md). Newer where the two disagree.

## Stage 1: binding layer (2026-10-28)

Done: `colored_box` reaches `Render` with no Dart error.

    tools/flutter-app/build.sh $FLUTTER_ROOT apps/colored_box out
    fl-dart run-app out/colored_box.so 800x600@2
    frame 1: view 0 800x600 layers=1 pictures=1 draws=1

* `fl-dart-ffi`: `Symbol` tables matched by name and argument count (a count mismatch stays unresolved rather than calling across a wrong ABI), `peer::attach`/`with`/`take` for native field 0, `guard` for `catch_unwind`
* Peers are tagged with their `TypeId`, read through a `repr(C)` header: a native given another class's object, or a disposed one, gets `None`
* A peer is freed once, by its finalizer. `dispose` natives take the value out first; the finalizer then frees an empty peer
* `fl-dart-vm`: `IsolateConfig::natives`, a per-isolate resolver for dart:ui `@Native`s; the FFI resolver is installed whenever `dart:ui` exists. `HandleScope::new_double`, `new_bool`, `new_f64_list`, `new_i64_list` (typed data, so `List<double>`/`List<int>` parameters accept them)
* `fl-ui`: the 21 natives of brief §3 plus `NativeSemanticsFlags::initSemanticsFlags`; 8 stubs, listed in `fl_ui::STUBS` with what each returns
* `fl-shell`: `_implicitViewId`, `_addView` (29 arguments, tight constraints), locales, semantics, accessibility, user settings, lifecycle, displays, in `RuntimeController`'s order
* `fl-dart run-app <libapp.so> [WxH[@dpr]]`: one line per frame; an app that renders none is an error
* `tools/flutter-app/build.sh`: libapp.so the way `flutter build linux --release` makes it (frontend_server `--aot --tfa`, release gen_snapshot), without the GTK shell
* ci.yml `Flutter apps (aot)`: Flutter 3.47.8 cloned at its tag, checked against commit `737ba8f` from Flutter's release index, Linux artifacts precached, cached between runs

Found on the way:

* The survey missed one native: `SemanticsFlags.none` is built while the root render object attaches, and its constructor is `@Native` `NativeSemanticsFlags::initSemanticsFlags` (25 arguments). Stubbed with an empty peer
* The scene is as the survey said: the root `TransformLayer`, one picture, one `drawRect`
* With `ScheduleFrame` a stub the app renders the warm-up frame and then has nothing left to do; the run loop returns and the process exits 0

## colored_box to PNG (2026-10-28)

Pulled forward from stages 2 and 3 for one app: its first frame as pixels.

    fl-dart run-app out/colored_box.so 800x600@2 --png colored_box.png

* All 480,000 pixels `(33, 150, 243, 255)`: `Color(0xFF2196F3)` exactly. A flat fill has no edge to anti-alias, so the test asserts every pixel
* Paint: the 68-byte `ByteData` decoded (painting.dart:1328-1365, little-endian). Stored against defaults: alpha as `1 - a`, anti-alias as "0 is on", blend mode XOR srcOver, so zero bytes are `Paint()`
* `fl-raster`: the scene replayed into one emblema `Canvas`. Transform and offset layers are `save` plus `concat_4x4`/`translate`; pictures replay in place; `pop` is `restore`; an unbalanced `pop` is ignored, as the engine's builder ignores it
* emblema 0.2.0 from crates.io, `vulkan` only; built against it unchanged. Offscreen RGBA8 surface, read back, written with the `png` crate. No C build dependency added
* A blend mode other than srcOver, or a color space other than sRGB, fails the frame and names itself. Not approximated
* Rastering runs in the frame sink on the UI thread. The raster thread comes with the frame loop (stage 4)
* App lane installs lavapipe (`mesa-vulkan-drivers`), as emblema's CI does

Next, stage 2 proper: the rest of the drawing set, and the scene checked op by op.

## Fixed: JIT isolate-cycle growth (2026-10-28)

* Soak JIT failed once: burst grew 664 kB over 900 cycles, limit 512. Linear, about 130 bytes per cycle, any program. AOT flat
* Cause: the non-product VM's default ring timeline recorder. `OSThread::SetName` calls `AddTrackMetadataBasedOnThread` (timeline.cc) for every new OS thread; the metadata is kept for the VM's lifetime
* Found with the ASan JIT VM's heap profile, 100 against 400 cycles: only those stacks scaled
* Fix: `Vm::init` passes `--timeline_recorder=none` ahead of `VmConfig::flags`, JIT only. The product VM (AOT) has no timeline and rejects the flag. The recorder is only readable through the VM service, which is off
* After: live heap +7.6 kB at 500 cycles, +9.6 kB at 2,000; ASan profile diff 0 bytes (burst: 3 chunks, a worker's buffers)
* Soak limit back to 512 kB for both modes. JIT soak locally: hello +20 kB, loop +8, ffi +24, burst +156
