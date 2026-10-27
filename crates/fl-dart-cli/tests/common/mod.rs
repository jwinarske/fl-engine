//! Harness shared by the L2 and soak suites. See `run.rs` for the
//! environment both read.

// Each suite uses part of this; the rest would warn as dead in that crate.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;

pub fn dart() -> PathBuf {
    let p = std::env::var_os("FL_DART_SDK_BIN")
        .expect("FL_DART_SDK_BIN must name the pinned SDK's `dart` executable");
    PathBuf::from(p)
}

pub fn out_dir() -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("dart");
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `dart compile --target-arch` for the architecture under test. Kernel is
/// architecture-independent; only AOT snapshots need it.
pub const DART_TARGET_ARCH: Option<&str> = if cfg!(target_arch = "aarch64") {
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

pub fn compile(name: &str, kind: &str) -> PathBuf {
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
pub fn fl_dart_cmd() -> Command {
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

/// Runs `fl-dart`. Under a cross runner, one rerun when qemu-user itself
/// crashed: "QEMU internal SIGSEGV" is a fault in the emulator's own code
/// (seen once in the racing-posts test on aarch64, addr=0x20, with the same
/// commit green before and after), which qemu-user is prone to when a guest
/// starts and ends many threads at once. A fault in fl-dart is a guest signal
/// and never carries that text, and a second emulator crash fails the test.
pub fn fl_dart(args: &[&std::ffi::OsStr]) -> Output {
    let o = fl_dart_cmd().args(args).output().unwrap();
    let emulator_crashed = std::env::var_os("FL_DART_TEST_RUNNER").is_some()
        && String::from_utf8_lossy(&o.stderr).contains("QEMU internal SIGSEGV");
    if !emulator_crashed {
        return o;
    }
    eprintln!(
        "qemu crashed in its own code, running once more: {}",
        String::from_utf8_lossy(&o.stderr).trim()
    );
    fl_dart_cmd().args(args).output().unwrap()
}

#[cfg(feature = "aot")]
pub const MODE: (&str, &str) = ("aot-snapshot", "run-aot");
#[cfg(feature = "jit")]
pub const MODE: (&str, &str) = ("kernel", "run");
