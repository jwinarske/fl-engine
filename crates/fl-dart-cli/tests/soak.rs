//! L5: leaks and soak, through the real binary (brief §11).
//!
//! Slow, so each test is `#[ignore]`d with the command that runs it; a plain
//! `cargo test` lists them as ignored rather than leaving them out. The soak
//! lane runs them in a release build:
//!
//!     cargo test --release -p fl-dart-cli --test soak -- --ignored
//!
//! Same environment as the L2 suite (`FL_DART_SDK_BIN`, and for cross lanes
//! `FL_DART_TEST_RUNNER`). The valgrind test needs `valgrind` on PATH and
//! fails without it.

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{compile, fl_dart, MODE};
use std::collections::HashMap;

/// The programs every soak runs: the empty case, the event loop with a timer,
/// both FFI crossings, and many threads posting at once.
const PROGRAMS: [&str; 4] = ["hello", "loop", "ffi", "burst"];

/// Growth allowed from a tenth of the way through a run to its end. Over 900
/// cycles this catches a leak of roughly 600 bytes per cycle; measured growth
/// over that span is 0-20 kB, AOT and JIT.
///
/// JIT once grew about 130 bytes per cycle: the VM's default ring timeline
/// recorder kept every thread's metadata. fl-dart-vm turns the recorder off
/// (docs/m2-status.md); this limit is what would catch it coming back.
const RSS_GROWTH_LIMIT_KB: u64 = 512;

fn cycle_report(prog: &std::path::Path, n: u32) -> HashMap<String, u64> {
    report(&["cycle".as_ref(), prog.as_os_str(), n.to_string().as_ref()])
}

/// The `key=value` line a leak-harness command prints, from `args`.
fn report(args: &[&std::ffi::OsStr]) -> HashMap<String, u64> {
    // One malloc arena. With glibc's default, burst's threads land in new
    // arenas now and then and RSS steps up 0.5-1.5 MB at random: 3,000
    // cycles measured flat, 6,000 grew 1.5 MB. With one arena 6,000 cycles
    // grow 40 kB. A leak still grows linearly here; valgrind checks the rest.
    //
    // And a fixed mmap threshold. glibc raises it each time a large mmapped
    // block is freed, so the images app's textures, allocated and freed by
    // lavapipe each cycle, move from mmap to the heap and back, and RSS
    // wanders about 1 MB: 300 cycles measured +876 kB, 2,700 cycles ended
    // 696 kB below their tenth. Fixed at 128 kB, glibc's starting value,
    // 300 and 900 cycles grow 0 to 364 kB.
    let o = common::fl_dart_cmd()
        .env("MALLOC_ARENA_MAX", "1")
        .env("MALLOC_MMAP_THRESHOLD_", "131072")
        .env("MALLOC_TRIM_THRESHOLD_", "131072")
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    let line = String::from_utf8_lossy(&o.stdout).into_owned();
    line.split_whitespace()
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.parse().unwrap()))
        .collect()
}

#[test]
#[ignore = "soak lane: cargo test --release -p fl-dart-cli --test soak -- --ignored"]
fn isolate_cycles_stay_flat() {
    for name in PROGRAMS {
        let prog = compile(name, MODE.0);
        let r = cycle_report(&prog, 1000);
        let growth = r["rss_end_kb"].saturating_sub(r["rss_tenth_kb"]);
        assert!(
            growth <= RSS_GROWTH_LIMIT_KB,
            "{name}: RSS grew {growth} kB over the last 900 cycles: {r:?}"
        );
        assert_eq!(
            r["fds_end"], r["fds_warm"],
            "{name}: descriptors leaked: {r:?}"
        );
        assert_eq!(r["persistent_handles"], 0, "{name}: {r:?}");
        println!("{name}: {r:?}");
    }
}

/// `--font` options for Roboto and MaterialIcons from the Flutter release's
/// cache, as the counter golden is given them.
fn material_fonts() -> [String; 4] {
    let root = std::env::var("FLUTTER_ROOT").expect("FLUTTER_ROOT names the Flutter release");
    let fonts = format!("{root}/bin/cache/artifacts/material_fonts");
    [
        "--font".into(),
        format!("Roboto={fonts}/Roboto-Regular.ttf"),
        "--font".into(),
        format!("MaterialIcons={fonts}/MaterialIcons-Regular.otf"),
    ]
}

/// One app's `app-cycle` run in the soak: cycles, frames each renders, its
/// options; with `--png`, drawn on the raster thread.
#[cfg(feature = "aot")]
struct Soak {
    name: &'static str,
    cycles: u32,
    per_cycle: u64,
    opts: Vec<String>,
}

#[cfg(feature = "aot")]
fn soak(name: &'static str, cycles: u32, per_cycle: u64, opts: &[&str], draw: bool) -> Soak {
    let mut opts: Vec<String> = opts.iter().map(|o| (*o).to_owned()).collect();
    if draw {
        let png = common::out_dir().join(format!("apps/{name}-soak.png"));
        opts.extend(["--png".to_owned(), png.display().to_string()]);
    }
    Soak {
        name,
        cycles,
        per_cycle,
        opts,
    }
}

/// App lifecycles (brief M2 §10, M3 §10, L5): each app started and stopped
/// in one VM, `app-cycle`, with RSS, descriptors and handles held to the
/// same bounds as isolate cycles, through one raster thread whose GPU
/// image cache and glyph atlas outlive the cycles.
///
/// * colored_box, material and shapes run to their own end, the UI side
///   only: the warm-up frame and the one scheduled after it
/// * ticker: 100 frames a cycle at 1 kHz, 10,000 in all, picture and
///   scene churn on both sides of the raster thread
/// * images decodes and uploads its nine images each cycle, 2,700 decodes
///   and disposes, and draws them twice
/// * text and counter: static text, its glyphs written to the device once
///   a cycle (each cycle's fonts are new), none on the frame after
/// * gif: 16 frames a cycle of four animations decoded frame by frame
/// * text_churn: 100 frames a cycle of text never the same, 10,000 in all;
///   the atlas evicts, stays within its limit, and so do the placements
///   kept beside it
#[cfg(feature = "aot")]
#[test]
#[ignore = "soak lane: needs FLUTTER_ROOT"]
fn app_cycles_stay_flat() {
    let fonts = material_fonts();
    let fonts: Vec<&str> = fonts.iter().map(String::as_str).collect();
    let with_fonts = |base: &[&'static str]| -> Vec<&str> {
        base.iter().copied().chain(fonts.iter().copied()).collect()
    };
    let runs = [
        soak("colored_box", 1000, 2, &[], false),
        soak("material", 1000, 2, &[], false),
        soak("shapes", 300, 2, &[], false),
        soak(
            "ticker",
            100,
            100,
            &["200x100", "--frames", "100", "--hz", "1000"],
            true,
        ),
        soak("images", 300, 2, &["800x600@2"], true),
        soak("text", 300, 2, &["1600x1200@2"], true),
        soak("counter", 100, 2, &with_fonts(&["1200x900@3"]), true),
        soak("gif", 20, 16, &["400x300", "--frames", "16"], true),
        soak(
            "text_churn",
            100,
            100,
            &with_fonts(&["400x300", "--frames", "100", "--hz", "1000"]),
            true,
        ),
    ];
    for Soak {
        name,
        cycles: n,
        per_cycle,
        opts,
    } in runs
    {
        let app = common::build_app(name);
        let n_str = n.to_string();
        let mut args: Vec<&std::ffi::OsStr> =
            vec!["app-cycle".as_ref(), app.as_os_str(), n_str.as_ref()];
        args.extend(opts.iter().map(std::ffi::OsStr::new));
        let r = report(&args);
        assert_eq!(r["frames"], u64::from(n) * per_cycle, "{name}: {r:?}");
        let growth = r["rss_end_kb"].saturating_sub(r["rss_tenth_kb"]);
        assert!(
            growth <= RSS_GROWTH_LIMIT_KB,
            "{name}: RSS grew {growth} kB over the last nine tenths of {n} cycles: {r:?}"
        );
        assert_eq!(
            r["fds_end"], r["fds_warm"],
            "{name}: descriptors leaked: {r:?}"
        );
        assert_eq!(r["persistent_handles"], 0, "{name}: {r:?}");
        // Every image freed with its isolate is gone from the device too.
        assert_eq!(r["gpu_images"], 0, "{name}: GPU images leaked: {r:?}");
        // The atlas within its limit, and the placements beside it pruned
        // as it evicts (glyphs.rs).
        assert!(r["atlas_side"] <= 4096, "{name}: {r:?}");
        assert!(
            r["atlas_placements"] <= 2 * r["atlas_glyphs"] + 1024,
            "{name}: placements outgrew the atlas: {r:?}"
        );
        // Static text: no atlas write on a frame that draws only glyphs
        // already in it (brief M3 §15, atlas uploads).
        if matches!(name, "text" | "counter") {
            assert!(r["atlas_uploads"] <= u64::from(n), "{name}: {r:?}");
        }
        println!("{name}: {r:?}");
    }
}

#[test]
#[ignore = "soak lane: cargo test --release -p fl-dart-cli --test soak -- --ignored"]
fn vm_start_stop_in_100_processes() {
    // A Vm is once per process, so a full init/cleanup cycle is a process.
    let prog = compile("hello", MODE.0);
    for i in 0..100 {
        let o = fl_dart(&[MODE.1.as_ref(), prog.as_os_str()]);
        assert!(o.status.success(), "run {i}: {o:?}");
        assert_eq!(
            String::from_utf8_lossy(&o.stdout),
            "Hello World\n",
            "run {i}"
        );
    }
}

/// AOT only. The JIT VM's code-memory reservation is an `mmap` valgrind
/// refuses (`virtual_memory_posix.cc: mmap failed: 22`), so a JIT run aborts
/// at startup under it.
#[cfg(feature = "aot")]
#[test]
#[ignore = "soak lane: cargo test --release -p fl-dart-cli --test soak -- --ignored"]
fn valgrind_finds_no_errors_or_leaks() {
    use std::process::Command;

    let valgrind = |args: &[&std::ffi::OsStr]| {
        let log = common::out_dir().join("valgrind.log");
        let o = Command::new("valgrind")
            .args([
                "--leak-check=full",
                "--show-leak-kinds=definite,indirect",
                "--errors-for-leak-kinds=definite,indirect",
                "--error-exitcode=99",
                "--num-callers=30",
            ])
            .arg(format!("--log-file={}", log.display()))
            // The binary itself, never FL_DART_TEST_RUNNER: this lane is host-only.
            .arg(env!("CARGO_BIN_EXE_fl-dart"))
            .args(args)
            .output()
            .expect("valgrind must be on PATH for the soak lane");
        (o, std::fs::read_to_string(&log).unwrap_or_default())
    };
    let check = |what: &str, o: &std::process::Output, log: &str, want: i32| {
        assert_ne!(
            o.status.code(),
            Some(99),
            "{what}: valgrind reported errors:\n{log}"
        );
        assert_eq!(o.status.code(), Some(want), "{what}: {o:?}\n{log}");
        assert!(log.contains("ERROR SUMMARY: 0 errors"), "{what}:\n{log}");
    };
    for name in PROGRAMS {
        let prog = compile(name, MODE.0);
        let (o, log) = valgrind(&[MODE.1.as_ref(), prog.as_os_str()]);
        check(name, &o, &log, 0);
    }
    // An uncaught exception exits 255 by design; the error path must be as
    // clean as the success path.
    let prog = compile("throws", MODE.0);
    let (o, log) = valgrind(&[MODE.1.as_ref(), prog.as_os_str()]);
    check("throws", &o, &log, 255);
    // Isolate lifecycles under memcheck, where a per-cycle leak is named.
    let prog = compile("ffi", MODE.0);
    let (o, log) = valgrind(&["cycle".as_ref(), prog.as_os_str(), "100".as_ref()]);
    check("cycle ffi x100", &o, &log, 0);
}

/// App lifecycles under memcheck: ten cycles each of the apps whose peers
/// differ (pictures, scenes and layers; paths; reply callbacks; images,
/// codecs and the decode thread; paragraphs and fonts; an animation's
/// frames). A peer a finalizer misses, or a retained layer never freed, is
/// named here.
#[cfg(feature = "aot")]
#[test]
#[ignore = "soak lane: needs FLUTTER_ROOT and valgrind"]
fn valgrind_finds_no_errors_or_leaks_in_app_cycles() {
    use std::process::Command;

    let runs: [(&str, &[&str]); 6] = [
        ("colored_box", &[]),
        ("shapes", &[]),
        ("material", &[]),
        ("images", &[]),
        ("text", &[]),
        ("gif", &["--frames", "4"]),
    ];
    for (name, opts) in runs {
        let app = common::build_app(name);
        let log = common::out_dir().join(format!("valgrind-{name}.log"));
        let o = Command::new("valgrind")
            .args([
                "--leak-check=full",
                "--show-leak-kinds=definite,indirect",
                "--errors-for-leak-kinds=definite,indirect",
                "--error-exitcode=99",
                "--num-callers=30",
            ])
            .arg(format!("--log-file={}", log.display()))
            .arg(env!("CARGO_BIN_EXE_fl-dart"))
            .args(["app-cycle".as_ref(), app.as_os_str(), "10".as_ref()])
            .args(opts)
            .output()
            .expect("valgrind must be on PATH for the soak lane");
        let log = std::fs::read_to_string(&log).unwrap_or_default();
        assert_ne!(
            o.status.code(),
            Some(99),
            "{name}: valgrind reported errors:\n{log}"
        );
        assert!(o.status.success(), "{name}: {o:?}\n{log}");
        assert!(log.contains("ERROR SUMMARY: 0 errors"), "{name}:\n{log}");
    }
}

/// The sanitizer lanes' app check: the M2 and M3 apps, JIT from kernel,
/// through a few lifecycles each and short ticker, gif and text_churn
/// runs, under ASan or TSan. The UI
/// side only: no `--png`, so no raster thread, whose Vulkan driver is not
/// instrumented. That leaves dart:ui's natives, the peers and their
/// finalizers, the frame loop and the notify callback that reaches it from
/// the VM's own threads.
#[test]
#[ignore = "sanitizer lanes: see .github/workflows/sanitize.yml"]
fn apps_under_a_sanitizer() {
    let fonts = material_fonts();
    let fonts: Vec<&str> = fonts.iter().map(String::as_str).collect();
    let counter: Vec<&str> = ["2"].into_iter().chain(fonts.iter().copied()).collect();
    let churn: Vec<&str> = ["2", "400x300", "--frames", "50", "--hz", "1000"]
        .into_iter()
        .chain(fonts.iter().copied())
        .collect();
    let runs: [(&str, &[&str]); 9] = [
        ("colored_box", &["3"]),
        ("shapes", &["3", "--frames", "2"]),
        ("material", &["3"]),
        // The decode thread, and its results crossing to the UI thread.
        ("images", &["3"]),
        (
            "ticker",
            &["2", "200x100", "--frames", "20", "--hz", "1000"],
        ),
        // Paragraphs, fonts loaded from lists and from files.
        ("text", &["2"]),
        ("counter", &counter),
        // An animation's frames, decoded in turn on the decode thread.
        ("gif", &["2", "400x300", "--frames", "8"]),
        ("text_churn", &churn),
    ];
    for (name, args) in runs {
        let app = common::build_app(name);
        let mut a: Vec<&std::ffi::OsStr> = vec!["app-cycle".as_ref(), app.as_os_str()];
        a.extend(args.iter().map(std::ffi::OsStr::new));
        let o = fl_dart(&a);
        assert!(
            o.status.success(),
            "{name}: {}\n{}",
            o.status,
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

/// The sanitizer lanes' lifecycle check: every soak program through a short
/// run of isolate cycles. Instrumented JIT is several times slower, so 25
/// cycles, not 1,000; the sanitizer reports the first bad access or race, and
/// at exit LeakSanitizer any leak. Either makes fl-dart exit non-zero.
#[test]
#[ignore = "sanitizer lanes: see .github/workflows/sanitize.yml"]
fn cycles_under_a_sanitizer() {
    for name in PROGRAMS {
        let prog = compile(name, MODE.0);
        let o = fl_dart(&["cycle".as_ref(), prog.as_os_str(), "25".as_ref()]);
        assert!(
            o.status.success(),
            "{name}: {}\n{}",
            o.status,
            String::from_utf8_lossy(&o.stderr)
        );
    }
}
