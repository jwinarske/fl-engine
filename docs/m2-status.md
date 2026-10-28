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

## Stage 2: the drawing set, shapes to its golden (2026-10-28)

Done: `shapes` records the expected scene, and its frame matches the stock engine's within the recorded tolerance.

    tools/flutter-app/build.sh $FLUTTER_ROOT apps/shapes out
    fl-dart run-app out/shapes.so 800x600@2 --ops --png shapes.png
    frame 1: view 0 800x600 layers=3 pictures=3 draws=22

* `apps/shapes`: one cell each for drawRect, drawRRect, drawPath, drawCircle, canvas clipRRect, stroked rect, canvas transform, pushClipRect over pushOpacity, drawLine with round caps. 400x300 logical at DPR 2
* Natives: 57 more, 79 in all. Not yet: Canvas images, atlases, vertices, points, shadows, superellipses, `getTransform`, clip bounds; Path `op`, `contains`, `extendWithPath*`, superellipses; SceneBuilder filters, shader masks, backdrops, textures, platform views, `addRetained`
* `drawArc` is recorded as the path it draws
* Paint: stroke cap, join, miter limit, mask blur, invertColors, and whether a shader or filter is set
* `fl-ui` Path: move, line, quad, conic, cubic, close. Rects, ovals, arcs and rounded rects become those as they are added; arcs and ovals as conics, exact. `arcToPoint` per SVG 1.1 F.6.5; Skia's implicit moveTo before a first segment and after a close
* `fl-ui` has no emblema dependency; `fl-raster` maps Path to `PathBuilder`, RRect to emblema's radii, all 29 blend modes by name
* Clip layers are `save` plus a clip; `antiAliasWithSaveLayer` adds a layer. Opacity layers are `save_layer`. Each pop restores to the depth its push started at
* Still a named error, not approximated: shaders, color and image filters, invertColors, color spaces other than sRGB, mask filters on a saveLayer
* `run-app --ops`: each frame's layers and draws by kind, no geometry. The app test checks it line for line
* Golden: `flutter test --update-goldens` in apps/shapes, the stock engine's flutter_tester at the same size and DPR. Committed with `shapes.tolerance` beside it: 1.0% of pixels may differ, 0.25% by more than 32
* Measured: 0.628% differ, 0.121% by more than 32. Edges only: every interior pixel matches, the opacity cell included (emblema blends in the target's encoding here, as Skia does)
* App lane runs `flutter test` on apps/shapes first: an exact compare, so a pin bump that changes the golden fails there
* L2-app: every native's argument count checked against the pinned dart:ui's `@Native` declarations

Found on the way:

* `pushClipRect` takes left, right, top, bottom, not l, t, r, b
* dart:ui's RRect corners are TL, TR, BR, BL; emblema's are TL, TR, BL, BR. Swapped in `fl-raster`
* dart:ui and emblema list blend modes in different orders: multiply is 24 in one and 14 in the other. Mapped by name
* `drawLine` strokes whatever the paint's style; with the fill default emblema drew nothing. Forced to stroke
* A non-finite path draws nothing and clips to nothing, as in Skia, rather than failing the frame
* Clipping is hard-edged in emblema; the clipped cell differs from the golden only along its edge
* Two app tests building one app ran build.sh at once into the same file; builds are now once per test process

## Stage 4: the frame loop (2026-10-28)

Done: `ticker` runs frames after the first, each asked for, begun at vsync, and animated on the vsync's time. Exit criterion 3.

    fl-dart run-app out/ticker.so 200x100 --frames 30 --png ticker.png
    frame 30: view 0 200x100 layers=1 pictures=1 draws=1
    vsync: frames=29 interval_us=16667,16667,66666,16667,…

* `fl-shell::run`: the UI thread's loop in place of `Dart_RunLoop`. Messages as the isolate's notify callback announces them, one `Dart_HandleMessage` each; a frame when one is asked for; done when the isolate has no live port and no frame pending, or when the caller's stop says so
* A frame: `_beginFrame(time, n)`, microtasks, `_drawFrame()`, microtasks, as `Animator::BeginFrame` and `Window::BeginFrame`. Microtasks through dart:isolate's `_runPendingImmediateCallback`, which a message handler runs itself and a direct call does not
* Vsync: a fixed clock, ticks one period apart (60 Hz from `ViewConfig::refresh_rate`). A request is served at the first tick at or after it, never twice on one tick; a late frame skips the ticks it missed. `_beginFrame` gets the tick's time, not the time it ran, so an animation is a function of ticks
* `ScheduleFrame` is no longer a stub: it asks the scheduler `fl_ui::set_frame_scheduler` installs. 7 stubs remain
* `fl-dart-vm`: `IsolateConfig::notify` (`Dart_SetMessageNotifyCallback`, set before the isolate is runnable), `HandleScope::handle_message`, `has_live_ports`, `run_microtasks`
* `fl-raster::RasterThread`: the `Offscreen` is made on the raster thread and stays there; scenes cross as `Arc<Scene>`. A queue of one beyond the frame being drawn, Flutter's pipeline depth of two. `Offscreen` is `!Send` (emblema's `Context` is `Send`, so the placement is decided here); compile-fail case `offscreen_crosses_threads`. `Scene`, `Picture`, `Frame` are `Send + Sync`, checked
* `run-app --frames N` stops after N frames rendered; a `vsync:` line gives the frame-time intervals
* `apps/frame_order`: dart:ui with no framework, printing its own sequence. Three frames, each `begin`, its microtask, `draw`, the render, the draw's microtask, then the next tick
* `apps/ticker`: an `AnimationController` over one second lerping red to blue. 30 frames, 29 at vsync; the last frame's color is the lerp at the vsync time since the ticker started, within 2 of 255
* L2-app checks: every interval a whole number of periods, on average no more than one tick missed per frame, the color from the reported times

Found on the way:

* `colored_box` now renders two frames: the warm-up frame, then the one the framework schedules while attaching the root widget. Stage 1 ended after the first because `ScheduleFrame` did nothing
* A frame's tick was first computed from the time the loop woke, not the time it was asked for. Woken just after the tick, it chose the next one, every time: the first build hung. Fixed by keeping the request time
* The `shapes` tests take `--frames 1`: they check the first frame
* The first frames on lavapipe can miss ticks while the raster thread warms up: one 4-period gap in 29 locally, the UI thread waiting on the full queue. Reported, not hidden

Next: `material` (`flutter/restoration`), then stage 6, KMS on the boards.

## Fixed: JIT isolate-cycle growth (2026-10-28)

* Soak JIT failed once: burst grew 664 kB over 900 cycles, limit 512. Linear, about 130 bytes per cycle, any program. AOT flat
* Cause: the non-product VM's default ring timeline recorder. `OSThread::SetName` calls `AddTrackMetadataBasedOnThread` (timeline.cc) for every new OS thread; the metadata is kept for the VM's lifetime
* Found with the ASan JIT VM's heap profile, 100 against 400 cycles: only those stacks scaled
* Fix: `Vm::init` passes `--timeline_recorder=none` ahead of `VmConfig::flags`, JIT only. The product VM (AOT) has no timeline and rejects the flag. The recorder is only readable through the VM service, which is off
* After: live heap +7.6 kB at 500 cycles, +9.6 kB at 2,000; ASan profile diff 0 bytes (burst: 3 chunks, a worker's buffers)
* Soak limit back to 512 kB for both modes. JIT soak locally: hello +20 kB, loop +8, ffi +24, burst +156
