//! Repository tooling: `cargo xtask <command>`.
//!
//! - `bindgen` regenerates `fl-dart-sys`'s checked-in bindings from the headers
//!   under `third_party/dart-prebuilt/<version>/include`. Needs the `bindgen`
//!   CLI (`cargo install bindgen-cli --version 0.72.1 --locked`) and libclang.
//! - `fetch-prebuilt` downloads and unpacks the pinned VM artifacts.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Must agree with `fl-dart-sys`. Read from there rather than repeated would be
/// better, and is not worth a dependency on the crate this regenerates.
const PINNED_VERSION: &str = "3.13.7";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("bindgen") => bindgen(),
        Some("fetch-prebuilt") => fetch_prebuilt(&args[1..]),
        _ => Err("usage: cargo xtask bindgen\n       cargo xtask fetch-prebuilt [--target <triple>] [--mode jit|aot]".to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn bindgen() -> Result<(), String> {
    let root = root();
    let include = root
        .join("third_party/dart-prebuilt")
        .join(PINNED_VERSION)
        .join("include");
    let out = root.join("crates/fl-dart-sys/src/bindings");

    // C enums become newtypes over their integer, not Rust enums: the VM is
    // free to hand back a value a newer header added, and a Rust enum holding a
    // discriminant it does not declare is undefined behaviour.
    let common = [
        "--use-core",
        "--default-enum-style",
        "newtype",
        "--no-prepend-enum-name",
        "--merge-extern-blocks",
        "--sort-semantically",
    ];
    run_bindgen(
        &include.join("dart_native_api.h"),
        &out.join("dart_api.rs"),
        &common,
        &["--allowlist-file", ".*/dart_(api|native_api)\\.h"],
        &include,
    )?;
    // The tools header is bound on its own and leans on the types above, so it
    // is generated without following references back into dart_api.h.
    run_bindgen(
        &include.join("dart_tools_api.h"),
        &out.join("dart_tools_api.rs"),
        &common,
        &[
            "--allowlist-file",
            ".*/dart_tools_api\\.h",
            "--no-recursive-allowlist",
        ],
        &include,
    )
}

fn run_bindgen(
    header: &Path,
    out: &Path,
    common: &[&str],
    extra: &[&str],
    include: &Path,
) -> Result<(), String> {
    let status = Command::new("bindgen")
        .arg(header)
        .arg("-o")
        .arg(out)
        .args(common)
        .args(extra)
        .arg("--")
        .arg(format!("-I{}", include.display()))
        .status()
        .map_err(|e| format!("cannot run `bindgen` (is bindgen-cli installed?): {e}"))?;
    if status.success() {
        println!("wrote {}", out.display());
        Ok(())
    } else {
        Err(format!("bindgen failed on {}: {status}", header.display()))
    }
}

/// Where the pipeline publishes (`.github/workflows/dart-prebuilt.yml`).
const RELEASES: &str = "https://github.com/jwinarske/fl-engine/releases/download";

/// `fetch-prebuilt [--target <triple>] [--mode jit|aot]`: downloads one
/// artifact tarball, checks it against the checked-in `SHA256SUMS`, and
/// unpacks it over `third_party/dart-prebuilt/`.
///
/// The checksum file is the trust root: it is reviewed into the repository
/// when a release is published, so a tarball that changed after review does
/// not unpack. Without it there is nothing to fetch, and this says so rather
/// than downloading something unverified.
fn fetch_prebuilt(args: &[String]) -> Result<(), String> {
    let mut target = host_target().to_owned();
    let mut mode = "aot".to_owned();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let v = it.next().ok_or_else(|| format!("{a} needs a value"))?;
        match a.as_str() {
            "--target" => target = v.clone(),
            "--mode" => mode = v.clone(),
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    let dir = root().join("third_party/dart-prebuilt");
    let sums_path = dir.join(PINNED_VERSION).join("SHA256SUMS");
    let sums = std::fs::read_to_string(&sums_path).map_err(|e| {
        format!(
            "{}: {e}\nno artifacts for Dart {PINNED_VERSION} have been published and \
             reviewed yet: run the dart-prebuilt workflow with publish=true, then check in \
             its SHA256SUMS",
            sums_path.display()
        )
    })?;
    let name = format!("fl-dart-prebuilt-{PINNED_VERSION}-{target}-{mode}.tar.xz");
    let want = sums
        .lines()
        .filter_map(|l| l.split_once(char::is_whitespace))
        .find(|(_, f)| f.trim().trim_start_matches('*') == name)
        .map(|(h, _)| h.to_owned())
        .ok_or_else(|| format!("{} has no entry for {name}", sums_path.display()))?;

    let tmp = std::env::temp_dir().join(&name);
    let url = format!("{RELEASES}/dart-prebuilt-{PINNED_VERSION}/{name}");
    let mut curl = Command::new("curl");
    curl.args(["-fL", "--retry", "3", "-o"]).arg(&tmp).arg(&url);
    // No Authorization header: the browser download endpoint answers an
    // Actions token with 404, and GITHUB_TOKEN is always set on a runner --
    // which is how ci's first fetch failed. The repository is public; a
    // private one would need the API asset endpoint, not this URL with a
    // bearer token.
    run(&mut curl)?;

    let out = Command::new("sha256sum")
        .arg(&tmp)
        .output()
        .map_err(|e| format!("cannot run sha256sum: {e}"))?;
    let got = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_owned();
    if got != want {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "{name}: sha256 is {got}, SHA256SUMS says {want}; not unpacked"
        ));
    }
    run(Command::new("tar")
        .arg("-xJf")
        .arg(&tmp)
        .arg("-C")
        .arg(&dir))?;
    let _ = std::fs::remove_file(&tmp);
    println!("unpacked {name} into {}", dir.display());
    Ok(())
}

fn host_target() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "aarch64-unknown-linux-gnu",
        "riscv64" => "riscv64gc-unknown-linux-gnu",
        _ => "x86_64-unknown-linux-gnu",
    }
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd
        .status()
        .map_err(|e| format!("cannot run {cmd:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} failed: {status}"))
    }
}
