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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;

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

/// `dart compile --target-arch` for the architecture under test. Kernel is
/// architecture-independent; only AOT snapshots need it.
const DART_TARGET_ARCH: Option<&str> = if cfg!(target_arch = "aarch64") {
    Some("arm64")
} else if cfg!(target_arch = "riscv64") {
    Some("riscv64")
} else {
    None
};

/// Compiled programs, by name and kind. Tests run in parallel and several
/// compile the same program: without this they raced to write one output file,
/// and a test could read it half written.
static COMPILED: Mutex<Option<HashMap<(String, String), PathBuf>>> = Mutex::new(None);

fn compile(name: &str, kind: &str) -> PathBuf {
    let mut cache = COMPILED.lock().unwrap_or_else(|e| e.into_inner());
    let key = (name.to_owned(), kind.to_owned());
    if let Some(p) = cache.get_or_insert_with(HashMap::new).get(&key) {
        return p.clone();
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/dart")
        .join(format!("{name}.dart"));
    let ext = if kind == "kernel" { "dill" } else { "aot" };
    let out = out_dir().join(format!("{name}.{ext}"));
    let mut cmd = Command::new(dart());
    cmd.args(["compile", kind]);
    if let (Some(arch), "aot-snapshot") = (DART_TARGET_ARCH, kind) {
        cmd.args(["--target-os", "linux", "--target-arch", arch]);
    }
    let st = cmd.arg(&src).arg("-o").arg(&out).output().unwrap();
    assert!(
        st.status.success(),
        "dart compile {kind} {name}: {}",
        String::from_utf8_lossy(&st.stderr)
    );
    cache
        .get_or_insert_with(HashMap::new)
        .insert(key, out.clone());
    out
}

/// `fl-dart`, wrapped in `FL_DART_TEST_RUNNER` when a cross lane sets it.
fn fl_dart_cmd() -> Command {
    let bin = env!("CARGO_BIN_EXE_fl-dart");
    match std::env::var("FL_DART_TEST_RUNNER") {
        Ok(r) if !r.trim().is_empty() => {
            let mut parts = r.split_whitespace();
            let mut c = Command::new(parts.next().unwrap());
            c.args(parts).arg(bin);
            c
        }
        _ => Command::new(bin),
    }
}

fn fl_dart(args: &[&std::ffi::OsStr]) -> Output {
    fl_dart_cmd().args(args).output().unwrap()
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
