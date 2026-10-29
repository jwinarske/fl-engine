//! Flutter framework apps through dart:ui (brief M2 §8), via the real binary.
//!
//! Each app under `apps/` is built with the pinned Flutter release's own
//! compilers (`tools/flutter-app/build.sh`): AOT as `libapp.so`, JIT as
//! kernel. So these need `FLUTTER_ROOT` and are `#[ignore]`d; the app lane
//! in ci.yml runs them in both modes:
//!
//!     FLUTTER_ROOT=/path/to/flutter cargo test -p fl-dart-cli --test app -- --ignored

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};

use common::{build_app, repo};

#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn colored_box_renders_its_first_frame() {
    let app = build_app("colored_box");
    let o = common::fl_dart(&["run-app".as_ref(), app.as_os_str(), "800x600@2".as_ref()]);
    assert!(o.status.success(), "{o:?}");
    // The warm-up frame: the root transform layer (the DPR scale), one
    // picture, one drawRect (docs/m2-dart-ui.md §3). Then the one frame the
    // framework schedules while attaching the root widget, at the first
    // vsync, the same; then nothing is asked for and the run ends. One
    // vsync frame, so no `vsync:` line.
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "frame 1: view 0 800x600 layers=1 pictures=1 draws=1\n\
         frame 2: view 0 800x600 layers=1 pictures=1 draws=1\n"
    );
}

/// The same frame drawn by emblema offscreen (Vulkan; lavapipe in CI). A
/// flat fill has no edge to anti-alias, so every pixel is exactly the color.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT and a Vulkan driver"]
fn colored_box_draws_its_color_in_every_pixel() {
    let app = build_app("colored_box");
    let png = common::out_dir().join("apps/colored_box.png");
    let _ = std::fs::remove_file(&png);
    let o = common::fl_dart(&[
        "run-app".as_ref(),
        app.as_os_str(),
        "800x600@2".as_ref(),
        "--png".as_ref(),
        png.as_os_str(),
    ]);
    assert!(o.status.success(), "{o:?}");
    let pixels = read_png(&png, (800, 600));
    let wrong = pixels
        .chunks_exact(4)
        .filter(|p| *p != [0x21, 0x96, 0xF3, 0xFF])
        .count();
    assert_eq!(wrong, 0, "pixels other than Color(0xFF2196F3)");
}

/// Writes RGBA8 `pixels` of `size` as a PNG.
fn write_png(path: &Path, size: (u32, u32), pixels: &[u8]) {
    let file = std::fs::File::create(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), size.0, size.1);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().unwrap();
    w.write_image_data(pixels).unwrap();
}

/// The RGBA8 pixels of a PNG of `size`.
fn read_png(path: &Path, size: (u32, u32)) -> Vec<u8> {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), size, "{}", path.display());
    assert_eq!(info.color_type, png::ColorType::Rgba, "{}", path.display());
    assert_eq!(info.bit_depth, png::BitDepth::Eight, "{}", path.display());
    buf.truncate(info.buffer_size());
    buf
}

/// The structure of shapes' first frame, as `run-app --ops` prints it: one
/// cell per drawing path (apps/shapes/lib/shapes.dart). Canvas transforms
/// and clips are in the picture; the clip over an opacity is two layers,
/// because an opacity layer composites, and the line's cell its own picture
/// after them.
const SHAPES_OPS: &str = "\
frame 1: view 0 800x600 layers=3 pictures=3 draws=22
  PushTransform
    AddPicture
      Rect
      Rect
      RRect
      Save
      Translate
      Path
      Restore
      Circle
      Save
      ClipRRect
      Rect
      Restore
      Rect
      Save
      Transform
      Rect
      Restore
    PushClipRect
      PushOpacity
        AddPicture
          Rect
      Pop
    Pop
    AddPicture
      Save
      Translate
      Line
      Restore
  Pop
";

#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn shapes_records_the_expected_scene() {
    let app = build_app("shapes");
    let o = common::fl_dart(&[
        "run-app".as_ref(),
        app.as_os_str(),
        "800x600@2".as_ref(),
        "--ops".as_ref(),
        "--frames".as_ref(),
        "1".as_ref(),
    ]);
    assert!(o.status.success(), "{o:?}");
    assert_eq!(String::from_utf8_lossy(&o.stdout), SHAPES_OPS);
}

/// Against the stock engine's frame, within the tolerance recorded beside
/// the golden (docs/m2-dart-ui.md §9). A frame that needs more fails.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT and a Vulkan driver"]
fn shapes_matches_its_golden() {
    matches_golden("shapes");
}

/// MaterialApp's first frame, after flutter/restoration is answered: the
/// app has a restoration scope, so without the answer it renders nothing.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT and a Vulkan driver"]
fn material_matches_its_golden() {
    matches_golden("material");
}

/// The asset bundle (M3 stage 1): `flutter/assets` and each
/// `ImmutableBuffer` source reach it, and nothing outside it: a key with
/// `..` is a miss, a missing file and a device are refused. `FontLoader`
/// completes.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn assets_reach_the_bundle_and_nothing_else() {
    let app = build_app("assets");
    let o = common::fl_dart(&["run-app".as_ref(), app.as_os_str(), "200x100".as_ref()]);
    assert!(o.status.success(), "{o:?}");
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "\
load: hello from the bundle
fromAsset: 22 bytes
fromUint8List: 3 bytes
load assets/none.txt: not found
fromAsset assets/none.txt: not found
load ../escape.txt: not found
fromAsset ../escape.txt: not found
fromFilePath /nonexistent/file: refused
fromFilePath /dev/zero: refused
FontLoader: done
frame 1: view 0 200x100 layers=1 pictures=1 draws=1
frame 2: view 0 200x100 layers=1 pictures=1 draws=1
"
    );
}

/// M3 stage 1's bar: `flutter create`'s counter app, unmodified, renders
/// with no Dart error, its text drawn as nothing (the text natives are
/// stubs) and its button's shadow not drawn. The scene: the background and
/// the app bar, the body's clip, and the button's shape.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn counter_renders_without_a_dart_error() {
    let app = build_app("counter");
    let o = common::fl_dart(&[
        "run-app".as_ref(),
        app.as_os_str(),
        "800x600@2".as_ref(),
        "--ops".as_ref(),
        "--frames".as_ref(),
        "1".as_ref(),
    ]);
    assert!(o.status.success(), "{o:?}");
    let (out, err) = (
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr),
    );
    for bad in ["Exception", "Couldn't resolve", "Error"] {
        assert!(
            !out.contains(bad) && !err.contains(bad),
            "{bad}:\n{out}\n{err}"
        );
    }
    // Not a `\` continuation: that would drop the first line's indent.
    let want = "frame 1: view 0 800x600 layers=3 pictures=3 draws=12
  PushTransform
    PushOffset
      PushOffset
        AddPicture
          RRect
        AddPicture
          RRect
          Save
          ClipRect
          Restore
        AddPicture
          Save
          Transform
          Path
          Save
          Translate
          Restore
          Restore
      Pop
    Pop
  Pop
";
    assert_eq!(out, want);
}

/// Exit criterion 1: each image source and draw, against the golden.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT and a Vulkan driver"]
fn images_matches_its_golden() {
    matches_golden("images");
}

/// What the images app prints: each source decoded to its size, a resize
/// at decode, raw pixels, and the refusals of brief M3 §15 with their
/// reasons: data no decoder reads, a header over the pixel limit, a file
/// cut short. Then the rest of the image natives, toByteData refused.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn images_decode_and_refuse_what_they_should() {
    let app = build_app("images");
    let o = common::fl_dart(&["run-app".as_ref(), app.as_os_str(), "800x600@2".as_ref()]);
    assert!(o.status.success(), "{o:?}");
    let out = String::from_utf8_lossy(&o.stdout);
    let (log, frames) = out.split_once("frame 1:").unwrap_or((&out, ""));
    assert_eq!(
        log,
        "\
checker.png: 8x8 frames=1
photo.jpg: 64x64 frames=1
nine.png: 24x24 frames=1
atlas.png: 32x16 frames=1
quads.gif: 16x16 frames=1
stripes.webp: 16x16 frames=1
photo.jpg@16: 16x16 frames=1
raw: 4x4
garbage: Exception: Invalid image data
bomb: Exception: a 65535x65535 image is over the 67108864-pixel limit
truncated: Exception: Invalid image data: png: unexpected end of file
descriptor: 8x8 bpp=4 repeat=0 ColorSpace.sRGB
toByteData: Exception: Image.toByteData is not supported by fl-engine yet
badge.png: cached
"
    );
    // The asset image is in the first frame; the second is the warm-up's
    // follow-up, and there is no third.
    assert!(
        frames.contains("frame 2:") && !frames.contains("frame 3:"),
        "{out}"
    );
}

/// The natives no other app reaches, one cell each, against their golden.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT and a Vulkan driver"]
fn natives_matches_its_golden() {
    matches_golden("natives");
}

/// Natives no app reaches yet, and the M3 stage whose programs will.
const NOT_YET: &[(&str, &str)] = &[
    ("ParagraphBuilder::addPlaceholder", "M3 stage 4, text"),
    ("Paragraph::width", "M3 stage 4, metrics"),
    ("Paragraph::longestLine", "M3 stage 4, metrics"),
    ("Paragraph::minIntrinsicWidth", "M3 stage 4, metrics"),
    ("Paragraph::alphabeticBaseline", "M3 stage 4, metrics"),
    ("Paragraph::ideographicBaseline", "M3 stage 4, metrics"),
    ("Paragraph::getRectsForRange", "M3 stage 4, metrics"),
    ("Paragraph::getPositionForOffset", "M3 stage 4, metrics"),
    ("Paragraph::getGlyphInfoAt", "M3 stage 4, metrics"),
    ("Paragraph::getClosestGlyphInfo", "M3 stage 4, metrics"),
    ("Paragraph::getWordBoundary", "M3 stage 4, metrics"),
    ("Paragraph::getLineBoundary", "M3 stage 4, metrics"),
    ("Paragraph::computeLineMetrics", "M3 stage 4, metrics"),
    ("Paragraph::getLineMetricsAt", "M3 stage 4, metrics"),
    ("Paragraph::getNumberOfLines", "M3 stage 4, metrics"),
    ("Paragraph::getLineNumberAt", "M3 stage 4, metrics"),
    ("Paragraph::dispose", "M3 stage 4, text"),
];

/// Exit criterion 5: every native fl-ui implements is called by one of the
/// apps here. `--natives` lists what a run resolved, and the VM resolves a
/// native the first time Dart calls it. A native added to the table that no
/// app reaches fails this until an app does.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn every_native_is_called_by_an_app() {
    let runs: [(&str, &[&str]); 9] = [
        ("colored_box", &[]),
        ("shapes", &["--frames", "1"]),
        ("material", &[]),
        ("natives", &["--frames", "1"]),
        ("frame_order", &[]),
        ("ticker", &["--frames", "5"]),
        ("assets", &[]),
        ("counter", &["--frames", "1"]),
        ("images", &[]),
    ];
    let mut called = std::collections::BTreeSet::new();
    for (name, opts) in runs {
        let app = build_app(name);
        let mut args: Vec<&std::ffi::OsStr> = vec!["run-app".as_ref(), app.as_os_str()];
        args.extend(opts.iter().map(std::ffi::OsStr::new));
        args.push("--natives".as_ref());
        let o = common::fl_dart(&args);
        assert!(o.status.success(), "{name}: {o:?}");
        let out = String::from_utf8_lossy(&o.stdout);
        let line = out
            .lines()
            .find_map(|l| l.strip_prefix("natives: "))
            .unwrap_or_else(|| panic!("{name}: no natives line"));
        called.extend(line.split(',').map(str::to_owned));
    }
    let missing: Vec<&str> = fl_ui::NATIVES
        .iter()
        .map(|s| s.name)
        .filter(|n| !called.contains(*n))
        .collect();
    // Exactly the stubs no app reaches yet, each with the stage whose apps
    // will. Both ways: a stage that starts reaching one takes it off.
    let not_yet: Vec<&str> = NOT_YET.iter().map(|(n, _)| *n).collect();
    assert_eq!(missing, not_yet, "natives no app calls, against NOT_YET");
}

/// Its scene: the root transform, Scaffold's two offsets, the background
/// and the card in one picture. Then the frame the framework schedules after
/// the first, the same layers reused through `addRetained`.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn material_records_the_expected_scene() {
    let app = build_app("material");
    let o = common::fl_dart(&[
        "run-app".as_ref(),
        app.as_os_str(),
        "800x600@2".as_ref(),
        "--ops".as_ref(),
    ]);
    assert!(o.status.success(), "{o:?}");
    // Not a `\` continuation: that would drop the first line's indent.
    let frame = "  PushTransform
    PushOffset
      PushOffset
        AddPicture
          RRect
          Path
          Save
          Translate
          Restore
      Pop
    Pop
  Pop
";
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        format!(
            "frame 1: view 0 800x600 layers=3 pictures=1 draws=5\n{frame}\
             frame 2: view 0 800x600 layers=3 pictures=1 draws=5\n{frame}"
        )
    );
}

/// `apps/<name>`'s first frame at 800x600, DPR 2, against
/// `apps/<name>/test/goldens/<name>.png` and the tolerance beside it.
fn matches_golden(name: &str) {
    let app = build_app(name);
    let png = common::out_dir().join(format!("apps/{name}.png"));
    let _ = std::fs::remove_file(&png);
    let o = common::fl_dart(&[
        "run-app".as_ref(),
        app.as_os_str(),
        "800x600@2".as_ref(),
        "--png".as_ref(),
        png.as_os_str(),
        "--frames".as_ref(),
        "1".as_ref(),
    ]);
    assert!(o.status.success(), "{o:?}");
    let goldens = repo().join(format!("apps/{name}/test/goldens"));
    let ours = read_png(&png, (800, 600));
    let theirs = read_png(&goldens.join(format!("{name}.png")), (800, 600));
    let tolerance = std::fs::read_to_string(goldens.join(format!("{name}.tolerance"))).unwrap();
    let limit = |key: &str| -> f64 {
        tolerance
            .lines()
            .filter(|l| !l.starts_with('#'))
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(' ')?.trim().parse().ok())
            .unwrap_or_else(|| panic!("{name}.tolerance: no {key}"))
    };
    let (mut differ, mut over_32) = (0usize, 0usize);
    // Beside the frame, for the artifact: the golden faded to a tenth, and
    // in red, as bright as the difference, every pixel that differs.
    let mut diff = Vec::with_capacity(ours.len());
    for (a, b) in ours.chunks_exact(4).zip(theirs.chunks_exact(4)) {
        let d = a
            .iter()
            .zip(b)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        differ += usize::from(d > 0);
        over_32 += usize::from(d > 32);
        if d > 0 {
            let fade = 255 - d.saturating_mul(2);
            diff.extend([255, fade, fade, 255]);
        } else {
            diff.extend(b[..3].iter().map(|c| 230 + c / 10));
            diff.push(255);
        }
    }
    let diff_png = png.with_file_name(format!("{name}-diff.png"));
    write_png(&diff_png, (800, 600), &diff);
    let pct = |n: usize| 100.0 * n as f64 / (800.0 * 600.0);
    let report = format!(
        "{:.3}% differ (limit {}), {:.3}% by more than 32 (limit {}); frame at {}, diff at {}",
        pct(differ),
        limit("differ"),
        pct(over_32),
        limit("differ_over_32"),
        png.display(),
        diff_png.display()
    );
    assert!(pct(differ) <= limit("differ"), "{name}: {report}");
    assert!(pct(over_32) <= limit("differ_over_32"), "{name}: {report}");
    println!("{name}: {report}");
}

/// The engine's frame sequence, from dart:ui with no framework: each frame
/// asked for by `scheduleFrame`, begun at vsync, its begin microtasks run
/// before `onDrawFrame`, its draw microtasks before the next vsync
/// (`Animator::BeginFrame`, `Window::BeginFrame`).
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn frame_order_follows_the_engines_sequence() {
    let app = build_app("frame_order");
    let o = common::fl_dart(&["run-app".as_ref(), app.as_os_str(), "200x100".as_ref()]);
    assert!(o.status.success(), "{o:?}");
    let out = String::from_utf8_lossy(&o.stdout);
    let mut want = String::new();
    for n in 1..=3 {
        want += &format!(
            "begin {n}\nmicrotask after begin {n}\ndraw {n}\n\
             frame {n}: view 0 200x100 layers=0 pictures=0 draws=0\n\
             microtask after draw {n}\n"
        );
    }
    let (frames, vsync) = out.rsplit_once("vsync: ").unwrap_or((&out, ""));
    assert_eq!(frames, want);
    let gaps = intervals(vsync, 3);
    // Each frame asked for during the one before: the very next tick.
    for g in gaps {
        assert!(periods(g) == 1, "{vsync}");
    }
}

/// Frames after the first (exit criterion 3): the ticker asks for each,
/// each is begun at a vsync tick, and the animation runs on those ticks'
/// times. 30 frames: the warm-up frame, then 29 at vsync.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT and a Vulkan driver"]
fn ticker_animates_at_vsync() {
    let app = build_app("ticker");
    let png = common::out_dir().join("apps/ticker.png");
    let _ = std::fs::remove_file(&png);
    let o = common::fl_dart(&[
        "run-app".as_ref(),
        app.as_os_str(),
        "200x100".as_ref(),
        "--frames".as_ref(),
        "30".as_ref(),
        "--png".as_ref(),
        png.as_os_str(),
    ]);
    assert!(o.status.success(), "{o:?}");
    let out = String::from_utf8_lossy(&o.stdout);
    let (frames, vsync) = out.rsplit_once("vsync: ").unwrap_or((&out, ""));
    let lines: Vec<&str> = frames.lines().collect();
    assert_eq!(lines.len(), 30, "{out}");
    for (i, l) in lines.iter().enumerate() {
        assert_eq!(
            *l,
            format!(
                "frame {}: view 0 200x100 layers=1 pictures=1 draws=1",
                i + 1
            )
        );
    }
    let gaps = intervals(vsync, 29);
    // Never faster than vsync. A frame late enough to miss a tick waits for
    // the next one, so the ticks it missed are gaps, not extra frames.
    let ticks: u64 = gaps.iter().map(|g| periods(*g)).sum();
    assert!(
        ticks <= 2 * gaps.len() as u64,
        "more than one tick missed per frame on average: {vsync}"
    );
    // The ticker starts on the first vsync frame, so the last frame's value
    // is the time from it, over the one-second duration.
    let elapsed_us: i64 = gaps.iter().sum();
    let t = (elapsed_us as f64 / 1e6).fract();
    let px = read_png(&png, (200, 100));
    let center = &px[(50 * 200 + 100) * 4..][..4];
    let want = [255.0 * (1.0 - t), 0.0, 255.0 * t, 255.0];
    for (got, want) in center.iter().zip(want) {
        assert!(
            (f64::from(*got) - want).abs() <= 2.0,
            "center {center:?}, want {want:?} at t={t:.4} ({vsync})"
        );
    }
}

/// One period of the default view's 60 Hz, in microseconds.
const PERIOD_US: f64 = 1e6 / 60.0;

/// The intervals of a `vsync: frames=N interval_us=a,b,…` line, checking N.
fn intervals(vsync: &str, frames: usize) -> Vec<i64> {
    let vsync = vsync.trim();
    let (n, gaps) = vsync
        .strip_prefix("frames=")
        .and_then(|r| r.split_once(" interval_us="))
        .unwrap_or_else(|| panic!("not a vsync line: {vsync:?}"));
    assert_eq!(n.parse::<usize>().unwrap(), frames, "{vsync}");
    gaps.split(',').map(|g| g.parse().unwrap()).collect()
}

/// How many vsync periods `gap_us` is. Frame times are ticks, so a gap is
/// a whole number of periods to within the clock's microsecond rounding.
fn periods(gap_us: i64) -> u64 {
    let n = (gap_us as f64 / PERIOD_US).round();
    assert!(
        n >= 1.0 && (gap_us as f64 - n * PERIOD_US).abs() <= 2.0,
        "{gap_us} us is not a whole number of vsync periods"
    );
    n as u64
}

/// Every native fl-ui implements is declared by the pinned release's
/// dart:ui with the argument count fl-ui registers it under. A wrong count
/// fails only when the framework first calls that native, which a test
/// program may never do.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn every_native_matches_its_dart_declaration() {
    let flutter = PathBuf::from(std::env::var_os("FLUTTER_ROOT").expect("FLUTTER_ROOT"));
    let ui = flutter.join("bin/cache/pkg/sky_engine/lib/ui");
    let mut declared = std::collections::HashMap::new();
    for entry in std::fs::read_dir(&ui).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "dart") {
            let src = std::fs::read_to_string(&path).unwrap();
            for (name, args) in natives_declared(&src) {
                declared.insert(name, args);
            }
        }
    }
    assert!(
        declared.len() > 100,
        "found only {} in {}",
        declared.len(),
        ui.display()
    );
    for s in fl_ui::NATIVES {
        match declared.get(s.name) {
            Some(n) => assert_eq!(*n, s.args, "{}: dart:ui declares {n} arguments", s.name),
            None => panic!("{}: not declared by this dart:ui", s.name),
        }
    }
}

/// `(symbol, parameter count)` of each `@Native<R Function(...)>(symbol: '...')`.
fn natives_declared(src: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find("@Native<") {
        rest = &rest[i + "@Native<".len()..];
        let Some(f) = rest.find("Function(") else {
            break;
        };
        let params = &rest[f + "Function(".len()..];
        let (mut depth, mut commas, mut end) = (0i32, 0, 0);
        let mut any = false;
        for (j, c) in params.char_indices() {
            match c {
                '(' | '<' => depth += 1,
                ')' if depth == 0 => {
                    end = j;
                    break;
                }
                ')' | '>' => depth -= 1,
                ',' if depth == 0 => commas += 1,
                c if !c.is_whitespace() => any = true,
                _ => {}
            }
        }
        // A trailing comma before the `)` adds no parameter.
        let trailing = params[..end].trim_end().ends_with(',');
        let n = if any {
            commas + 1 - usize::from(trailing)
        } else {
            0
        };
        let Some(sym) = params[end..].find("symbol: '") else {
            continue;
        };
        let name = &params[end + sym + "symbol: '".len()..];
        if let Some(q) = name.find('\'') {
            out.push((name[..q].to_owned(), n));
        }
    }
    out
}
