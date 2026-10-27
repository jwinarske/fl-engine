//! `fl-dart`: run a Dart program in a Rust-hosted VM.
//!
//! ```text
//! fl-dart run hello.dill [args...]       # JIT, dev hosts (--features jit)
//! fl-dart run-aot hello.aot [args...]    # precompiled runtime, every target
//! fl-dart cycle <program> <n>            # leak harness: n isolate lifecycles
//! ```
//!
//! Exit codes follow the standalone `dart` tool: 0 on success, 254 when the
//! program cannot be loaded, 255 when it throws or the VM fails.

mod ffi;

use std::process::ExitCode;

use fl_dart_vm::{DartError, IsolateConfig, Vm, VmConfig};

const EXIT_COMPILE: u8 = 254;
const EXIT_ERROR: u8 = 255;

fn main() -> ExitCode {
    // Touch the table so it is not discarded with the functions it holds.
    std::hint::black_box(&ffi::KEEP);

    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, path, rest) = match args.as_slice() {
        [cmd, path, rest @ ..] => (cmd.as_str(), path.as_str(), rest),
        _ => return usage(),
    };
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    let result = match cmd {
        #[cfg(feature = "jit")]
        "run" => run_kernel(path, &rest),
        #[cfg(feature = "aot")]
        "run-aot" => run_aot(path, &rest),
        "cycle" => match rest.first().and_then(|n| n.parse().ok()) {
            Some(n) => cycle(path, n),
            None => return usage(),
        },
        _ => return usage(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fl-dart: {e}");
            ExitCode::from(match e {
                Failure::Dart(
                    DartError::Compilation(_) | DartError::SnapshotVersionMismatch { .. },
                ) => EXIT_COMPILE,
                _ => EXIT_ERROR,
            })
        }
    }
}

fn usage() -> ExitCode {
    let modes = if cfg!(feature = "jit") {
        "run <file.dill>"
    } else {
        "run-aot <file.aot>"
    };
    let prog = if cfg!(feature = "jit") {
        "file.dill"
    } else {
        "file.aot"
    };
    eprintln!("usage: fl-dart {modes} [args...]\n       fl-dart cycle <{prog}> <n>");
    ExitCode::from(64)
}

#[derive(Debug)]
enum Failure {
    Dart(DartError),
    Other(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Dart(e) => e.fmt(f),
            Failure::Other(s) => f.write_str(s),
        }
    }
}

impl From<DartError> for Failure {
    fn from(e: DartError) -> Self {
        Failure::Dart(e)
    }
}

/// The VM, checked against the version the bindings and snapshots assume.
/// A VM that is not the pinned one is refused before anything is loaded into
/// it; this is the run-time half of the build script's VERSION check.
fn start_vm() -> Result<Vm, Failure> {
    let vm = Vm::init(VmConfig::default()).map_err(|e| Failure::Other(e.to_string()))?;
    let v = vm.version_string();
    let pinned = fl_dart_vm::sys::DART_SDK_VERSION;
    if v.split_whitespace().next() != Some(pinned) {
        return Err(Failure::Dart(DartError::SnapshotVersionMismatch {
            expected: format!("Dart {pinned}"),
            found: format!("linked VM {v}"),
        }));
    }
    Ok(vm)
}

#[cfg(feature = "jit")]
fn run_kernel(path: &str, args: &[&str]) -> Result<(), Failure> {
    let dill = std::fs::read(path).map_err(|e| Failure::Other(format!("{path}: {e}")))?;
    let vm = start_vm()?;
    let mut isolate = vm.isolate_from_kernel(&dill, config(path))?;
    run_main(&mut isolate, args)?;
    isolate.shutdown();
    Ok(())
}

#[cfg(feature = "aot")]
fn run_aot(path: &str, args: &[&str]) -> Result<(), Failure> {
    let snapshot = std::sync::Arc::new(fl_dart_vm::AotSnapshot::open(path.as_ref())?);
    let vm = start_vm()?;
    let mut isolate = vm.isolate_from_aot(&snapshot, config(path))?;
    run_main(&mut isolate, args)?;
    isolate.shutdown();
    Ok(())
}

fn config(path: &str) -> IsolateConfig {
    IsolateConfig {
        script_uri: path.to_owned(),
        ..IsolateConfig::default()
    }
}

fn run_main(isolate: &mut fl_dart_vm::Isolate<'_>, args: &[&str]) -> Result<(), DartError> {
    isolate.enter()?.with(|s| {
        s.invoke_main(args)?;
        s.run_loop()
    })
}

/// The L5 harness's in-process half (brief §11): create, run and shut down
/// `n` isolates from one program in one VM, and report resident memory and
/// open descriptors before and after. The lane compares the two, under ASan
/// or valgrind; a count that grows with `n` is the leak.
fn cycle(path: &str, n: u32) -> Result<(), Failure> {
    let vm = start_vm()?;
    #[cfg(feature = "jit")]
    let program = std::fs::read(path).map_err(|e| Failure::Other(format!("{path}: {e}")))?;
    #[cfg(feature = "aot")]
    let program = std::sync::Arc::new(fl_dart_vm::AotSnapshot::open(path.as_ref())?);
    let quiet = IsolateConfig {
        print: std::sync::Arc::new(|_| {}),
        ..config(path)
    };

    // Samples after the first cycle (which warms caches the VM keeps for its
    // lifetime), after a tenth of the run, and at the end. Growth from the
    // tenth to the end is what a per-cycle leak shows up in; the first
    // cycle's one-time growth does not count against it.
    let tenth = (n / 10).max(1);
    let (mut warm, mut mid) = (None, None);
    for i in 0..n {
        #[cfg(feature = "jit")]
        let mut isolate = vm.isolate_from_kernel(&program, quiet.clone())?;
        #[cfg(feature = "aot")]
        let mut isolate = vm.isolate_from_aot(&program, quiet.clone())?;
        run_main(&mut isolate, &[])?;
        isolate.shutdown();
        if i == 0 {
            warm = Some(usage_now());
        }
        if i + 1 == tenth {
            mid = Some(usage_now());
        }
    }
    let end = usage_now();
    let warm = warm.unwrap_or(end);
    let mid = mid.unwrap_or(end);
    println!(
        "cycles={n} rss_warm_kb={} rss_tenth_kb={} rss_end_kb={} fds_warm={} fds_end={} persistent_handles={}",
        warm.0,
        mid.0,
        end.0,
        warm.1,
        end.1,
        fl_dart_vm::live_persistent_handles()
    );
    Ok(())
}

/// (VmRSS in kB, open descriptors), from procfs.
fn usage_now() -> (u64, usize) {
    let rss = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("VmRSS:"))
                .and_then(|v| v.split_whitespace().next()?.parse().ok())
        })
        .unwrap_or(0);
    let fds = std::fs::read_dir("/proc/self/fd")
        .map(|d| d.count())
        .unwrap_or(0);
    (rss, fds)
}
