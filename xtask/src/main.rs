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
    // This repository is private, which decides the whole download shape: the
    // browser URL answers anonymous *and* Actions-token requests with 404, so
    // the fetch goes through the API asset endpoint, whose octet-stream form
    // redirects to a signed blob URL. The token rides a curl config file, not
    // argv, so a failed command echoed by run() cannot leak it.
    let token = std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .map_err(|_| {
            "GITHUB_TOKEN (or GH_TOKEN) is not set; the repository is private and \
             release assets are only served to it through the API"
                .to_owned()
        })?;
    let api = format!(
        "https://api.github.com/repos/jwinarske/fl-engine/releases/tags/dart-prebuilt-{PINNED_VERSION}"
    );
    let header_file = std::env::temp_dir().join("fl-dart-fetch-headers");
    std::fs::write(
        &header_file,
        format!("header = \"Authorization: Bearer {token}\"\n"),
    )
    .map_err(|e| format!("cannot write curl config: {e}"))?;
    let release = Command::new("curl")
        .args(["-fsSL", "--retry", "3", "-K"])
        .arg(&header_file)
        .args(["-H", "Accept: application/vnd.github+json", &api])
        .output()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if !release.status.success() {
        return Err(format!(
            "listing release dart-prebuilt-{PINNED_VERSION} failed: {}",
            String::from_utf8_lossy(&release.stderr)
        ));
    }
    // One asset's id, from JSON scanned rather than deserialized: the id is
    // the first "id" key after the "name" whose value is ours, per the
    // response's field order is not promised -- so scan the other way, each
    // asset object's id precedes its name in the payload GitHub serves, and
    // the pairing below tolerates either order by tracking the latest id.
    let body = String::from_utf8_lossy(&release.stdout);
    let asset_id =
        asset_id_for(&body, &name).ok_or_else(|| format!("release has no asset named {name}"))?;
    let asset_url =
        format!("https://api.github.com/repos/jwinarske/fl-engine/releases/assets/{asset_id}");
    let mut curl = Command::new("curl");
    curl.args(["-fL", "--retry", "3", "-K"])
        .arg(&header_file)
        .args(["-H", "Accept: application/octet-stream", "-o"])
        .arg(&tmp)
        .arg(&asset_url);
    let fetched = run(&mut curl);
    let _ = std::fs::remove_file(&header_file);
    fetched?;

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

/// Finds the asset id whose `"name"` equals `name` in a release JSON payload,
/// pairing each asset's most recent `"id"` with the next `"name"` seen, which
/// holds for either field order within an object.
fn asset_id_for(body: &str, name: &str) -> Option<u64> {
    let needle = format!("\"name\":\"{name}\"");
    let mut last_id: Option<u64> = None;
    let mut rest = body;
    loop {
        let id_at = rest.find("\"id\":");
        let name_at = rest.find(&needle);
        match (id_at, name_at) {
            (Some(i), Some(n)) if i < n => {
                let after = &rest[i + 5..];
                let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                last_id = digits.parse().ok().or(last_id);
                rest = &rest[i + 5..];
            }
            (_, Some(n)) => {
                // The name matched; the id already seen for this object is it.
                // If none preceded it, look ahead for the next "id".
                if last_id.is_some() {
                    return last_id;
                }
                let after = &rest[n..];
                let i = after.find("\"id\":")?;
                let digits: String = after[i + 5..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                return digits.parse().ok();
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    /// The shape GitHub actually serves: each asset's `id` precedes its
    /// `name`, other `id` keys (uploader, release) surround them.
    #[test]
    fn asset_id_is_the_one_paired_with_the_name() {
        let body = r#"{"id":1111,"assets":[
            {"id":681784949,"name":"fl-dart-prebuilt-3.13.7-x86_64-unknown-linux-gnu-jit.tar.xz","uploader":{"id":22}},
            {"id":681784951,"name":"fl-dart-prebuilt-3.13.7-x86_64-unknown-linux-gnu-aot.tar.xz","uploader":{"id":22}},
            {"id":681784959,"name":"SHA256SUMS","uploader":{"id":22}}]}"#;
        assert_eq!(
            super::asset_id_for(
                body,
                "fl-dart-prebuilt-3.13.7-x86_64-unknown-linux-gnu-aot.tar.xz"
            ),
            Some(681784951)
        );
        assert_eq!(super::asset_id_for(body, "SHA256SUMS"), Some(681784959));
        assert_eq!(super::asset_id_for(body, "absent.tar.xz"), None);
    }
}
