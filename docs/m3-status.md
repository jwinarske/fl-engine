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
* Goldens against Impeller, measured: shapes 0.226% differ, 0.028% over 32 (Skia: 0.628, 0.121); material 0.021, 0.007; natives 1.562, 0.209 (5.048, 0.335); images 10.746, 0.039 (10.411, 0.235); shadows 17.147, 0.002, then 6.290, 0.002 with shapes kept (below). Tolerances tightened to match
* `apps/shadows`: rects at elevations 1 to 16, a rounded rect, an oval, a colored shadow, a transparent occluder, two `PhysicalModel`s. 1200x900 at DPR 3
* `matches_golden_at`: size and DPR per app

Found on the way:

* emblema's `draw_shadow` is not Impeller's at this pin: it translates in local units (`PreConcat` puts Impeller's before the transform, so device pixels) and converts the radius before dividing by the scale. Fixed in jwinarske/emblema#227; fl-raster drew the shadow itself until then, and calls `draw_shadow` with `dpr * elevation` from emblema `657035f` on
* emblema's normal mask blur is byte-identical to Impeller's at sigma 2, 4 and 8
* flutter_tester's engine DPR is 3 whatever `tester.view.devicePixelRatio` says; only `drawShadow` reads it. Goldens with shadows render at DPR 3 so both engines agree
* flutter_test draws a physical model's outline unless `debugDisableShadows` is false, reset before the test ends
* A shadow's rect, uniform rounded rect or circle reaches emblema as that shape, so it is blurred analytically, as Impeller's `SimplifyOrDrawPath` sends it. Built from segments, emblema could not tell and ran its general blur, about 0.87 of the sigma in effect: 1 to 3 levels off across every falloff. Against Impeller's formula in double precision the stock engine is within 0.6 levels; emblema's analytic route within 1. Ovals that are not circles keep emblema's blur; Impeller draws them as a shadow mesh, which emblema does not build
* The analytic route drew a colored blur unpremultiplied (jwinarske/emblema#233, fixed in #234); black hid it

## Stage 4: layout (2026-10-30)

Done: `metrics` matches flutter_tester with Impeller: 748 lines, every number equal at three decimals, within the half pixel the test allows.

    tools/flutter-app/build.sh $FLUTTER_ROOT apps/metrics out
    fl-dart run-app out/metrics.so 200x100 --frames 1
    time: 39 paragraphs, 422 us each

* `fl-text`: SkParagraph at Flutter's Skia pin, ported: builder, `ParagraphImpl`, `Run`, `Cluster`, `OneLineShaper`, `TextWrapper`, `TextLine`. UTF-8 inside, UTF-16 at the queries. No dart:ui, no emblema
* Shaping: harfrust, monotone character clusters, 16.16 scale as SkShaper. A feature that covers the text is global, as SkShaper passes it
* Unicode: unicode-linebreak, unicode-bidi, unicode-segmentation in place of ICU; ICU's space, whitespace, control and ideographic classes as tables
* Fonts: `loadFontFromList`, then `FontManifest.json` (read on first match), then the system's (fontdb, files read, not mapped). Faces matched by CSS rules, slant first. Fallback per code point from the system only, cached by code point and style
* Unresolved text: the first face's `.notdef`, counted
* dart:ui: 27 natives on fl-text. `Paragraph::paint` still a stub: glyphs are stage 5
* Limits: text shaped in pieces past 32 KiB, split after a space; shaped runs cached by face, variations, size, text, direction, locale and features, 4096 entries then cleared; harfrust's `max_ops` and `max_len` on, its defaults; fonts parsed under `catch_unwind`, and only with head, maxp and cmap; shaping under `catch_unwind`, a panic is the text unresolved
* `apps/metrics`: 39 paragraphs in Roboto from the Flutter cache. Sizes, weights, italic, height and leading distribution, letter and word spacing, maxLines and ellipsis, every alignment, rtl, strut, text height behavior, features, placeholders of every alignment. Measures, line metrics, boxes in every height style, positions, word and line boundaries, line numbers, glyph info
* Format drift: `run-app --styles` prints each style as decoded; one paragraph and two text styles with every field set, held against `test/styles.txt`
* Coverage: every native reached by an app; `NOT_YET` empty
* fl-text tests: limits and font parsing everywhere; layout in Roboto in the app lane

Not yet:

* Script itemization: a style block is shaped as one run, its script guessed from its text. Mixed-script blocks shape apart from SkShaper's
* ICU's line-break and word tailorings: UAX #14 and #29 as written
* Soft hyphens are not rendered (Flutter does not ask)

Found on the way:

* Skia's `ParagraphCache` keys a paragraph without its leading distribution: the stock engine answers an even-leading paragraph with the runs of an earlier proportional one of the same text. `metrics` gives that case its own text
* harfrust's `Feature::new` takes an exclusive end one short; the struct is built directly
* A table directory with no tables parses as a font in read-fonts
* `loadFontFromList` sends `fontsChange` on flutter/system; before the binding is up the message is dropped with a warning. `metrics` initializes the binding first
* dart:ui encodes `TextStyle.textBaseline`; the engine reads it and drops it, and so does fl-text

## Stage 5: glyphs (2026-10-31)

Done: `text` matches flutter_tester's golden with Impeller within the recorded tolerance (exit criterion 2); the counter app draws its title, label, count and icon.

    tools/flutter-app/build.sh $FLUTTER_ROOT apps/text out
    fl-dart run-app out/text.so 1600x1200@2 --png text.png
    frame 1: view 0 1600x1200 layers=1 pictures=1 draws=58

* fl-text paints as `DisplayListParagraphPainter` records with Impeller (`paint.rs`): backgrounds as rects, shadows as blurred glyph runs, text as glyph runs, decorations as filled rects (solid, double), dashed stroked lines (dotted, dashed) and a stroked wave, inside saves, translations and clips. `TextLine::paint`, `iterateThroughSingleRunByStyles` and `Decorations` ported
* fl-ui: `Paragraph::paint` records `Draw::Glyphs` and the rest as ordinary draws; a dashed line as `DashedLinePathSource` cuts it
* fl-raster `Glyphs`: swash rasterizes at the device size; coverage into emblema's `Atlas` (512, growing to 4096); runs drawn with `draw_glyphs`. `!Send`, compile-fail case
* A frame replays twice: the first rasterizes and inserts every glyph, the atlas uploads once, the second draws. The atlas may repack or grow on insert, and a run recorded before would read stale places
* Outlines, drawn as a path, for a glyph past 256 device pixels (Skia's `kSkSideTooBigForAtlas`), a paint with a shader or a stroke (`ShouldRenderAsPath`), a transform with perspective, and a glyph the atlas refuses: text never goes missing
* Fallback determinism: fl-ui says on stderr when a code point is drawn in a system fallback face; `text_draws_no_fallback_glyph` holds the golden program to none
* swash 0.2.10 builds on 1.85, the floor
* `apps/text`: sizes, weights, italic, color, letter and word spacing, alignment, rtl, ellipsis, wrapping, every decoration style, background, hard and blurred shadows, stroked text, three MaterialIcons glyphs, mixed spans, a 160 px pair past the atlas. 1600x1200 at DPR 2
* Golden: 3.132% differ, 1.365% by more than 32, lavapipe. Tolerance 4.0 and 1.6

Not yet:

* Glyph edges: Impeller rasterizes through Skia with slight hinting, quarter-pixel subpixel positions and Skia's coverage; swash here, unhinted, on whole pixels. Small text is 5 to 9% heavier in ink; positions agree within a third of a pixel; outlines and icons agree
* Gradient-filled text: dart:ui's `Gradient` natives are not in fl-ui; the text program has none
* A frame needing more glyphs than the atlas's limit draws the rest as outlines rather than in a second pass
* The first replay costs a second walk of the scene each frame

Found on the way:

* The empty cluster at the end of the text is a whitespace and an intra-word break, as `Cluster`'s constructor makes it (zero of zero code units); built otherwise, a one-word line kept it in its range and painted no glyphs

## Stage 6: counter (2026-10-31)

Done: `flutter create`'s counter app, unmodified, matches flutter_tester's golden with Impeller within the recorded tolerance (exit criterion 3): app bar title, body text, the count, and the button with its icon and shadow.

    R=$FLUTTER_ROOT/bin/cache/artifacts/material_fonts
    fl-dart run-app out/counter.so 1200x900@3 --font Roboto=$R/Roboto-Regular.ttf --font MaterialIcons=$R/MaterialIcons-Regular.otf --png counter.png
    frame 1: view 0 1200x900 layers=3 pictures=3 draws=17

* The app ships no Roboto, and flutter_tester draws text in its test font unless a test loads one. Both engines are given the same files: the golden test with `FontLoader`, fl-engine with `run-app --font FAMILY=PATH` (`fl_ui::load_font`, as `loadFontFromList`)
* The golden test sets the platform to Linux, as fl-engine reports it, so both use the same typography; DPR 3, flutter_tester's engine DPR, for the shadow; the DEBUG banner off, as the release build has none
* `lib/` is the template's; `test/golden_test.dart` is added beside its `widget_test.dart`, and CI's goldens step runs both
* Golden: 1.423% differ, 0.550% by more than 32, all at glyph edges (stage 5). Tolerance 2.0 and 0.8

## Stage 7: animation (2026-10-31)

Done: `gif` advances through its frames at their durations (exit criterion 4); every frame, each disposal applied, matches flutter_tester's.

    tools/flutter-app/build.sh $FLUTTER_ROOT apps/gif out
    fl-dart run-app out/gif.so 400x300 --frames 16 --frames-dir gif
    dispose.gif: frames=5 repeat=2
    frame 1: view 0 400x300 layers=1 pictures=1 draws=13

* fl-image `Frames`: frame count and repetition count read without decoding (a GIF's frame headers, its pixel data skipped; a WebP's chunks); frames decoded one at a time on the worker (`Source::Next`), composed on the image's canvas after the previous frame's disposal, premultiplied. Held: the canvas, and for restore-to-previous, the canvas before
* Disposal as Skia and `MultiFrameCodec`: background clears to transparent, the color unused; previous before any frame is transparent; after the last frame, the first on a clear canvas
* Repetition count: GIF NETSCAPE n is n, 0 is forever (-1), none is 0; WebP loop n is n - 1, 0 is forever. Durations as the file says, 0 kept
* WebP frames composed here, each frame's bitstream decoded by image-webp as an image of its own: image-webp's own blend copies no opaque pixel, rounding 255 to 254, where libwebp and Skia copy it
* fl-ui `Codec`: more than one frame, `MultiFrameCodec`'s rules: each `getNextFrame` decodes the next, in turn, at the image's own size (target sizes are for stills, as the engine's); nothing kept on the UI side. `getNextFrame` answers with the frame's duration
* Limits: frame count 1,048,576 (`Limits::max_frames`), refused as the codec's every answer; each frame's own size held to the pixel limit before it is decoded, a GIF's first frame decoded alone too; buffers `try_reserve`d; a worker panic mid-frame starts the animation over
* `apps/gif`: hand-written GIFs (`tool/assets.py`) for every disposal, a loop count, none, a frame past the screen; an animated WebP; the steps animation through `Image.asset`. The report (`test/report.txt`) is the stock engine's, line for line; the frames are pixel-exact (tolerance 0)
* The golden is the first frame the steps show in, their first, red for 100 ms: when `Image.asset` finishes loading depends on the machine (the warm-up frame on a fast one), so both engines are held to that frame
* `gif_advances_at_its_durations`: the steps change in order, each after its frame's duration, at most three vsync periods late, timed from the frames run-app renders

Found on the way:

* The stock engine fails a WebP animation frame with alpha after the first ("Could not getPixels for frame 1"); fl-image decodes it, its tests cover it, and the app's WebP is opaque
* The framework gives no frames before `runApp` (`WidgetsBinding.framesEnabled`), so an animation is loaded after the first frame in any engine
* A frame `main` asked for before the loop started was dropped: fl-ui's frame scheduler was set by the loop. It is set with the isolate's configuration now

## Stage 7: lanes (2026-10-31)

Done: churn and sanitizer lanes run the M3 programs; fuzz targets for the decoders and font loading; every limit in §15 crossed by a test (exit criterion 7, but for one below).

    cargo test --release -p fl-dart-cli --test soak -- --ignored
    FL_FUZZ_ITERS=20000 cargo test -p fl-image --test fuzz -- --ignored
    FL_FUZZ_ITERS=5000 cargo test -p fl-text --test fuzz -- --ignored

* Soak (`app_cycles_stay_flat`): text 300 cycles, counter 100, gif 20 of 16 frames, `text_churn` 100 of 100 frames (10,000 frames of text never the same), beside the M2 apps and images (2,700 decodes). RSS, descriptors, handles and GPU images held as before; the atlas within 4096, the placements beside it within twice its glyphs and 1,024; static text writes the atlas once a cycle, never on the frame after
* `app-cycle` reports the atlas: `atlas_side`, `atlas_glyphs`, `atlas_placements`, `atlas_uploads`
* `apps/text_churn`: each frame new words at a size unused for 64 frames, normal and italic, through the ticker
* valgrind app cycles add text and gif; the sanitizer app runs add text, counter, gif and text_churn
* Fuzz targets, pure Rust, no libFuzzer (no C++): fixtures and the apps' animations mutated (flips, extremes in size fields, cuts, splices, repeats) through every decode entry point; Roboto and MaterialIcons mutated through `loadFontFromList`'s path, a paragraph laid out, queried and painted. Deterministic. Short runs in ci, 20,000 inputs each under ASan. 200,000 decoder inputs, 2,000 fonts: no panic in fl-image or past fl-text
* ci now runs fl-image's and fl-assets' own tests, which no lane ran

Limits (§15) and the tests that cross them:

* pixel limit, source and target: `a_header_over_the_pixel_limit_is_refused`; images app `bomb`
* allocation refused: `a_failed_reservation_is_out_of_memory`
* PNG header reads, 16 MiB: `a_png_header_past_its_byte_limit_is_refused`
* frame count: `an_animation_over_the_frame_limit_is_refused`; each frame's size: `a_frame_over_the_pixel_limit_is_refused`; 100,000 frames: `a_hundred_thousand_tiny_frames_cost_one_frame`
* decoder panics: `the_worker_answers_every_job_and_survives_a_bad_one`, the decoder fuzz target; font parsing and shaping panics: the font fuzz target
* text length, 32 KiB: `long_text_splits_after_a_space`, `text_with_no_space_splits_on_a_character`; shaped-run cache, 4,096: `the_shaped_run_cache_is_bounded`
* glyph size, 256 px: `a_glyph_past_the_atlas_is_its_outline`; atlas side, 4096: `a_frame_past_the_atlas_limit_draws_the_rest_as_outlines`; placements: the soak
* asset keys, file kinds, 256 MiB: `keys_cannot_leave_the_bundle`, `only_regular_files_within_the_limit`; assets app
* every request answered, results for a dead isolate dropped: `retired_jobs_are_dropped_unstarted`, the soak's handle count

Not yet:

* harfrust's operation limits: set on every shape from the text's length, as HarfBuzz's, and not switchable, so on; no font here crosses them. A crafted looping GSUB font would
* The soak's time budget for changing text: frames and memory are held, time is not

Found on the way:

* Glyph placements were never pruned as the atlas evicted: text that kept changing grew them without bound. Pruned once they outnumber the atlas's glyphs twice over
* `app-cycle` kept one font collection for every cycle, so fonts an app loaded piled up, about 2 MB a cycle for text; each lifecycle now sets its bundle and fonts afresh, as an engine's
* The shaped-run cache outlived the faces it was keyed by, until its bound cleared it; it is cleared with the font collection
