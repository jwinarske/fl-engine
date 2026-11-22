# Platform views

> Design note, 2026-11-22. Proposed; nothing here is built. Decisions are
> numbered PV1 to PV7 and marked **proposed** until taken.

**A widget that shows pixels fl-engine did not draw: a web view first, for
Wonderous's videos, then anything a plugin renders (a video decoder, a
camera, a 3D view). Drawn on a board's display with no compositor, and in CI
offscreen.**

Sources, read at the pin (Flutter 3.47.8, engine `cf636b1d`): the framework's
`services/platform_views.dart` (`PlatformViewsService`, the Android view
controllers and their `create` message), `widgets/platform_view.dart`
(`AndroidViewSurface`, `PlatformViewLink`), `rendering/platform_view.dart`;
`dart:ui`'s `SceneBuilder.addTexture` and `addPlatformView`. ivi-homescreen
`v2.0`'s `shell/platform/homescreen/platform_views` and ivi-homescreen-plugins
`v2.0`'s `plugins/webview_flutter_view` (CEF); tcna-packages'
`packages/webview/webview_flutter_linux`, the Dart side that registers a
`WebViewPlatform` on Linux.

---

## 1. Why

Wonderous's videos are YouTube iframes in a `webview_flutter` `WebView`.
`webview_flutter` needs a `WebViewPlatform` registered, Linux has none, and the
app throws before it draws one (`docs/wonderous.md`, "Every screen"). A web view
on Linux exists: tcna-packages' `webview_flutter_linux` registers one, and its
native half, ivi-homescreen-plugins' `webview_flutter_view`, drives the Chromium
Embedded Framework. Both speak Flutter's platform view protocol. fl-engine has no
part of it: no `flutter/platform_views` handler, no `SceneBuilder.addTexture`, no
`addPlatformView`. Each is a native the framework calls and fl-ui does not
register.

## 2. What the framework sends

There is no Linux platform view API in the framework. What exists, and what
ivi-homescreen and `webview_flutter_linux` both use, is the Android one, which
nothing in it restricts to Android:

* **`flutter/platform_views`**, a `StandardMethodCodec` channel. `create` with
  `id`, `viewType`, `direction`, `width`, `height`, `left`, `top`, `params` (the
  creation parameters, already encoded by the plugin's codec), and `hybrid` or
  `hybridFallback`; then `resize`, `offset`, `touch` (an Android `MotionEvent`
  as a list), `setDirection`, `clearFocus`, `dispose`. ivi-homescreen answers
  these seven, and `acceptGesture` and `rejectGesture` besides.
* **How the view is drawn depends on `create`'s reply.** A
  `SurfaceAndroidViewController` (`initSurfaceAndroidView`, which
  `webview_flutter_linux` uses) sends `hybridFallback: true`. If the reply is an
  `int`, the framework takes it as a texture id and draws the view as a
  `TextureLayer`, `SceneBuilder.addTexture`, inside the frame like a picture.
  Any other reply makes it fall back to hybrid composition, and the view becomes
  a `PlatformViewLayer`, `SceneBuilder.addPlatformView`, which the engine has to
  composite outside the frame.
* **Touch goes to the view as `touch` calls**, in the view's own coordinates,
  after the framework's gesture arena has given the view the gesture.

## 3. Decisions

**PV1, a view is a texture (proposed).** `create` answers with a texture id, and
the framework draws a `TextureLayer`. emblema draws it inside the frame, as it
draws an image: clipped, transformed, faded and blended like any other layer,
with nothing to place outside it. Hybrid composition needs a second surface
under or over the frame, a Wayland subsurface (what ivi-homescreen's web view
uses) or a KMS plane. The boards have no compositor, and a plane is a scarce,
driver-specific resource with its own scaling and format limits. Hybrid is left
for the Wayland backend (D8), where a subsurface is the natural answer.
`addPlatformView` is registered and draws nothing: a frame with one still draws,
and fl-shell says once that hybrid composition is not supported.

**PV2, `SceneBuilder.addTexture` and a texture registry (proposed).** fl-ui
registers `SceneBuilder::addTexture`, which records a texture op (id, offset,
size, `freeze`, filter quality) in the scene, as `addPicture` records a picture.
fl-shell keeps the registry: an id for each texture a producer registers, its
latest frame, and `mark_frame_available`, which schedules a frame. This is also
what the `Texture` widget needs on its own, for a plugin that has a texture and
no view.

**PV3, frames as CPU pixels first, dma-bufs next (proposed).** A producer hands
the registry either RGBA pixels with a stride, or a dma-buf (fd, fourcc,
modifier, planes). fl-raster uploads the first into the texture's one GPU image
and imports the second as an external image; emblema already imports a GBM
buffer's dma-buf, as a scanout target. CEF's off-screen `OnPaint` gives CPU pixels, which is where
ivi-homescreen starts too. `OnAcceleratedPaint`, a video decoder and a V4L2
camera give dma-bufs, and they come second. Each texture holds one GPU image,
replaced in place when its size is unchanged and freed on `dispose`: the cache is
bounded by the number of live textures.

**PV4, a view factory per `viewType`, in Rust (proposed).** A plugin registers a
factory for a `viewType` (`plugins.flutter.io/webview`). `create` calls it with
the id, size, direction and the raw `params`. It returns a view that owns its
texture and takes `resize`, `offset`, `touch`, `setDirection`, `clearFocus` and
`dispose`. A `create` for a `viewType` with no factory is answered with an error,
which the framework surfaces to the app as a `PlatformException`. Only the root
isolate's channel is served.

**PV5, the web view: CEF, in its own process (proposed).** CEF is the web engine
ivi-homescreen uses, with CPU frames from `OnPaint`. It runs its renderer, GPU
and utility processes beside the host; the browser side runs in fl-dart's
process, on a thread of its own, posting frames to the registry. It is a
separate crate behind a cargo feature, never in the default build: Chromium is
a large build with its own patches (see the plugin's README), and it has no
riscv64 build, so the VisionFive 2 does not get a web view. The native side of
`webview_flutter_linux`'s pigeon API (`cef_webview.g.dart`) is implemented in
that crate, against the same messages ivi-homescreen-plugins' C++ plugin
answers.

**PV6, Wonderous gets `webview_flutter_linux` through its build (proposed).** A
Dart plugin is registered only if it is in the app's dependencies. Wonderous does
not depend on any Linux web view, so `tools/flutter-app/build.sh` adds
`webview_flutter_linux` as a dependency override for that build, as a recorded
patch, rather than editing the app. The patch is one line of pubspec.

**PV7, the order (proposed).** Each step is a PR with an app in the lane and a
golden where the result is a picture:

1. `addTexture`, the registry and CPU frames (PV2, PV3): `apps/textures`, a
   `Texture` widget fed by a producer in fl-dart that draws a known pattern,
   clipped, transformed and faded; against a golden made by flutter_tester with
   an image in the texture's place.
2. `flutter/platform_views` and view factories (PV1, PV4): `apps/platform_views`,
   an `AndroidViewSurface` over a test factory's view, resized, moved and
   touched by a scripted input; what the view receives is printed and checked.
   `addPlatformView` registered, drawing nothing.
3. dma-buf frames (PV3): the same app with a dma-buf producer, on the boards.
4. CEF (PV5) and `webview_flutter_linux` (PV6): a page, then Wonderous's video,
   on the Pi 5 and Pi 4.

## 4. What it is not

* hybrid composition (a subsurface or a plane): the Wayland backend's, later
  (D8)
* iOS's `UiKitView` or macOS's `AppKitView` protocols, which the framework only
  sends on those platforms
* a web view on riscv64
* `flutter/platform_views_2` (`useNewController`), which nothing here uses

## 5. Open questions

* CEF's build: build it in CI per target (hours) or take a prebuilt binary
  distribution, and which one, with ivi-homescreen-plugins' patch.
* CEF's `OnPaint` frames are BGRA in system memory at the view's size; a 1280x720
  view at 30 frames a second is about 110 MB/s of uploads. On a Pi 4 that is
  worth measuring before step 4 commits to CPU frames.
* `webview_flutter_linux` is BSD-3-Clause, as the rest of tcna-packages: taken
  from there by git dependency, or vendored. ivi-homescreen-plugins states no
  license GitHub recognizes; its C++ plugin is read for the protocol, not
  copied.
