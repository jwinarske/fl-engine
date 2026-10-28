# M3: images and text

> Brief, 2026-10-28. The decisions in §12 are open. Findings from bring-up go
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

* **Fonts**: bytes from `FontManifest.json` and `loadFontFromList`, shared as `Arc<[u8]>` across threads; system fonts for fallback (D4)
* **Shaping and layout** (D1): runs shaped, lines broken by UAX #14, bidi by UAX #9, laid out to SkParagraph's rules: height multipliers, `TextHeightBehavior` and leading distribution, strut, `maxLines` and ellipsis, letter and word spacing, placeholders. These rules are what the framework's `TextPainter` measures against, so they are what parity means here
* **Queries**: line metrics, boxes for a range, position for an offset, word and line boundaries (UAX #29), glyph info; all from the laid-out paragraph, on the UI thread
* **Paint**: `Paragraph::paint(canvas, x, y)` records the laid-out glyph runs into the picture: font, size, glyph ids, positions, color, decorations as lines. No raster work on the UI thread
* **Rasterizing** (D2): on the raster thread, each glyph not in the atlas is rasterized at the device scale and inserted; the atlas uploads when dirty; a run is one `draw_glyphs`. emblema has no subpixel positioning, so glyph positions are rounded to the pixel; that is a recorded difference from Skia
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

* As M2: the stock engine's flutter_tester renders each program; fl-engine's frame is compared with a tolerance recorded beside the golden
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
7. Leak and sanitizer lanes green with the M3 programs
8. Still no C toolchain: every decoder, the shaper and the rasterizer are pure Rust; the no-C++ lane covers them

## 12. Staged Path

1. **Assets.** `build.sh` produces `flutter_assets` beside the app; `flutter/assets` answered; `ImmutableBuffer`; `FontManifest.json` read. The counter app's native survey. Done when the counter app reaches `Render` with no Dart error, text drawn as nothing
2. **Images.** Decoders, descriptor, codec, `Image`, the GPU image cache, `drawImage*`, `ImageShader`. Exit criterion 1
3. **Shadows.** `drawShadow` through emblema's `draw_shadow`
4. **Layout.** ParagraphBuilder and Paragraph, metrics and queries. `metrics` against the stock engine
5. **Glyphs.** Rasterizing into the atlas, `draw_glyphs`, decorations. Exit criterion 2
6. **counter.** Exit criterion 3
7. **Animation and lanes.** `gif`, churn, sanitizers. Exit criteria 4 to 8

## 13. Decisions (open)

* **D1, shaping and layout.** Two ways that fit the Rust 1.85 floor, and one that does not:
  * harfrust (shaping, the Rust port of HarfBuzz, 1.85), skrifa (font tables, 1.85), unicode-linebreak, unicode-bidi, unicode-segmentation, and a layout of fl-text's own written to SkParagraph's rules. More code, but the rules are Flutter's, which is what the metrics parity in §9 measures. **Recommended**
  * parley (Linebender's layout) or cosmic-text: whole paragraph engines with their own models of line height and spacing. Both need Rust 1.88 or later (parley 0.12, cosmic-text 0.19), so either raises fl-engine's floor and parts with emblema's
* **D2, rasterizer.** swash 0.2 (built on skrifa; scaling, hinting, coverage in one), or skrifa outlines through zeno. emblema's example uses swash. **Recommended: swash**, checked on 1.85 at stage 5, since it declares no `rust-version`
* **D3, decoders.** One crate per format, all on 1.85 (`png` 1.73, `zune-jpeg` 1.77, `gif` 1.62, `image-webp` 1.80). **Recommended.** The `image` crate would be one dependency, but needs 1.88
* **D4, fallback fonts.** fontdb (1.71) scanning the system font directories, as fontconfig would find them, after the manifest's and the app's. **Recommended.** Or none: an app that names no bundled family draws nothing, which is honest but unlike Flutter on Linux
* **D5, decode thread.** One worker, as the engine's IO runner. **Recommended.** Decoding on the UI thread would be simpler and would make large images stall frames

## 14. Risks

* **Text parity.** HarfBuzz, ICU and SkParagraph against a Rust stack: line breaks at the edges of UAX #14, shaping of complex scripts, the rounding of glyph positions. Mitigation: `metrics` holds the layout numerically; goldens hold the pixels with a recorded tolerance; scripts beyond Latin are tested only for not failing
* **Dependency floor.** The text crates moving to 1.88 is the trend. Mitigation: D1 and D3 as recommended stay on 1.85; raising the floor is a decision taken with emblema, not inside M3
* **Surface.** `Paragraph` is 22 natives with fine semantics. Mitigation: the counter app first, then the rest as the `text` and `metrics` programs reach them; each stubbed native is listed with what it returns
* **GPU memory on boards.** Decoded images at full size. Mitigation: `targetWidth`/`targetHeight` honored at decode; the cache's size reported in the soak lane
* **Icon fonts.** Release builds tree-shake MaterialIcons to the glyphs used. Mitigation: build the bundle as `flutter build linux --release` does, so the counter app gets the font it would ship with
