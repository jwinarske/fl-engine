//! `fl-dart`: run a Dart program in a Rust-hosted VM.
//!
//! ```text
//! fl-dart run hello.dill [args...]       # JIT, dev hosts (--features jit)
//! fl-dart run-aot hello.aot [args...]    # precompiled runtime, every target
//! fl-dart run-app libapp.so [WxH[@dpr]] [--png out.png] [--frames-dir DIR] [--ops] [--natives] [--frames N] [--hz H]
//!                                       # a Flutter app, through dart:ui (M2);
//!                                       # JIT takes the app's kernel instead
//! fl-dart app-cycle libapp.so <n> [run-app options]
//!                                       # leak harness: n app lifecycles
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
        "run-app" => match app_args(&rest) {
            Some(app) => run_app(path, app),
            None => return usage(),
        },
        "app-cycle" => match (
            rest.first().and_then(|n| n.parse().ok()),
            app_args(rest.get(1..).unwrap_or_default()),
        ) {
            (Some(n), Some(app)) => app_cycle(path, n, app),
            _ => return usage(),
        },
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
    let app = if cfg!(feature = "jit") {
        "app.dill"
    } else {
        "libapp.so"
    };
    eprintln!(
        "       fl-dart run-app <{app}> [WxH[@dpr]] [--png out.png] [--frames-dir DIR] [--ops] [--natives] [--frames N] [--hz H]\n       fl-dart app-cycle <{app}> <n> [run-app options]"
    );
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

/// A Flutter app's `libapp.so`, with dart:ui's natives, run on fl-shell's
/// frame loop until it has nothing left to do, or until it has rendered
/// `--frames`. Each frame is reported on stdout; an app that renders none
/// is an error. With `--png`, every frame is drawn by emblema offscreen on
/// the raster thread and the last one written there. With frames begun at
/// vsync, a `vsync:` line reports their spacing.
fn run_app(path: &str, app: AppArgs) -> Result<(), Failure> {
    let program = load_app(path)?;
    let vm = start_vm()?;
    let raster = raster_for(&app)?;
    let run = app_once(&vm, &program, path, &app, raster.as_ref(), true)?;
    if let Some(line) = vsync_line(&run.report.frame_times_us) {
        println!("{line}");
    }
    if app.natives {
        println!("natives: {}", fl_ui::resolved().join(","));
    }
    finish_raster(raster, &app)
}

/// The L5 harness for apps: `n` start-and-stop cycles of one app in one VM,
/// each run as `run-app` runs it (to its end, or to `--frames`), quietly.
/// Reports what `cycle` reports, and the frames rendered in all. With
/// `--png` one raster thread draws every cycle's frames, so the raster side
/// churns too; it is made once, as a device is.
fn app_cycle(path: &str, n: u32, app: AppArgs) -> Result<(), Failure> {
    let program = load_app(path)?;
    let vm = start_vm()?;
    let raster = raster_for(&app)?;
    // Sampled as `cycle` samples: after the first cycle, after a tenth,
    // and at the end.
    let tenth = (n / 10).max(1);
    let (mut warm, mut mid, mut frames) = (None, None, 0u64);
    for i in 0..n {
        frames += app_once(&vm, &program, path, &app, raster.as_ref(), false)?.rendered;
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
    finish_raster(raster, &app)?;
    println!(
        "cycles={n} frames={frames} rss_warm_kb={} rss_tenth_kb={} rss_end_kb={} fds_warm={} fds_end={} persistent_handles={}",
        warm.0,
        mid.0,
        end.0,
        warm.1,
        end.1,
        fl_dart_vm::live_persistent_handles()
    );
    Ok(())
}

/// An app as this build runs it: AOT, its `libapp.so`; JIT, its kernel,
/// built against Flutter's non-product platform with that platform linked
/// in (`tools/flutter-app/build.sh ... kernel`).
#[cfg(feature = "aot")]
type AppProgram = std::sync::Arc<fl_dart_vm::AotSnapshot>;
#[cfg(feature = "jit")]
type AppProgram = Vec<u8>;

fn load_app(path: &str) -> Result<AppProgram, Failure> {
    #[cfg(feature = "aot")]
    let program = std::sync::Arc::new(fl_dart_vm::AotSnapshot::open(path.as_ref())?);
    #[cfg(feature = "jit")]
    let program = std::fs::read(path).map_err(|e| Failure::Other(format!("{path}: {e}")))?;
    Ok(program)
}

/// What one run of an app did.
struct AppRun {
    rendered: u64,
    report: fl_shell::Report,
}

/// One run of an app in `vm`: prepare, `main`, the frame loop, shutdown.
/// With `echo`, frames (and `--ops`) go to stdout and `print` to stdout;
/// without, both are dropped. An app that renders nothing is an error.
fn app_once(
    vm: &Vm,
    program: &AppProgram,
    path: &str,
    app: &AppArgs,
    raster: Option<&std::sync::Arc<fl_raster::RasterThread>>,
    echo: bool,
) -> Result<AppRun, Failure> {
    use std::sync::{Arc, Mutex};

    let rendered = Arc::new(Mutex::new(0u64));
    let sink_raster = raster.map(Arc::clone);
    let (sink_count, ops) = (Arc::clone(&rendered), app.ops);
    fl_ui::set_frame_sink(move |f| {
        let mut n = sink_count.lock().unwrap_or_else(|e| e.into_inner());
        *n += 1;
        if echo {
            println!(
                "frame {}: view {} {}x{} {}",
                *n,
                f.view_id,
                f.width,
                f.height,
                summary(&f.scene)
            );
            if ops {
                print!("{}", outline(&f.scene));
            }
        }
        if let Some(raster) = sink_raster.as_ref() {
            // A thread that is gone has its reason in `finish`.
            let _ = raster.submit(Arc::clone(&f.scene));
        }
    });
    let wake = fl_shell::Wake::default();
    let mut cfg = fl_shell::isolate_config(path, &wake);
    if !echo {
        cfg.print = Arc::new(|_| {});
    }
    #[cfg(feature = "aot")]
    let mut isolate = vm.isolate_from_aot(program, cfg)?;
    #[cfg(feature = "jit")]
    let mut isolate = vm.isolate_from_kernel(program, cfg)?;
    let view = app.view;
    let period = std::time::Duration::from_secs_f64(1.0 / view.refresh_rate);
    let limit = app.frames;
    let report = {
        let mut scope = isolate.enter()?;
        scope.with(|s| {
            fl_shell::prepare(s, &view)?;
            s.invoke_main(&[])
        })?;
        fl_shell::run(&mut scope, &wake, period, || {
            limit.is_some_and(|l| *rendered.lock().unwrap_or_else(|e| e.into_inner()) >= l)
        })
    };
    isolate.shutdown();
    // The sink is the process's, and holds the raster thread: replacing it
    // is what lets `finish` have the thread to itself.
    fl_ui::set_frame_sink(|_| {});
    let report = report?;
    let count = *rendered.lock().unwrap_or_else(|e| e.into_inner());
    if count == 0 {
        return Err(Failure::Other(format!(
            "{path}: ran to completion without rendering a frame"
        )));
    }
    Ok(AppRun {
        rendered: count,
        report,
    })
}

/// A raster thread at the view's size if `--png` asks for pixels.
fn raster_for(app: &AppArgs) -> Result<Option<std::sync::Arc<fl_raster::RasterThread>>, Failure> {
    if app.png.is_none() && app.frames_dir.is_none() {
        return Ok(None);
    }
    if let Some(dir) = &app.frames_dir {
        std::fs::create_dir_all(dir)
            .map_err(|e| Failure::Other(format!("{}: {e}", dir.display())))?;
    }
    let (w, h) = (
        app.view.width.round() as u32,
        app.view.height.round() as u32,
    );
    fl_raster::RasterThread::spawn_capturing(w, h, app.frames_dir.clone())
        .map(|r| Some(std::sync::Arc::new(r)))
        .map_err(|e| Failure::Other(format!("offscreen target: {e}")))
}

/// Stops the raster thread and writes its last frame to `--png`. Its first
/// failure is the run's.
fn finish_raster(
    raster: Option<std::sync::Arc<fl_raster::RasterThread>>,
    app: &AppArgs,
) -> Result<(), Failure> {
    let Some(raster) = raster else {
        return Ok(());
    };
    let r = std::sync::Arc::try_unwrap(raster)
        .map_err(|_| Failure::Other("the raster thread is still shared".into()))?
        .finish();
    if let Some(e) = r.error {
        return Err(Failure::Other(e));
    }
    if let (Some(png), Some(pixels)) = (app.png.as_ref(), r.last.as_ref()) {
        fl_raster::write_png(png, r.width, r.height, pixels)
            .map_err(|e| Failure::Other(e.to_string()))?;
    }
    Ok(())
}

/// `vsync: frames=… interval_us=…`: how many frames were begun at vsync, and
/// the spacing of their times, each interval in microseconds. `None` with
/// fewer than two.
fn vsync_line(times: &[i64]) -> Option<String> {
    if times.len() < 2 {
        return None;
    }
    let gaps: Vec<String> = times
        .windows(2)
        .map(|w| (w[1] - w[0]).to_string())
        .collect();
    Some(format!(
        "vsync: frames={} interval_us={}",
        times.len(),
        gaps.join(",")
    ))
}

/// `layers=… pictures=… draws=…` for a frame line.
fn summary(scene: &fl_ui::Scene) -> String {
    use fl_ui::LayerOp;
    let (mut layers, mut pictures, mut draws) = (0, 0, 0);
    for op in &scene.ops {
        match op {
            LayerOp::AddPicture { picture, .. } => {
                pictures += 1;
                draws += picture.draws.len();
            }
            LayerOp::Pop => {}
            _ => layers += 1,
        }
    }
    format!("layers={layers} pictures={pictures} draws={draws}")
}

/// A scene's operations, one per line, each picture's draws indented under
/// it: the kinds only, no geometry, so a test can check the structure the
/// framework built while the golden checks the pixels.
fn outline(scene: &fl_ui::Scene) -> String {
    use fl_ui::{Draw, LayerOp};
    use std::fmt::Write;
    // The variant name of a Debug rendering: `Rect { .. }` is `Rect`.
    fn kind(d: &impl std::fmt::Debug) -> String {
        let s = format!("{d:?}");
        s.split(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap_or_default()
            .to_owned()
    }
    fn draws(out: &mut String, depth: usize, list: &[Draw]) {
        for d in list {
            let _ = writeln!(out, "{:w$}{}", "", kind(d), w = depth * 2);
            if let Draw::Picture(p) = d {
                draws(out, depth + 1, &p.draws);
            }
        }
    }
    let mut out = String::new();
    let mut depth = 1usize;
    for op in &scene.ops {
        if matches!(op, LayerOp::Pop) {
            depth = depth.saturating_sub(1).max(1);
        }
        let _ = writeln!(out, "{:w$}{}", "", kind(op), w = depth * 2);
        match op {
            LayerOp::Pop => {}
            LayerOp::AddPicture { picture, .. } => draws(&mut out, depth + 1, &picture.draws),
            _ => depth += 1,
        }
    }
    out
}

/// `run-app`'s options.
#[derive(Debug, Default)]
struct AppArgs {
    view: fl_shell::ViewConfig,
    png: Option<std::path::PathBuf>,
    frames_dir: Option<std::path::PathBuf>,
    ops: bool,
    natives: bool,
    frames: Option<u64>,
}

/// `[WxH[@dpr]] [--png out.png] [--frames-dir DIR] [--ops] [--natives] [--frames N] [--hz H]`,
/// in any order; no size is the default view. Sizes are physical pixels.
/// `--frames-dir` writes every frame there, `frame-NNNN.png`, as `--png`
/// writes the last. `--ops` prints each frame's [`outline`]; `--natives`, at the end, every
/// dart:ui native the app called; `--frames` stops after N frames rendered;
/// `--hz` sets the vsync rate (60 by default).
fn app_args(args: &[&str]) -> Option<AppArgs> {
    let mut app = AppArgs::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--png" => app.png = Some(std::path::PathBuf::from(it.next()?)),
            "--frames-dir" => app.frames_dir = Some(std::path::PathBuf::from(it.next()?)),
            "--ops" => app.ops = true,
            "--natives" => app.natives = true,
            "--frames" => app.frames = Some(it.next()?.parse().ok().filter(|n| *n > 0)?),
            "--hz" => app.view.refresh_rate = it.next()?.parse().ok().filter(|v: &f64| *v > 0.0)?,
            _ => {
                let (size, dpr) = match a.split_once('@') {
                    Some((size, dpr)) => (size, Some(dpr)),
                    None => (*a, None),
                };
                let (w, h) = size.split_once('x')?;
                app.view.width = w.parse().ok().filter(|v: &f64| *v >= 1.0)?;
                app.view.height = h.parse().ok().filter(|v: &f64| *v >= 1.0)?;
                if let Some(dpr) = dpr {
                    app.view.device_pixel_ratio = dpr.parse().ok().filter(|v: &f64| *v > 0.0)?;
                }
            }
        }
    }
    Some(app)
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
