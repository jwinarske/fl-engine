# M2: dart:ui — first frame through emblema

> Brief, 2026-10-28. The four decisions in §13 were taken the same day.
> Findings from bring-up go in `docs/m2-status.md`, newer where the two
> disagree.

**A Flutter app's `libapp.so`, built by the stock Flutter tooling, runs on fl-engine and its frames are drawn by emblema. dart:ui is bound FFI-first: `@Native` symbols resolved by `Dart_SetFfiNativeResolver`, no tonic.**

Sources, read at the pin (Flutter 3.47.8, engine `cf636b1d`, Dart `416e33b0`): engine `lib/ui`, `runtime/runtime_controller.cc`, `lib/ui/window/platform_configuration.cc`; framework `scheduler`, `rendering`, `widgets/binding.dart`, `services`. emblema 0.2.0 `docs/parity.md`, `docs/non-parity.md`, `docs/architecture.md`.

---

## 1. What M2 Is and Is Not

**Is:**
* `fl-dart-ffi`, the binding layer M1 previewed (§13 of the M1 brief)
* the dart:ui natives a framework app needs to reach its first frame and keep producing frames
* the embedder side of the engine contract: views, metrics, frame scheduling, the platform messages the framework waits on
* `Picture` and `Scene` as Rust data, replayed into an emblema `Canvas` on a raster thread
* the first frame offscreen in CI and on a KMS panel on a board

**Is not:**
* input (pointer, keyboard), text input, accessibility
* platform channels beyond the replies startup needs
* image decoding and codecs, `FragmentProgram`, platform views and external textures
* multiple views, a Wayland or X11 window
* hot reload, the VM service, plugins
* text (D3: first item of M3)

## 2. The Contract Being Implemented

The framework talks to the engine through two surfaces. Both are measured for a minimal app.

**Embedder to Dart, before `main`** (the order of `RuntimeController`, runtime_controller.cc:594-600):
* set `dart:ui` `_implicitViewId = 0`. Without it `implicitView` is null and `runApp` throws
* `_addView(0, …)` with 29 arguments. Width and height go in as tight constraints (`min = max`); zero constraints lay out a 0×0 box that paints nothing
* `_updateLocales`, `_updateSemanticsEnabled(false)`, `_updateAccessibilityFeatures(0)`, `_updateUserSettingsData("{}")`, `_updateInitialLifecycleState("AppLifecycleState.resumed")`, `_updateDisplays`

**Embedder to Dart, per frame after the first:** `ScheduleFrame` is answered with `_beginFrame(micros, frameNumber)`, then a microtask flush, then `_drawFrame()` (platform_configuration.cc:454-492).

**The warm-up frame needs neither of those.** `runApp` schedules it with `Timer.run`, which M1's run loop already drains. So the first frame is reachable before any vsync source exists.

**Dart to engine:** `@Native` symbols on `dart:ui`, which M1 already resolves through `Dart_SetFfiNativeResolver` (`DartRuntimeHooks::Logger_PrintString`).

## 3. The Native Set

From `main()` to the first `FlutterView.render`, for `runApp(ColoredBox(color: …))`:

| Native | Needs |
|---|---|
| `PlatformConfigurationNativeApi::GetRootIsolateToken` | a non-zero token. 0 sends the framework down the background-isolate path, which throws |
| `…::SendChannelUpdate`, `SetSemanticsTreeEnabled`, `SetNeedsReportTimings`, `EndWarmUpFrame` | stub |
| `…::SendPlatformMessage` | return Dart null. Reply asynchronously to `flutter/platform` with JSON `[null]` |
| `…::ScheduleFrame` | stub for the first frame; a vsync request after |
| `PictureRecorder::Create`, `Canvas::Create` | attach a native peer (`Dart_SetNativeInstanceField`, field 0) |
| `Canvas::drawRect` | record. Paint arrives as a 68-byte `ByteData` (painting.dart:1328-1365) |
| `PictureRecorder::endRecording`, `SceneBuilder::Create`, `pushTransformHandle`, `build` | record and attach peers to the out-objects |
| `SceneBuilder::addPicture`, `pop` | record |
| `…::Render(viewId, scene, w, h)` | the frame. w and h are physical pixels |
| `Scene::dispose`, later `Picture::dispose`, `EngineLayer::dispose` | free |

The rule this table implies: any native that takes a `Pointer<Void>` reads native field 0 of a `NativeFieldWrapperClass1`, so every constructor-style native must set it. `fl-dart-ffi` makes that a type rather than a convention (§6).

Beyond the first frame, M2 covers the drawing set of a plain widget tree: `drawRRect`, `drawPath`, `drawPaint`, `drawColor`, `drawCircle`, `drawLine`, clips, `save`/`saveLayer`/`restore`, transforms, and `pushOffset`/`pushClipRect`/`pushClipRRect`/`pushOpacity`. `MaterialApp` adds one blocking message: `flutter/restoration` `get` must be answered (null is enough) or the first frame never comes.

## 4. Rendering Through emblema

emblema's `docs/parity.md` is written against dart:ui's Canvas and Paint: 42 rows present, 5 a caller assembles, 0 absent, drawParagraph out of scope. What fl-engine supplies:

* **SceneBuilder.** emblema has no retained layer tree. fl-engine keeps the layer tree and flattens it each frame into one `Canvas`: transform and offset become `save` + `concat`, clip layers become `save` + `clip_*`, and opacity, color-filter and image-filter layers become `save_layer`. `pushBackdropFilter` maps to `save_layer_backdrop`
* **Picture.** A Rust display list, recorded on the UI thread and replayed into the frame's `Canvas` on the raster thread. Not an emblema `Recording`: a Recording is tessellated for one target size and one flattening tolerance, so it cannot stand in for a Picture that is drawn again under another transform
* **Composable gaps.** `clipRRect` via `to_rounded_path_with_radii`, `drawArc` via `PathBuilder::arc`, `skew` via `concat`, `restoreToCount` via `save_depth`
* **Missing in emblema.** `Path.combine`, `computeMetrics`, `contains`, `Path.transform`, the raster cache, wide gamut. None of these is on the M2 path
* **Expected differences.** emblema triangulates its fills and covers edges its own way (non-parity.md), so edge pixels will not match Impeller's exactly. Blending is in the target's encoding, as Skia's: shapes' opacity cell matches exactly (docs/m2-status.md). Goldens compare with a tolerance and record it

## 5. Presentation and the Frame Loop

* **Offscreen** (CI): emblema `OffscreenTarget`, read back as RGBA, compared to a golden. Vulkan on lavapipe, as emblema's CI already runs it
* **KMS** (boards): emblema `DrmScanoutTarget` over `KmsOutput`. The flip completes the frame; `pacing()` counts missed blanks
* **vsync**: on KMS the flip event drives `_beginFrame`; offscreen, a fixed-interval timer does
* **Threads**: Flutter's split. The UI thread owns the isolate and runs Dart. The raster thread owns the emblema `Context`, the target and the glyph atlas. Pictures and scenes move from one to the other as owned values. The isolate is `!Send` (M1); the M2 types that hold raster resources are `!Send`, and the ones that cross are `Send`, each checked by a compile-fail case

## 6. Crate Structure

```
crates/
  fl-dart-ffi    native binding layer: a symbol table the FFI resolver reads, the
                 native-peer types (attach, borrow, dispose), argument decoders.
                 No dart:ui knowledge
  fl-ui          the dart:ui natives: platform configuration, painting, compositing
  fl-shell       the embedder: view and metrics state, the runtime hooks of §2,
                 frame scheduling, UI and raster threads, platform message replies
  fl-raster      display list and layer tree to an emblema Canvas; targets
  fl-dart-cli    adds `fl-dart run-app <libapp.so> [--offscreen out.png | --kms <card>]`
```

emblema is a crates.io dependency (0.2.0, BSD-3-Clause, MSRV 1.85, no C toolchain), so fl-engine's floor stays at 1.85. drmkit is not a dependency in M2 (D2).

## 7. Peers and Lifetimes

* A peer is `Box<T>` attached to the wrapper object. It is freed by a finalizer, or by an explicit `dispose` native, whichever comes first. Never both: the M1 finalizable-peer test extends to a peer that is disposed and then collected
* `dispose` natives (`Picture`, `Scene`, `EngineLayer`) take the peer out of the object; later uses see null and fail as a Dart error, not UB
* Peers that the raster thread needs (`Picture`, `Scene`) are `Arc`'d immutable display lists, so handing a scene to the raster thread never borrows Dart memory
* Every native's body runs under `catch_unwind`, as in M1

## 8. Test Programs

Built with `flutter build linux --release`, using the Flutter release that pins the VM, as M1's libapp.so run was:

* `colored_box`: `runApp(ColoredBox(color: Color(0xFF2196F3)))`, the first frame
* `shapes`: rects, rounded rects, paths, a clip, an opacity layer, a transform
* `ticker`: an `AnimationController` driving a color, so frames after the first
* `material`: `MaterialApp` with no text, for `flutter/restoration` and the scheduler under the real widget library

## 9. Goldens

* References come from the real engine: `flutter test` with `matchesGoldenFile` renders each program with the stock engine at the same size and DPR
* fl-engine's offscreen frame is compared with a per-program tolerance, recorded beside the golden with the reason (edge coverage, AA differences)
* A golden that needs a wider tolerance than recorded fails. No silent skips, as in M1

## 10. Test Plan

* **L0**: peer attach, dispose and finalize; Paint `ByteData` decoding; argument decoders; display-list recording
* **Compile-fail**: raster types do not cross to the UI thread; a peer borrow does not outlive its native call
* **L2**: each program to its first frame offscreen, JIT and AOT; `ticker` produces N frames at the timer rate; `material` gets past restoration
* **Goldens**: §9, Vulkan on lavapipe
* **L5**: 1,000 app start and stop cycles with RSS, fds and handles flat; picture and scene churn over 10,000 frames; ASan and TSan with the M1 sanitizer VMs
* **Boards**: the board kit grows a `run-app --kms` check on Pi 5 and VisionFive 2, frames flip, `pacing()` reports 0 missed at 60 Hz

## 11. Exit Criteria

1. `colored_box` libapp.so renders its first frame through emblema offscreen, matching its golden, in CI
2. `shapes` and `material` match their goldens
3. `ticker` runs: `ScheduleFrame` → vsync → `_beginFrame` → microtasks → `_drawFrame`, frame count and spacing checked
4. The same programs flip on a KMS panel on Pi 5 and VisionFive 2
5. Every native in the M2 set has a test, and every stub is listed with what it returns
6. Thread placement enforced by types, each guarantee with a compile-fail case
7. Leak and sanitizer lanes green with the M2 programs
8. Still no C++ toolchain to build: ci.yml `no C++ toolchain` covers `fl-dart-cli` with M2 in it

## 12. Staged Path

1. **Binding layer.** `fl-dart-ffi` symbol table and peers; every §3 native present, stubs first. Done when `colored_box` reaches `Render` with no Dart error
2. **Recording.** `Canvas`, `PictureRecorder`, `SceneBuilder` record into Rust data. Done when the scene `Render` receives matches the expected op list
3. **First pixels.** Raster thread, flatten, emblema offscreen, golden. Exit criterion 1
4. **Frame loop.** vsync source, `_beginFrame`/`_drawFrame`, microtask flush. Exit criterion 3
5. **Breadth.** The rest of §3's drawing set, `material`. Exit criterion 2
6. **Boards.** KMS through emblema, board kit. Exit criterion 4

## 13. Decisions (2026-10-28)

* **D1, first target: offscreen, then KMS.** CI gets pixels with no display, and KMS is one target swap later. No Wayland or X11 window in M2: every Wayland client crate needs `cc` and `pkg-config`, which the no-C++ rule and emblema's own deny list refuse, and emblema's GLES path cannot reach a Wayland `EGLDisplay` today
* **D2, KMS stack: emblema's `DrmScanoutTarget`** (drm-rs). drmkit's `PageFlip` gives better vsync data (sequence and timestamp per flip), and emblema's `ScanoutOutput` trait is built for another binding, so a drmkit output is a later change behind the trait. Taking drmkit now would raise fl-engine's floor from Rust 1.85 to 1.94.1
* **D3, text: out of M2, first item of M3.** Shaping, line breaking, font fallback, rasterizing into emblema's atlas, `FontManifest.json` from the asset bundle. Pure-Rust candidates: rustybuzz, skrifa, swash. Until then no test program draws text
* **D4, backend: Vulkan first** (lavapipe in CI, V3D on Pi 5, the VisionFive 2's Vulkan driver), GLES second. emblema has both, so the second is a configuration change, not new code

## 14. Risks

* **Peer lifetime.** Two ways to free one peer (finalizer, dispose). Mitigation: §7 and its tests
* **Pixel parity.** Edge coverage differs from Impeller by design. Mitigation: per-program tolerances with recorded reasons
* **Microtask ordering.** A bare `Dart_Invoke` of `_beginFrame` does not drain microtasks before `_drawFrame`. Mitigation: flush explicitly between the two, as the engine does
* **Framework drift.** The native set is read at one Flutter release. Mitigation: pin it beside the Dart pin; a pin bump re-runs the §3 survey
* **Frame budget on boards.** First numbers come at stage 6. emblema measures a UI-like frame at 5.19 ms on Pi 5 V3D
