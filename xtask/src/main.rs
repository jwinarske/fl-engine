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
        Some("fetch-prebuilt") => fetch_prebuilt(),
        _ => Err("usage: cargo xtask <bindgen | fetch-prebuilt>".to_owned()),
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

fn fetch_prebuilt() -> Result<(), String> {
    // No release carries the artifacts yet: the pipeline that builds them needs
    // a gclient checkout, and the first one is still being brought up (see
    // docs/m1-status.md). Saying so beats a download that 404s.
    Err(format!(
        "no published artifacts for Dart {PINNED_VERSION} yet; build them with \
         the artifact pipeline and point FL_DART_SDK_OUT at the result"
    ))
}
