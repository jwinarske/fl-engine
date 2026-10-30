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

## Stage 2: images (2026-10-29)

Done: `images` matches its golden within the recorded tolerance (exit criterion 1). Decoding is never on the UI thread.

    tools/flutter-app/build.sh $FLUTTER_ROOT apps/images out
    fl-dart run-app out/images.so 800x600@2 --png images.png
    frame 1: view 0 800x600 layers=1 pictures=1 draws=16

* `fl-image`: header, first frame, the decode worker. PNG (png), JPEG (zune-jpeg), GIF (gif), WebP (image-webp); pure Rust, no `image` crate
* Frames are premultiplied RGBA8, as emblema samples textures; rounding as Skia's `SkMulDiv255Round`
* JPEG EXIF orientation applied, all eight, size reported as shown
* GIF first frame on its logical screen, transparent outside the frame
* Resize at decode: area average down, bilinear up. Not Skia's exactly
* Pixel limit 64 Mpx, `checked_mul`, on the source and the target size; header checked in `ImageDescriptor.encoded`, before anything is decoded. Every buffer `try_reserve`d; png, gif and webp given the same byte limit. PNG header reads limited to 16 MiB
* One worker thread, `fl-image`. Jobs carry an id and the isolate generation; the decode runs under `catch_unwind`, a panic is that job's error. Results go back to the UI thread through the reply queue, which now wakes the loop from another thread
* Generation ends with the run: queued jobs dropped unstarted, late results dropped, so nothing calls into a dead isolate. `fl_shell::run` discards on error too
* `ImageDescriptor` (7), `Codec` (4), `Image` (5), `ImageShader` (3), `Canvas` drawImage, drawImageRect, drawImageNine, drawAtlas. 22 natives
* `Codec`: single frame, as the engine's `SingleFrameCodec`: decoded on the first `getNextFrame`, kept, every call answered. `frameCount` 1 and `repetitionCount` 0 until stage 7
* `decodeImageFromPixels`: rgba8888 and bgra8888, premultiplied as given; the float formats refused
* `Image.toByteData`: an error, thrown. `colorSpace` sRGB
* `Image` peer `Arc<ImageData>`; a draw records the `Arc`
* fl-raster `GpuImages`: uploaded when decoded, ahead of the draw (`RasterThread::uploader`, `fl_ui::set_image_sink`), and by the frame if not; CPU copy released once uploaded; kept by `Weak`, destroyed the frame after the last reference goes. `!Send`, compile-fail case
* Mipmapped, so FilterQuality none, low, medium, high are nearest, linear, mipmap, cubic
* `ImageShader`: scale and translate only; one tile mode for both axes. Otherwise a named error
* drawAtlas colors with modulate only; drawImageNine with an opaque paint only. Otherwise named errors
* Raster queue: scenes still two deep, uploads never wait
* `app-cycle` reports `gpu_images` and `gpu_kb` after a final sweep; the soak asserts 0
* `apps/images`: every format, from bytes and from an asset buffer, a resize, raw pixels; drawImage, drawImageRect, nine-slice, atlas with colors, a shader fill, the four qualities, `Image.asset`. Prints the refusals: garbage, a 35-byte GIF claiming 65535x65535, a truncated PNG
* Golden: 10.411% differ, 0.235% by more than 32, lavapipe. Tolerance 12.0 and 0.4
* Lanes: golden in ci, images in soak (300 cycles, `--png`), valgrind and the sanitizers

Found on the way:

* `getNextFrame`'s callback takes three arguments; the reply queue carries them now
* The engine makes `_Image` with `Dart_New` and the private constructor `_`, so its fields are initialized
* `Image.asset` resolving a frame later made the frame count timing-dependent: 2 or 3. The app resolves it through `AssetImage` before `runApp`; the widget finds it in the cache

## Stage 3: shadows, and Impeller parity (2026-10-30)

Done: `shadows` matches flutter_tester's golden with Impeller within the recorded tolerance; the counter app's button casts its shadow.

    tools/flutter-app/build.sh $FLUTTER_ROOT apps/shadows out
    fl-dart run-app out/shadows.so 1200x900@3 --png shadows.png
    frame 1: view 0 1200x900 layers=1 pictures=1 draws=23

Parity target Impeller (D6): every golden re-rendered with `flutter test --enable-impeller`, on lavapipe here and in CI. flutter_tester fails rather than fall back when it cannot make a Vulkan context.

* `drawShadow`: Impeller's `DlDispatcherBase::drawShadow` in fl-raster, on emblema's mask blur. Tonal spot color at a quarter of the alpha; occluder `dpr * elevation`; the path moved down by that in device pixels; blur radius that over the y scale, `Radius` to `Sigma` as upstream converts; light radius `800 / 600`, integers, 1
* The DPR is view 0's, recorded with the draw (`fl_ui::set_device_pixel_ratio`, set by `fl_shell::prepare`), as canvas.cc reads it
* `FilterQuality.high` sampled linear between mip levels: Impeller has no cubic and takes `kCubic` as `kMipmapLinear`
* Goldens against Impeller, measured: shapes 0.226% differ, 0.028% over 32 (Skia: 0.628, 0.121); material 0.021, 0.007; natives 1.562, 0.209 (5.048, 0.335); images 10.746, 0.039 (10.411, 0.235); shadows 17.147, 0.002. Tolerances tightened to match
* `apps/shadows`: rects at elevations 1 to 16, a rounded rect, an oval, a colored shadow, a transparent occluder, two `PhysicalModel`s. 1200x900 at DPR 3
* `matches_golden_at`: size and DPR per app

Found on the way:

* emblema's `draw_shadow` is not Impeller's at this pin: it translates in local units (`PreConcat` puts Impeller's before the transform, so device pixels) and converts the radius before dividing by the scale. Not used; fl-raster draws the shadow itself
* emblema's normal mask blur is byte-identical to Impeller's at sigma 2, 4 and 8
* flutter_tester's engine DPR is 3 whatever `tester.view.devicePixelRatio` says; only `drawShadow` reads it. Goldens with shadows render at DPR 3 so both engines agree
* flutter_test draws a physical model's outline unless `debugDisableShadows` is false, reset before the test ends
