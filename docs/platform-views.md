# Platform views

> Design note, 2026-11-22. Proposed; nothing here is built. Decisions are
> numbered PV1 to PV7 and marked **proposed** until taken.

**A widget that shows pixels fl-engine did not draw: a web view for
Wonderous's videos, a map, a 3D view, a camera, a video. Drawn on a board's
display with no compositor, and in CI offscreen. The plugins that render them
are Dart FFI plugins, and fl-engine hosts them through the same C ABI
ivi-homescreen v3 does, so a plugin built for one runs on the other.**

Sources: ivi-homescreen `v3.0.0`, `shared/` (`libihs_shared`, its README,
`docs/PLUGIN_ABI.md`, `include/ihs/platform_view.h` and
`platform_view_host.h`); the `ihs_pv` producers built against it: mapbox_view
(`native/src/mbv_ihs_pv_vk.cc`, `lib/src/mapbox_map_view.dart`),
maplibre_view, nav_map, fluorite, ihs_godot_view, ivi-homescreen-plugins'
`layer_playground_view` and ivi-homescreen's `test/integration/pv_bench`;
native_comms (zero-copy Dart FFI: direct calls on shared buffers,
`Dart_PostCObject` with external typed data and native pointers, SPSC rings).
The framework's `widgets/platform_view.dart` and `dart:ui`'s
`SceneBuilder.addPlatformView`, at the pin (Flutter 3.47.8).

---

## 1. Why

Wonderous's videos are YouTube iframes in a web view, and Linux has no web view
the app can register (`docs/wonderous.md`, "Every screen"). fl-engine has no
platform views at all: no `flutter/platform_views` handler and no
`SceneBuilder.addPlatformView`.

## 2. The model: ivi-homescreen v3's

Not platform channels. ivi-homescreen v3 split a platform view in three, and
only the first touches a channel:

* **Lifecycle on `flutter/platform_views`, and nothing else.** The Dart side is
  a `PlatformViewLink` over a `PlatformViewSurface`, with a
  `PlatformViewController` that sends `create` (id, `viewType`, size, `params`
  encoded by `StandardMessageCodec`), `resize`, `clearFocus` and `dispose`
  (mapbox_view's `_MapboxViewController`). The host's registry turns those
  into calls on the plugin's factory and callbacks.
* **The plugin's own traffic over Dart FFI, zero copy (native_comms).** The
  plugin's Dart library opens its native library and calls it directly:
  commands, gestures and bulk data as pointers into shared buffers, and
  replies and events posted back with `Dart_PostCObject` as external typed
  data or native pointers, never encoded or copied. No pigeon, no method
  channel per call. mapbox_view's camera gestures go this way.
* **Frames through `libihs_shared`'s `ihs_pv_*` C ABI.** The plugin registers a
  factory for its `viewType` (`ihs_pv_register_factory`), negotiates a surface
  path per view against what the host can grant (`ihs_pv_negotiate`: a dma-buf
  the host imports, a KMS plane, or host-allocated shared-memory buffers as the
  floor), and submits frames from its own thread (`ihs_pv_submit`), with
  fences, buffer ids for import caching, and `presented` feedback.

`PlatformViewSurface` puts a `PlatformViewLayer` in the scene,
`SceneBuilder.addPlatformView(id, offset, size)`: where the view goes and what
clips and transforms it. The host draws the view's newest submitted frame
there.

## 3. Decisions

**PV1, fl-engine hosts `libihs_shared` (proposed).** fl-engine loads
ivi-homescreen's `libihs_shared.so.1`, the library as built from `shared/`
(Apache-2.0, C, versioned, its ABI held by a libabigail baseline), and installs
its host into it with `ihs_pv_set_host` before any plugin loads, as the
ivi-homescreen shell does. The library is one copy of process state by design,
and a plugin reaches it by its SONAME, so fl-engine shipping the same library
is what makes a plugin portable between the two. Logging, tracing and config
read-back come with it: fl-engine starts the logging bridge, installs its trace
procs and publishes a config snapshot. Reimplementing the ABI in Rust would be
a second implementation to keep in step with each minor version, and is left
out.

**PV2, the host is Rust, in a new crate, fl-pv (proposed).** It implements
`IhsPvHost`: factory registration, capabilities, grants, `submit`,
`post_platform_task`, `retire_buffer`. fl-ui answers `flutter/platform_views`
from it (`create`, `resize`, `clearFocus`, `dispose`, `offset`, `touch`,
`acceptGesture`, `rejectGesture`) and calls the plugin's `IhsPvCallbacks`. The
platform thread is fl-dart's main thread, which is also the UI isolate's, so an
FFI plugin called from Dart is already on it and `ihs_pv_is_platform_thread` is
true there. `submit` is any-thread: it stores the view's newest frame and its
fences and schedules a frame. The registry keeps one entry per live view and a
bounded number of imported buffers per view (its ring, keyed by `buffer_id`,
dropped on `retire_buffer`, `renegotiate` and `dispose`).

**PV3, the view is drawn inline, in the frame (proposed).** fl-ui registers
`SceneBuilder::addPlatformView`, which records a view op (id, offset, size).
fl-raster draws the view's newest frame there as an image, so it is clipped,
transformed, faded and blended by the layers above it like anything else
(`IHS_PV_Z_INLINE`), with no second surface. That is what the boards need: KMS
with no compositor. A plane is PV5's.

**PV4, the grants, in order (proposed).** What `query_capabilities` offers,
and what each step adds:

1. `SOFTWARE_SHM`, the floor: the host allocates the ring in a memfd, the
   plugin fills it, fl-raster uploads it. Every plugin that handles the floor
   runs, including in CI on lavapipe.
2. `TEXTURE_DMABUF_IMPORT`: fl-raster imports the plugin's dma-buf with
   emblema's external images on its own device, zero copy, with the acquire
   fence waited on and a release fence returned.
3. `TEXTURE_VK_IMAGE`: a plugin renders on emblema's own `VkDevice`, from
   `ihs_pv_vulkan_context`. That needs emblema to expose its instance, device
   and queue, and to serialize submits on the shared queue as the ABI's
   interposed loader does.

`TEXTURE_EGL_IMAGE` and `ihs_pv_egl_context` are not offered: fl-engine has no
EGL backend.

**PV5, direct scanout later (proposed).** `DRM_PLANE`, a view's dma-buf on a KMS
overlay plane, skipping composition, is the faster path where it applies and
the one HDR video needs. It is emblema's scanout work and comes after PV4.

**PV6, the web view is an `ihs_pv` FFI plugin (proposed).** CEF, rendering off
screen, submitting its frames through `ihs_pv_submit` (CPU frames from
`OnPaint` on the floor, dma-bufs from `OnAcceleratedPaint` where it has them),
with navigation, JavaScript and its events over native_comms. It implements
`webview_flutter_platform_interface` in Dart so that `webview_flutter`, and so
`youtube_player_iframe`, find a `WebViewPlatform` on Linux. It is out of tree,
like the other producers, and runs on ivi-homescreen as well. Chromium has no
riscv64 build, so the VisionFive 2 has no web view.

**PV7, the order (proposed).** Each step a PR, with a producer that already
exists where one does:

1. `libihs_shared` loaded and the host installed, `flutter/platform_views`
   answered, `addPlatformView` drawn, the `SOFTWARE_SHM` floor:
   ivi-homescreen's `pv_bench` producer in the app lane, against a golden.
2. `TEXTURE_DMABUF_IMPORT`: the same producer exporting dma-bufs, on the boards
   and on lavapipe.
3. `TEXTURE_VK_IMAGE`, and a real producer: mapbox_view or maplibre_view on a Pi
   5.
4. The CEF web view (PV6), and Wonderous's video.
5. `DRM_PLANE` (PV5).

## 4. What it is not

* Platform channels for a plugin's own calls: those are FFI (§2)
* A texture registry and `SceneBuilder.addTexture` for platform views. The
  `Texture` widget is a separate, smaller piece, for a plugin that has a
  texture id and no view, and is not needed for any of the above
* A second surface (a Wayland subsurface): the Wayland backend's, later (D8)
* EGL contexts or images (no EGL backend)

## 5. Open questions

* How fl-engine gets `libihs_shared.so.1`: built from ivi-homescreen's
  `shared/` by fl-engine's build (CMake, C++17, a pinned tag), or taken from
  an installed package, as a plugin takes it.
* Which ABI minor fl-engine's host implements first. A plugin tests the minor
  and the weak symbols, so the host can start at the subset step 1 needs and
  grow, reporting the minor it serves.
* native_comms in fl-engine's VM: `Dart_InitializeApiDL` and
  `Dart_PostCObject_DL` are the VM's own and should work as they do under the
  stock engine. Step 1 confirms it with a producer that posts back.
