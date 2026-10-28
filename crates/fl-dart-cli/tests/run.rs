//! L2: real programs through the real binary.
//!
//! Each test compiles a program from `tests/dart` with the pinned SDK's own
//! `dart` tool and runs `fl-dart` on it. The SDK is found through
//! `FL_DART_SDK_BIN` (the `dart` executable). It is required, not optional: a
//! lane where it is missing fails here, rather than reporting a pass for
//! tests that never ran.
//!
//! Cross lanes run this suite for another architecture under qemu. Cargo's
//! `CARGO_TARGET_<TRIPLE>_RUNNER` runs the test binary there, and
//! `FL_DART_TEST_RUNNER` (the same command, e.g. `qemu-aarch64 -L
//! /usr/aarch64-linux-gnu`) runs `fl-dart`, which cargo does not wrap. AOT
//! programs are compiled for the architecture this binary was built for;
//! `dart` itself still runs on the host.

// Test support, not library code: a failed setup step should stop the test.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{compile, fl_dart, fl_dart_cmd, out_dir, MODE};
use std::process::Output;

fn run(name: &str, extra: &[&str]) -> Output {
    let prog = compile(name, MODE.0);
    let mut args: Vec<&std::ffi::OsStr> = vec![MODE.1.as_ref(), prog.as_os_str()];
    args.extend(extra.iter().map(|s| std::ffi::OsStr::new(*s)));
    fl_dart(&args)
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn hello_world_reaches_the_rust_sink_and_exits_zero() {
    let o = run("hello", &[]);
    assert!(o.status.success(), "{o:?}");
    assert_eq!(stdout(&o), "Hello World\n");
}

#[test]
fn run_loop_drains_microtasks_timers_and_futures() {
    let o = run("loop", &["a", "b"]);
    assert!(o.status.success(), "{o:?}");
    assert_eq!(stdout(&o), "main\nmicrotask\nfuture\nargs=a,b\ntimer\n");
}

#[test]
fn unhandled_exception_carries_the_dart_stack_trace() {
    let o = run("throws", &[]);
    assert_eq!(o.status.code(), Some(255), "{o:?}");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("unhandled exception: Bad state: thrown on purpose"),
        "{err}"
    );
    assert!(
        err.contains("inner"),
        "stack trace names the throwing function: {err}"
    );
}

#[test]
fn ffi_crossings_both_directions() {
    let o = run("ffi", &[]);
    assert!(o.status.success(), "{o:?}");
    assert_eq!(stdout(&o), "add=42\nlen=11\nport=from rust,7\n");
}

#[test]
fn snapshot_from_another_sdk_is_named_as_a_version_mismatch() {
    // A copy of a good program with its producer's hash overwritten: the
    // same bytes another SDK would have written, minus the need for one.
    let good = compile("hello", MODE.0);
    let mut bytes = std::fs::read(&good).unwrap();
    let hash = if cfg!(feature = "jit") {
        fl_dart_vm::sys::DART_SDK_REVISION.as_bytes()[..10].to_vec()
    } else {
        fl_dart_vm::sys::DART_SNAPSHOT_HASH.as_bytes().to_vec()
    };
    let at = bytes
        .windows(hash.len())
        .position(|w| w == hash.as_slice())
        .expect("producer hash in header");
    bytes[at..at + hash.len()].fill(b'f');
    let bad = out_dir().join(format!(
        "mismatch.{}",
        if cfg!(feature = "jit") { "dill" } else { "aot" }
    ));
    std::fs::write(&bad, &bytes).unwrap();

    let o = fl_dart(&[MODE.1.as_ref(), bad.as_os_str()]);
    assert_eq!(o.status.code(), Some(254), "{o:?}");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("snapshot version mismatch"), "{err}");
    assert!(err.contains("ffffffffff"), "names what it found: {err}");
}

#[test]
fn program_named_relative_to_the_working_directory() {
    // `fl-dart run-aot hello.aot`, as the README has it: a bare file name,
    // resolved against the current directory. dlopen alone searches the
    // loader path for such a name and does not find it.
    let prog = compile("hello", MODE.0);
    let dir = prog.parent().unwrap();
    let name = prog.file_name().unwrap();
    let o = fl_dart_cmd()
        .current_dir(dir)
        .arg(MODE.1)
        .arg(name)
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    assert_eq!(stdout(&o), "Hello World\n");
}

#[test]
fn missing_program_is_named() {
    let o = fl_dart(&[MODE.1.as_ref(), "no-such-program".as_ref()]);
    assert_eq!(o.status.code(), Some(255), "{o:?}");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("no-such-program"), "{err}");
}

#[test]
fn main_taking_args_runs_with_none() {
    // `main(List<String> args)` with no arguments gets an empty list, not
    // null, which its parameter type would refuse.
    let o = run("loop", &[]);
    assert!(o.status.success(), "{o:?}");
    assert!(stdout(&o).contains("args=\n"), "{}", stdout(&o));
}

#[test]
fn racing_posts_all_arrive_in_order() {
    let o = run("burst", &[]);
    assert!(o.status.success(), "{o:?}");
    assert_eq!(stdout(&o), "burst=8000 ok\n");
}

#[cfg(feature = "aot")]
#[test]
fn run_app_refuses_a_program_without_dart_ui() {
    let prog = compile("hello", MODE.0);
    let o = fl_dart(&["run-app".as_ref(), prog.as_os_str()]);
    assert_eq!(o.status.code(), Some(255), "{o:?}");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("dart:ui"), "names what is missing: {err}");
}
