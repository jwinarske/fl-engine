//! Flutter framework apps through dart:ui (brief M2 §8), via the real binary.
//!
//! Each app under `apps/` is built with the pinned Flutter release's own
//! compilers (`tools/flutter-app/build.sh`), so these need `FLUTTER_ROOT`
//! and are `#[ignore]`d; the app lane in ci.yml runs them:
//!
//!     FLUTTER_ROOT=/path/to/flutter cargo test -p fl-dart-cli --test app -- --ignored

#![cfg(feature = "aot")]
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Builds `apps/<name>` once per test process: tests run in parallel, and
/// two builds of one app would write the same file at once.
fn build_app(name: &str) -> PathBuf {
    static BUILT: Mutex<Vec<(String, PathBuf)>> = Mutex::new(Vec::new());
    let mut built = BUILT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, so)) = built.iter().find(|(n, _)| n == name) {
        return so.clone();
    }
    let so = build_app_now(name);
    built.push((name.to_owned(), so.clone()));
    so
}

fn build_app_now(name: &str) -> PathBuf {
    let flutter = std::env::var_os("FLUTTER_ROOT")
        .expect("FLUTTER_ROOT must name the pinned Flutter release");
    let repo = repo();
    let out = common::out_dir().join("apps");
    let st = Command::new(repo.join("tools/flutter-app/build.sh"))
        .arg(&flutter)
        .arg(repo.join("apps").join(name))
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        st.status.success(),
        "building {name}: {}",
        String::from_utf8_lossy(&st.stderr)
    );
    out.join(format!("{name}.so"))
}

#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn colored_box_renders_its_first_frame() {
    let app = build_app("colored_box");
    let o = common::fl_dart(&["run-app".as_ref(), app.as_os_str(), "800x600@2".as_ref()]);
    assert!(o.status.success(), "{o:?}");
    // The warm-up frame: the root transform layer (the DPR scale), one
    // picture, one drawRect (docs/m2-dart-ui.md §3).
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "frame 1: view 0 800x600 layers=1 pictures=1 draws=1\n"
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
    ]);
    assert!(o.status.success(), "{o:?}");
    assert_eq!(String::from_utf8_lossy(&o.stdout), SHAPES_OPS);
}

/// Against the stock engine's frame, within the tolerance recorded beside
/// the golden (docs/m2-dart-ui.md §9). A frame that needs more fails.
#[test]
#[ignore = "app lane: needs FLUTTER_ROOT and a Vulkan driver"]
fn shapes_matches_its_golden() {
    let app = build_app("shapes");
    let png = common::out_dir().join("apps/shapes.png");
    let _ = std::fs::remove_file(&png);
    let o = common::fl_dart(&[
        "run-app".as_ref(),
        app.as_os_str(),
        "800x600@2".as_ref(),
        "--png".as_ref(),
        png.as_os_str(),
    ]);
    assert!(o.status.success(), "{o:?}");
    let goldens = repo().join("apps/shapes/test/goldens");
    let ours = read_png(&png, (800, 600));
    let theirs = read_png(&goldens.join("shapes.png"), (800, 600));
    let tolerance = std::fs::read_to_string(goldens.join("shapes.tolerance")).unwrap();
    let limit = |key: &str| -> f64 {
        tolerance
            .lines()
            .filter(|l| !l.starts_with('#'))
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(' ')?.trim().parse().ok())
            .unwrap_or_else(|| panic!("shapes.tolerance: no {key}"))
    };
    let (mut differ, mut over_32) = (0usize, 0usize);
    for (a, b) in ours.chunks_exact(4).zip(theirs.chunks_exact(4)) {
        let d = a
            .iter()
            .zip(b)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        differ += usize::from(d > 0);
        over_32 += usize::from(d > 32);
    }
    let pct = |n: usize| 100.0 * n as f64 / (800.0 * 600.0);
    let report = format!(
        "{:.3}% differ (limit {}), {:.3}% by more than 32 (limit {}); frame at {}",
        pct(differ),
        limit("differ"),
        pct(over_32),
        limit("differ_over_32"),
        png.display()
    );
    assert!(pct(differ) <= limit("differ"), "{report}");
    assert!(pct(over_32) <= limit("differ_over_32"), "{report}");
    println!("shapes: {report}");
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
