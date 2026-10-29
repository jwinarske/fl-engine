# M3 status

Decisions and findings since the brief (docs/m3-images-text.md). Newer where the two disagree.

## Stage 1: assets (2026-10-29)

Done: `flutter create`'s counter app, unmodified, reaches `Render` with no Dart error, text drawn as nothing.

    tools/flutter-app/build.sh $FLUTTER_ROOT apps/counter out
    fl-dart run-app out/counter.so 800x600@2 --assets out/counter.assets
    frame 1: view 0 800x600 layers=3 pictures=3 draws=12

* `build.sh`: `flutter build bundle --release` beside the app, `<app>.assets/`, both modes. `run-app` takes `--assets`, else the directory beside the app
* `fl-assets`: `Bundle` over a `flutter_assets` directory. Keys confined to it: no empty key, NUL, absolute path or `..`; canonicalized and checked against the root; regular files only; 256 MiB limit, read through `take`
* `flutter/assets`: the key as UTF-8, the bytes or null. A refused key is a miss to the app, and said on stderr
* `ImmutableBuffer`: `init`, `fromAsset`, `fromFilePath`, `dispose`. Bytes an `Arc<[u8]>` peer, for the decode worker. Callbacks through the reply queue, as the engine's: `init` with void, the file natives with the length or -1
* `fromFilePath`: any path, as dart:io allows; regular files only, same limit. `/dev/zero` refused
* `FontLoader`: `LoadFontFromList` answers 0, the engine's answer; the font not yet registered
* `flutter/restoration` `get` answered: the counter app asks
* `FontManifest.json` parsed by `Bundle::fonts`, read when asked. Nothing asks until stage 4
* Text: all 28 ParagraphBuilder and Paragraph natives stubbed, an empty paragraph. `Canvas::drawShadow` stubbed, nothing drawn. Listed in `fl_ui::STUBS` with their stages
* `apps/assets`: each source, the misses, `../escape.txt`, `/nonexistent`, `/dev/zero`, `FontLoader`. The test checks its output line for line
* `apps/counter`: the template, unmodified. The test checks its scene and that nothing on stdout or stderr is an error
* Coverage test takes an exact list of natives no app reaches yet, each with its stage: 16 Paragraph metrics and queries and `addPlaceholder`

Found on the way:

* `_futurize` treats a null callback argument as failure; `init`'s callback needs `void` (`Dart_TypeVoid`)
* The engine reads `FontManifest.json` itself (font_collection.cc); the framework does not
* `flutter build bundle` does not tree-shake MaterialIcons: 1.6 MB in the counter bundle. `flutter build linux` does it with const_finder on the kernel; `build.sh` will before stage 5
* Asset and file reads are synchronous on the UI thread. The decode worker takes them in stage 2
