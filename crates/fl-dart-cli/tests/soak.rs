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
/// over that span is 0-8 kB.
const RSS_GROWTH_LIMIT_KB: u64 = 512;

fn cycle_report(prog: &std::path::Path, n: u32) -> HashMap<String, u64> {
    // One malloc arena. With glibc's default, burst's threads land in new
    // arenas now and then and RSS steps up 0.5-1.5 MB at random: 3,000
    // cycles measured flat, 6,000 grew 1.5 MB. With one arena 6,000 cycles
    // grow 40 kB. A leak still grows linearly here; valgrind checks the rest.
    let o = common::fl_dart_cmd()
        .env("MALLOC_ARENA_MAX", "1")
        .args(["cycle".as_ref(), prog.as_os_str(), n.to_string().as_ref()])
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
