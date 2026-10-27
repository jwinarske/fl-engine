//! L2: real programs through the real binary.
//!
//! Each test compiles a program from `tests/dart` with the pinned SDK's own
//! `dart` tool and runs `fl-dart` on it. The SDK is found through
//! `FL_DART_SDK_BIN` (the `dart` executable). It is required, not optional: a
//! lane where it is missing fails here, rather than reporting a pass for
//! tests that never ran.

// Test support, not library code: a failed setup step should stop the test.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn dart() -> PathBuf {
    let p = std::env::var_os("FL_DART_SDK_BIN")
        .expect("FL_DART_SDK_BIN must name the pinned SDK's `dart` executable");
    PathBuf::from(p)
}

fn out_dir() -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("dart");
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn compile(name: &str, kind: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/dart")
        .join(format!("{name}.dart"));
    let ext = if kind == "kernel" { "dill" } else { "aot" };
    let out = out_dir().join(format!("{name}.{ext}"));
    let st = Command::new(dart())
        .args(["compile", kind])
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        st.status.success(),
        "dart compile {kind} {name}: {}",
        String::from_utf8_lossy(&st.stderr)
    );
    out
}

fn fl_dart(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fl-dart"))
        .args(args)
        .output()
        .unwrap()
}

#[cfg(feature = "aot")]
const MODE: (&str, &str) = ("aot-snapshot", "run-aot");
#[cfg(feature = "jit")]
const MODE: (&str, &str) = ("kernel", "run");

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
