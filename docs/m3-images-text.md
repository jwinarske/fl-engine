# M3: images and text

> Brief, 2026-10-28. The decisions in §13 were taken 2026-10-29 and 30. Findings from bring-up go
> in `docs/m3-status.md`, newer where the two disagree.

**`flutter create`'s counter app, unmodified, renders its first frame on fl-engine and matches the stock engine's golden within a recorded tolerance: app bar title, body text, the count, and the floating action button with its icon and shadow. Images are decoded, text shaped and laid out, and glyphs rasterized in pure Rust; emblema draws both.**

Sources, read at the pin (Flutter 3.47.8, engine `cf636b1d`, Dart `416e33b0`): engine `lib/ui/painting` (image, codec, image_descriptor, immutable_buffer), `lib/ui/text` (paragraph, paragraph_builder, font_collection), `txt/src/skia` (SkParagraph's use); framework `services/asset_bundle.dart`, `painting/image_provider.dart`, `painting/text_painter.dart`. emblema at `085d873`: `emblema-text` (the glyph atlas), `Canvas::draw_glyphs`, `Context::create_image`/`write_image`/`draw_with_images`, `examples/frame.rs` (its own text: swash and cosmic-text).

---

## 1. What M3 Is and Is Not

**Is:**
* the asset bundle: `flutter_assets` beside the app, `flutter/assets` answered, `ImmutableBuffer.fromAsset`, `FontManifest.json` read at startup as the engine reads it (`font_collection.cc:75`)
* images: `ImmutableBuffer`, `ImageDescriptor`, `Codec` (animated GIF and WebP frames included), `Image`, `ImageShader`, and `drawImage`, `drawImageRect`, `drawImageNine`, `drawAtlas`
* text: `FontCollection.loadFontFromList`, `ParagraphBuilder`, `Paragraph` (layout, its metrics and hit-testing queries, `paint`), glyphs rasterized into emblema's atlas and drawn with `draw_glyphs`
* `drawShadow`, which every elevated Material surface draws (the counter app's button)
* decoding off the UI thread, as the engine's IO runner does it

**Is not:**
* input of any kind, text input and IME, selection painting. M4
* accessibility and semantics (`UpdateSemantics` stays a stub)
* `FragmentProgram`, platform views, external textures
* `Picture.toImage`, `Scene.toImage`, `Image.toByteData` (GPU readback to Dart), unless the counter app turns out to need one
* color glyphs (emoji): emblema's atlas holds coverage only (`emblema-text` lib.rs)
* HEIF and AVIF, which the engine takes from the platform
* KMS on the boards: M2's stage 6, when the hardware is to hand

## 2. The Contract Being Implemented

**Assets.** A Flutter Linux bundle keeps them in `data/flutter_assets`: `AssetManifest.bin`, `FontManifest.json`, `fonts/MaterialIcons-Regular.otf` (tree-shaken in release), the app's own. The framework reaches them two ways:
* `rootBundle.load(key)`: a `flutter/assets` platform message, the key as UTF-8, answered with the bytes or null (asset_bundle.dart:329)
* `loadBuffer(key)`: the `ImmutableBuffer::initFromAsset` native (asset_bundle.dart:371), which image providers use

**Fonts.** The engine, not Dart, reads `FontManifest.json` and registers every family in it before `main` (font_collection.cc). `FontLoader` adds more at run time through `loadFontFromList`. A family the style names and nobody registered falls back to the platform's fonts.

**Images.** `instantiateImageCodec` gives a `Codec`; `getNextFrame` answers through a callback, later, with an `Image` and a duration. Decoding happens on the engine's IO thread, never the UI thread. An `Image` is a handle to GPU memory the raster thread owns; `dispose` and the finalizer release it.

**Text.** `ParagraphBuilder` takes styles encoded into `Int32List` and `ByteData` (text.dart's `_encodeTextStyle`, `_encodeParagraphStyle`); `Paragraph.layout(width)` must answer metrics synchronously, on the UI thread, because the framework lays out with them in the same frame. So layout is UI-thread work and rasterizing glyphs is raster-thread work.

## 3. The Native Set

Unimplemented at the pin, counted from dart:ui's `@Native` declarations:

| Area | Natives | Count |
|---|---|---|
| Buffers | `ImmutableBuffer` init, initFromAsset, initFromFile, dispose | 4 |
| Decoding | `ImageDescriptor` (7), `Codec` (4) | 11 |
| Images | `Image` (6), `ImageShader` (3) | 9 |
| Drawing | `Canvas` drawImage, drawImageRect, drawImageNine, drawAtlas, drawShadow | 5 |
| Text | `FontCollection::LoadFontFromList`, `ParagraphBuilder` (6), `Paragraph` (22) | 29 |
| Platform | `GetScaledFontSize` | 1 |

About 59. The survey is redone against the counter app at stage 1, as M2's was against `colored_box`: anything else its first frame reaches joins the set.

## 4. Images Through emblema

* **Decoders**, one crate per format, all pure Rust: `png` (already a dependency), `zune-jpeg`, `gif`, `image-webp`. BMP, ICO and WBMP if an app needs them. Not the `image` crate: it needs Rust 1.88 (§13)
* `ImageDescriptor.encoded` reads the header only: size and format without decoding. `instantiateCodec(targetWidth, targetHeight)` decodes, and resizes when asked, on the decode thread
* An `Image` peer holds an `Arc` of decoded RGBA. The raster thread uploads it to an emblema `Image` the first time a scene draws it and keeps it by that `Arc`; when the last reference goes, the raster thread is told and destroys the GPU image. So GPU memory follows Dart's lifetimes without the UI thread touching the device
* Drawing: `Paint::image(slot, rect)` and `draw_with_images`, the frame's images as its slot table. `FilterQuality` to `Sampling`: none → Nearest, low → Linear, medium → Linear with mipmaps, high → Cubic. The two enums are not one-to-one; the table is fl-raster's
* Premultiplied or straight alpha on upload is checked against emblema at stage 2, not assumed

## 5. Text

* **Fonts**: bytes from `FontManifest.json` and `loadFontFromList`, shared as `Arc<[u8]>` across threads; system fonts through fontdb for fallback (D4)
* **Shaping and layout** (D1): runs shaped by harfrust, lines broken by UAX #14, bidi by UAX #9, laid out to SkParagraph's rules: height multipliers, `TextHeightBehavior` and leading distribution, strut, `maxLines` and ellipsis, letter and word spacing, placeholders. These rules are what the framework's `TextPainter` measures against, so they are what parity means here
* **Queries**: line metrics, boxes for a range, position for an offset, word and line boundaries (UAX #29), glyph info; all from the laid-out paragraph, on the UI thread
* **Paint**: `Paragraph::paint(canvas, x, y)` records the laid-out glyph runs into the picture: font, size, glyph ids, positions, color, decorations as lines. No raster work on the UI thread
* **Rasterizing** (D2, swash): on the raster thread, each glyph not in the atlas is rasterized at the device scale and inserted; the atlas uploads when dirty; a run is one `draw_glyphs`. emblema has no subpixel positioning, so glyph positions are rounded to the pixel; that is a recorded difference from Skia
* **Decorations**: underline, overline, strike-through as rects and paths, styles solid, double, dotted, dashed, wavy

## 6. Threads

* UI thread: Dart, `Paragraph` layout, `Image` peers
* Decode thread: one, as the engine's IO runner. Takes encoded bytes, returns decoded frames; a result goes back through the reply queue M2 built for platform messages, now fed from another thread, so the queue wakes the loop
* Raster thread: the device, the glyph atlas, the GPU image cache. The atlas and the cache are `!Send`, each with a compile-fail case, as `Offscreen` is

## 7. Crate Structure

```
crates/
  fl-assets    the bundle: manifest, flutter/assets, ImmutableBuffer sources
  fl-image     decoding: descriptor, codec, frames; the decode thread. No dart:ui
  fl-text      fonts, shaping, layout, queries, glyph runs. No dart:ui, no emblema
  fl-ui        the new natives, over the three above
  fl-raster    images and glyph runs to emblema: GPU image cache, atlas
```

fl-text and fl-image know neither the VM nor emblema, so each is testable alone against known inputs, as `emblema-text` is.

## 8. Test Programs

* `images`: PNG, JPEG, a GIF's first frame and WebP, from memory and from assets; drawImageRect with source rects, nine-slice, an `ImageShader` fill, each `FilterQuality`
* `text`: paragraphs in Roboto loaded from assets: sizes, weights, alignment, wrapping, `maxLines` with ellipsis, mixed spans, decorations, a placeholder
* `metrics`: dart:ui only. Lays out fixed paragraphs and prints their metrics: line metrics, boxes for ranges, positions for offsets, word boundaries. Its expected output comes from the same program under flutter_tester
* `gif`: an animated GIF through `Codec.getNextFrame`, frames and durations checked
* `counter`: `flutter create`'s template, unmodified

## 9. Goldens

* As M2: the stock engine's flutter_tester renders each program, with Impeller (D6); fl-engine's frame is compared with a tolerance recorded beside the golden
* Programs that draw shadows render at DPR 3: `drawShadow` reads the engine's view-0 DPR, which flutter_tester fixes at 3
* flutter_tester draws every glyph as a box (the Ahem test font) unless a test loads fonts. The text goldens load Roboto and MaterialIcons from the Flutter cache (`bin/cache/artifacts/material_fonts`) with `FontLoader`, and the apps name those families, so both engines draw the same faces
* Text pixels will differ more than shapes did: hinting, subpixel positioning, the rasterizer's coverage. The layout is held separately and tighter, by `metrics`: positions within half a pixel of the stock engine's

## 10. Test Plan

* **L0**: each decoder against images with known pixels; `ImageDescriptor` headers; TextStyle and ParagraphStyle decoding; line breaking and layout cases from SkParagraph's own tests, where they apply
* **Compile-fail**: the glyph atlas and the GPU image cache stay on the raster thread
* **L2**: each program to its first frame, AOT and JIT; `gif` frame by frame; `metrics` against its expected output
* **Goldens**: §9, lavapipe
* **L5**: 1,000 decode and dispose cycles with RSS and the GPU image cache flat; 10,000 frames of changing text with the atlas bounded; the apps under ASan and TSan, with the decode thread
* Criterion 5 of M2 carries on: every native called by an app, checked

## 11. Exit Criteria

1. `images` matches its golden; decoding never on the UI thread
2. `text` matches its golden; `metrics` matches the stock engine's numbers within half a pixel
3. `counter`, unmodified, matches its golden
4. `gif` advances through its frames at their durations
5. Every native in the M3 set is called by an app; every stub is listed
6. The atlas and the GPU image cache are thread-placed by type, each with a compile-fail case
7. Leak and sanitizer lanes green with the M3 programs; fuzz targets for the decoders and font loading run there; every limit in §15 has a test that crosses it
8. Still no C toolchain: every decoder, the shaper and the rasterizer are pure Rust; the no-C++ lane covers them

## 12. Staged Path

Each stage carries its items from §15.

1. **Assets.** `build.sh` produces `flutter_assets` beside the app; `flutter/assets` answered; `ImmutableBuffer`; `FontManifest.json` registered, parsed lazily. Asset keys confined to the bundle, regular files only, size-limited. The counter app's native survey. Done when the counter app reaches `Render` with no Dart error, text drawn as nothing
2. **Images.** Decoders behind the pixel limit and `try_reserve`, on the worker under `catch_unwind`; descriptor, codec, `Image`; replies by id, tagged by isolate; the GPU image cache, uploaded ahead of the draw; `drawImage*`, `ImageShader`. Exit criterion 1
3. **Shadows.** `drawShadow` through emblema's `draw_shadow`
4. **Layout.** ParagraphBuilder and Paragraph, metrics and queries; shaped-run and fallback caches; harfrust's limits on; system fonts read, not mapped. `metrics` against the stock engine, with every style field round-tripped
5. **Glyphs.** Rasterizing into the atlas with eviction; large glyphs as paths; one atlas upload a frame; decorations; goldens refusing fallback glyphs. Exit criterion 2
6. **counter.** Exit criterion 3
7. **Animation and lanes.** `gif` frame by frame within the limits; churn, sanitizers, fuzz targets. Exit criteria 4 to 8

## 13. Decisions (2026-10-29)

* **D1, shaping and layout: fl-text's own layout, on harfrust.** harfrust (the Rust port of HarfBuzz), skrifa (font tables), unicode-linebreak, unicode-bidi and unicode-segmentation, with lines laid out to SkParagraph's rules. More code than a paragraph engine, but the rules are Flutter's, which is what the metrics parity in §9 measures. All on Rust 1.85. Not parley or cosmic-text: both have their own models of line height and spacing, and need 1.88 or later (parley 0.12, cosmic-text 0.19), which would raise fl-engine's floor and part it from emblema's
* **D2, rasterizer: swash 0.2**, built on skrifa: scaling, hinting and coverage in one, as emblema's example uses it. It declares no `rust-version`; checked on 1.85 at stage 5
* **D3, decoders: one crate per format.** `png` (1.73), `zune-jpeg` (1.77), `gif` (1.62), `image-webp` (1.80). Not the `image` crate, which needs 1.88
* **D4, fallback fonts: the system's, through fontdb** (1.71), scanning the directories fontconfig would, after the manifest's fonts and the app's. As Flutter on Linux behaves. Goldens still load Roboto by name, so they do not depend on what a runner has installed
* **D5, decoding: one worker thread**, as the engine's IO runner. Results come back through the reply queue, which then wakes the UI loop from another thread; the TSan lane covers that crossing
* **D6, parity target: Impeller** (2026-10-30). Goldens are rendered by flutter_tester with `--enable-impeller`, and fl-engine matches Impeller's behavior where it and Skia differ: shadows, sampling. Skia is being deprecated

## 14. Risks

* **Text parity.** HarfBuzz, ICU and SkParagraph against a Rust stack: line breaks at the edges of UAX #14, shaping of complex scripts, the rounding of glyph positions. Mitigation: `metrics` holds the layout numerically; goldens hold the pixels with a recorded tolerance; scripts beyond Latin are tested only for not failing
* **Dependency floor.** The text crates moving to 1.88 is the trend. Mitigation: D1 to D4 as taken stay on 1.85; raising the floor is a decision taken with emblema, not inside M3
* **Surface.** `Paragraph` is 22 natives with fine semantics. Mitigation: the counter app first, then the rest as the `text` and `metrics` programs reach them; each stubbed native is listed with what it returns
* **GPU memory on boards.** Decoded images at full size. Mitigation: `targetWidth`/`targetHeight` honored at decode; the cache's size reported in the soak lane
* **Icon fonts.** Release builds tree-shake MaterialIcons to the glyphs used; `flutter build bundle` does not (1.6 MB, m3-status). Mitigation: `build.sh` runs const_finder and font-subset as `flutter build linux --release` does, so the counter app gets the font it would ship with

## 15. Reliability, Security and Performance (review, 2026-10-29)

M3 is the first milestone that parses input fl-engine does not control: an image from the network, a font a package downloads, an asset key built from a deep link. Everything below is a rule for the stage named, checked by a test there.

**Security: untrusted bytes**
* **Decompression bombs** (stage 2). A header can claim 65535x65535 pixels from a few hundred bytes. Dimensions are read first (`ImageDescriptor`), checked against a pixel limit (`width * height` with `checked_mul`, 64 Mpx by default, settable), and refused as a Dart error before any pixel buffer exists. Every pixel and frame buffer is allocated with `try_reserve`: an allocation that fails is an error, not the process aborting, which is what Rust does on a failed allocation
* **Animated images** (stage 7). Frames are decoded one at a time on `getNextFrame`, never all at once; frame count and per-frame size are held to the same limits. A GIF of 100,000 tiny frames costs one frame of memory
* **Decoder and font-parser panics** (stages 2, 4). Each decode job and each font load runs under `catch_unwind`; a panic is that call's error and the worker carries on. The decoders, skrifa, harfrust and swash are safe Rust, so a malformed file is a panic or an error, not memory corruption; a fuzz target per decoder and for font loading runs in the sanitizer lane
* **Shaping cost** (stage 4). A crafted font can make shaping expensive. harfrust's operation limits (HarfBuzz's `max_ops`) are checked to be on, and a paragraph's text length is held to a limit, past which it is shaped in pieces
* **Glyph size** (stage 5). `fontSize: 5000` would ask for a coverage bitmap of tens of megabytes. Above a size threshold a glyph is drawn as its outline, a path through emblema, as Skia does, and never rasterized into the atlas; the atlas's `TooLarge` never reaches the frame
* **Asset keys** (stage 1). A key is a path inside `flutter_assets` and nothing else: no absolute paths, no `..`, no NUL; resolved, then checked to still be under the bundle after symlinks. Only regular files, read up to a size limit, so `/dev/zero` and a FIFO are refused. `ImmutableBuffer.fromFile` is the app's own choice of path, as `dart:io` already allows, but gets the same regular-file and size checks
* **System fonts** (stage 4). Read into memory, not memory-mapped: a mapped font file truncated by a package upgrade while fl-engine runs is a SIGBUS

**Reliability**
* **Replies across threads** (stage 2). The decode worker never touches a Dart handle. A job carries an id; the UI thread keeps the id's callback as a persistent handle and resolves it when the result arrives. Results are tagged with the isolate they belong to; results for an isolate that has shut down are dropped, and shutdown cancels its queued jobs. So a result can never call into a dead isolate
* **Every request answered** (stages 1, 2). Each `getNextFrame`, `flutter/assets` request and `fromAsset` completes, with data or with an error string, as the engine's do. An unanswered request is a persistent handle held until shutdown and a Future that never completes; the soak lane counts both
* **Atlas full** (stage 5). On `Full`, the atlas evicts glyphs not used this frame (`begin_frame`, compaction) and retries; a single frame that needs more glyphs than the atlas's limit holds is drawn in more than one pass. Text never silently goes missing
* **Fallback determinism** (stage 5). System fallback (D4) makes pixels depend on the runner's installed fonts. The golden programs fail if any of their glyphs came from fallback, so a golden only ever compares fonts it names
* **Format drift** (stage 4). `_encodeTextStyle` and `_encodeParagraphStyle` are bit layouts read at one Flutter release. `metrics` builds styles with every field set and checks fl-text decoded each one, so a pin bump that moves a field fails there, as the native-count check does for natives
* **harfrust's age** (stage 4). A young port. rustybuzz is the fallback; fl-text keeps shaping behind one function, so changing shapers is one file

**Performance**
* **UI-thread layout** (stage 4). The framework lays text out every frame it changes. Shaped runs are cached by font, size, features and text; fallback lookups by code point. `metrics` reports layout time per paragraph, and the soak lane holds 10,000 frames of changing text to a time budget
* **Startup** (stage 1). Manifest fonts are registered by name and parsed on first use, not read in full before `main`. The system-font scan happens on the first fallback miss, not at startup, and only once
* **Atlas uploads** (stage 5). emblema writes an image whole (`write_image`), so a dirty 2048x2048 atlas, one byte of coverage a texel, is 4 MB a frame. fl-raster batches a frame's new glyphs into one upload; a region write in emblema is asked for upstream if the soak lane shows uploads on steady-state frames
* **Image uploads** (stage 2). A large image uploaded on the frame that first draws it stalls that frame. The raster thread uploads when a decode completes, ahead of the draw; the CPU copy is released once uploaded, so a decoded image is not held twice. Flutter's own `ImageCache` (100 MB, 1,000 images by default) bounds what the framework keeps alive; the GPU cache reports its size in the soak lane
* **Decode throughput** (stage 2). One worker (D5), measured with `images`; more workers only if a measured case needs them
* **Boards** (stage 6 of M2, then here). First numbers for text frames on Pi 5 and VisionFive 2 when the hardware is to hand; emblema's UI-like frame is the budget (5.19 ms on Pi 5)
