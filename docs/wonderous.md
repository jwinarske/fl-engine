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

## Next

The next frame stops at `ColorFilter::Create`, an SVG's tint. What is known to
be missing in dart:ui, from reading the app; each is in emblema 0.3.8:

| dart:ui | Wonderous | emblema |
|---|---|---|
| `ColorFilter.mode`, `.matrix` on a paint | SVG icon tints, `Image.colorBlendMode`, flutter_animate's saturate and tint | `Paint::with_color_filter` |
| `ImageFilter.blur`, `pushImageFilter` | the carousel's blurred background, flutter_animate's `blur` | `save_layer_filtered` |
| `BackdropFilter`, `pushBackdropFilter` | the backdrop behind its dialogs | `save_layer_backdrop` |
| `ShaderMask`, `pushShaderMask` | flutter_animate's `shimmer` | a layer with a shader and a blend |
| `Canvas.drawVertices` | the time range on the artifact search | `draw_vertices` |
| `pushColorFilter` | `ColorFiltered` | a layer's color filter |

Then: `flutter/platform` methods answered (haptics, orientations) and
`flutter/navigation` (go_router's route reports); the network images over
HTTPS, with the board's own trusted certificates.
