# Running Wonderous

[Wonderous](https://github.com/gskinnerTeam/flutter-wonderous-app), built with
`tools/flutter-app/build.sh` and run with `fl-dart run-app`, offscreen at
1280x720. It has no Linux runner of its own; it takes Linux for a desktop, and
what it cannot have there (maps, web views, YouTube, home-screen widgets, the
display mode) is behind its own platform checks or a deferred import.

Each step below is what stopped it, in the order it stopped.

## Start-up

* **Plugins.** `SharedPreferences.getInstance()` waited forever: the app was
  compiled without the registrant `flutter build` generates, and nothing ran
  it, so `shared_preferences_linux` was not bound and its method channel was
  unanswered. `build.sh` now compiles it in as flutter_tools' compile.dart does
  (`--source` and `-Dflutter.dart_plugin_registrant`), and fl-shell runs
  `_PluginRegistrant.register()` last before `main`, as the engine does in
  `RunFromLibrary`. `flutter pub get` writes the registrant only for a project
  with a platform's directory, `flutter build bundle` for any, so `build.sh`
  looks for it after the bundle. A desktop plugin's inline Dart class is its
  own implementation only if the plugin asks for Flutter 2.11 or later
* **`Uri.base`** threw: package:path asks for it to choose its style. dart:io's
  closure is now given to dart:core, as the engine's `InitDartCore` does
* **`Isolate.spawn`** was refused: flutter_svg parses each picture in
  `compute`. fl-dart-vm now gives the VM an initialize callback, and an isolate
  in the root's group gets the root's hooks and print sink and not the
  embedder's dart:ui natives: their replies are queued for the root isolate's
  loop, and the engine refuses UI calls off the root isolate too. The VM runs a
  spawned isolate's loop on its own threads. 300 cycles of `apps/plugins`, two
  `compute` calls each: resident memory within 16 kB over the last nine tenths,
  descriptors and threads the same

With those, the intro screen's first frames are drawn (7 layers, 67 draws).

## Color filters

`ColorFilter` (mode, matrix, both gammas) on a paint, an image draw, text,
`saveLayer`, and `SceneBuilder.pushColorFilter`, each mapped to emblema's
filter of the same kind. A mode's color stays sRGB-encoded, as the engine
gives it to `DlColorFilter::MakeBlend`; a matrix's last column is divided by
255, as `ColorFilter::initMatrix` divides it. A filter that does not decode
keeps none, and its draw or layer is refused by name; a layer is still
pushed, so the framework's pop has it. Text under a filter goes the path
route, as text under a shader does.

With them the intro screen is drawn: the logo, the arrows and the page
indicator are SVGs tinted by `ColorFilter.mode`.

Found on the way, in emblema: the analytic shapes (circle, oval, rounded
rectangle) and glyphs were filtered after their coverage, so a filter that
colors transparent black tinted the shape's whole quad, a square around each
circle. Impeller filters the color and then covers it
(`UberSDFContents::ApplyColorFilter`). Fixed there (jwinarske/emblema#275),
taken with emblema 0.3.9.

One difference is kept: Impeller filters a gradient's stop colors and
interpolates what comes out (`LinearGradientContents::ApplyColorFilter`),
which is the filtered gradient only for a filter affine in color; emblema
filters each pixel, as Skia does. Under `colorBurn` or a gamma curve the two
differ by up to 40 levels mid-gradient. `apps/color_filters` draws those
over a solid square.

## Image filters

`ImageFilter` (blur, dilate, erode, a color filter, compositions) on a
paint, through `saveLayer`, as `SceneBuilder.pushImageFilter` and as
`pushBackdropFilter`, each mapped to emblema's. A blur's deviations and a
morphology's radii go as dart:ui states them, in the space the filter is
used in, which is what emblema takes. A blur's tile mode is left to emblema,
which picks edges by what it filters, as the engine's dynamic tile mode
does. Refused by name: a blur's bounds, a matrix filter and a fragment
shader's. A composition is bounded at 16 deep, and its depth is kept, so a
chain that shares its parts is not walked. A backdrop is bounded by the
clip it is under, as the engine's layer is.

Found on the way, in emblema: a bounded backdrop held the whole frame shrunk
into its region, not the part of it behind the layer, so every frosted panel
showed a miniature of the page. Fixed there (jwinarske/emblema#276), taken
with 0.3.9 too. Known differences from Impeller, in compositions:
`apps/image_filters`' tolerance names them.

## Shader masks and meshes

`SceneBuilder.pushShaderMask`, as `ShaderMask` pushes it: the children in
a layer, then the shader across the mask rect in the layer's blend mode,
its origin at the rect's corner, as `ShaderMaskLayer::Paint` draws it.
Impeller clips that layer to what the children painted; fl-engine keeps no
picture bounds, so a mode that colors transparent black (multiply) fills
the rest of the mask rect here. The modes masks are used in (`srcIn`,
`dstIn`, `srcATop`, `modulate`) keep it so, and agree.

`Canvas.drawVertices` and `Vertices`, as Impeller draws them: with colors,
the paint at full strength blended with them by the call's mode and faded
by its alpha after; with colors and no shader, the colors alone, the mode
ignored (Skia's legacy `IGNORE_DRAW_VERTICES_BLEND_WITH_NO_SHADER`, which
the engine builds with). An image shader read at texture coordinates is
sampled through its matrix and its image's size. Texture coordinates
without a shader mean nothing and are dropped.

## Platform channels

Wonderous awaits what it asks the platform: a haptic before it navigates,
its orientations as it starts, and go_router's route reports on
`flutter/navigation`. fl-engine dropped every message it had no answer
for, so each such `await` never returned. The Linux embedder answers a
channel it has no handler for, and a method its `flutter/platform`
handler does not have, with an empty reply; `OptionalMethodChannel`, which
`flutter/platform` and `flutter/navigation` are, reads that as null, and
a plugin's own channel as `MissingPluginException`. fl-engine now does
the same for every message it does not answer otherwise.

`flutter/platform`'s methods are the Linux embedder's: the clipboard's,
`SystemSound.play` (no display's beep here), `System.initializationComplete`,
`SystemNavigator.pop`, which ends the loop, and `System.exitApplication`.
An exit the app may cancel, once it has said it is ready, is put to it
first as `System.requestAppExit`, and only a `cancel` keeps it.
`apps/platform_calls` makes each call through the framework, refuses one
exit and takes the next.

## Network images

Wonderous's photos and artifacts are JPEGs over HTTPS (`www.wonderous.info`,
`images.metmuseum.org`), loaded by `NetworkImage` through dart:io's client.
That needed nothing of fl-engine's: the VM's TLS is BoringSSL, and on Linux
it trusts the system's roots as the stock engine does, the bundle at
`/etc/pki/tls/certs/ca-bundle.crt` if there is one (Fedora), or else the
hashed directory `/etc/ssl/certs` (Debian, Ubuntu, Yocto's
`ca-certificates`). The SDK is built without fallback roots
(`dart_use_fallback_root_certificates` is false, as for the stock
engine), so an image with neither trusts nothing and every HTTPS load
fails its handshake: a board needs `ca-certificates`. What is installed
there with `update-ca-certificates` is trusted too.

Checked on this host and on the Pi 5 (Debian 13, 150 roots in
`/etc/ssl/certs`) with a dart:io probe run by `fl-dart run-aot`: both
Wonderous hosts answer 200, and a self-signed certificate
(`self-signed.badssl.com`) is refused with `CERTIFICATE_VERIFY_FAILED`.

`apps/network_images` checks the same in the app lane without the
network: it serves a JPEG from loopback over HTTPS with a certificate
the test makes for localhost. A client trusting the system's roots
refuses it; `NetworkImage`, through `HttpOverrides` with a client trusting
that certificate alone, loads and draws the photo; a missing file is a
404.

## Screens

Driven by `run-app --input` with the `kms` feature's keymap, from a
fresh preferences directory: shared_preferences keeps onboarding as done
under `$XDG_DATA_HOME/fl-dart`, and a second run would skip the intro.
The details screen animates without end, so the script waits by time
past it; `--frames` ends the run.

```text
# The intro's three pages by the arrow key, then into the app.
wait 1500
idle
key arrowRight
idle
key arrowRight
idle
key arrowRight
wait 3000
idle
# The chevron under the wonder's name: its details, the editorial.
tap 622 626
wait 3000
scroll 640 400 0 5
wait 1000
scroll 640 400 0 5
wait 1000
scroll 640 400 0 5
wait 3000
# The nav rail: photos, artifacts, events.
mouse move 34 364
mouse down 34 364
mouse up 34 364
wait 5000
mouse move 34 456
mouse down 34 456
mouse up 34 456
wait 5000
mouse move 34 546
mouse down 34 546
mouse up 34 546
wait 5000
```

    XDG_DATA_HOME=$(mktemp -d) fl-dart run-app wonders.so 1280x720 \
        --input tour.txt --keymap us --frames 2400 --frames-dir frames

The intro, the home screen, the Great Wall's editorial with its photo
over HTTPS, the photo gallery's Unsplash photos, the artifacts carousel's
Met objects and the events timeline are drawn.

* **`RequestDartPerformanceMode`** was missing: the framework asks for
  the latency mode around a route's transition, and the tap into the
  details threw. It sets the VM's mode as the engine does
  (`Dart_SetPerformanceMode`) and returns the one before; in the latency
  mode the loop tells the VM of no idle time, as `RuntimeController::
  NotifyIdle` tells it none
* **`Path::op`** (`Path.combine`) is missing: the photo gallery cuts the
  selected photo's rounded window out of a dimming rectangle, and that
  clip throws, so the gallery is drawn without its dimming

## Next

`Path::op`, then the rest of Wonderous's screens: the timeline, the
collection, the artifact search and details, the wonder menu.
