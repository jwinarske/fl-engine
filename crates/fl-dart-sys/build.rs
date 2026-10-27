//! Locates a prebuilt Dart VM and tells rustc how to link it.
//!
//! The whole contract is that this script never builds C++: it does not run
//! GN, fetch sources, or look for a compiler. It reads a directory someone else
//! produced (layout in `third_party/dart-prebuilt/README.md`), checks that the
//! directory is the VM these bindings were generated from, and emits link
//! lines. Anything missing is an error that says how to get it.
//!
//! The bindings themselves are generated ahead of time (`cargo xtask bindgen`)
//! and checked in, rather than generated here. bindgen needs libclang, and a
//! build that needs libclang is a build that needs a C toolchain installed --
//! exactly what exit criterion 8 says this crate must not need.

// A build script is not a library: a failed assumption here should stop the
// build, and does, with the message the error carries.
#![allow(clippy::panic)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// The VM these bindings were generated from. Must agree with
/// `src/lib.rs` and with the checked-in `VERSION` beside the headers.
const PINNED_VERSION: &str = "3.13.7";
const PINNED_REVISION: &str = "416e33b0ffaef062a60ec966bb8c82ed2b83f128";
const PINNED_SNAPSHOT_HASH: &str = "0451907c2eaa8467e848c0067bfe8ed4";

/// The value of `FL_DART_SDK_OUT` that asks for no VM at all.
///
/// For the lanes that never link -- `cargo check`, clippy, rustdoc, the
/// compile-fail suite. It is a word that has to be typed, rather than what
/// happens when nothing is found, so that a run without a VM is never mistaken
/// for a run with one: anything that does try to link then fails on undefined
/// `Dart_*` symbols, loudly.
const NO_VM: &str = "none";

const FETCH_HINT: &str = "run `cargo xtask fetch-prebuilt` to download the pinned \
     artifacts, or set FL_DART_SDK_OUT to a directory laid out as \
     third_party/dart-prebuilt/README.md describes";

fn main() {
    println!("cargo:rerun-if-env-changed=FL_DART_SDK_OUT");
    println!("cargo:rerun-if-changed=build.rs");

    let jit = env::var_os("CARGO_FEATURE_JIT").is_some();
    let aot = env::var_os("CARGO_FEATURE_AOT").is_some();
    let mode = match (jit, aot) {
        (true, false) => "jit",
        (false, true) => "aot",
        (true, true) => panic!(
            "fl-dart-sys: features `jit` and `aot` are both enabled. They link two \
             different builds of the VM and cannot share a process; enable one. \
             (Cargo unifies features across a build, so check what else in the \
             graph enables the other.)"
        ),
        (false, false) => panic!(
            "fl-dart-sys: enable exactly one of the features `jit` (x86_64 dev hosts) \
             or `aot` (every target)"
        ),
    };
    println!("cargo:rustc-cfg=fl_dart_{mode}");
    println!("cargo::rustc-check-cfg=cfg(fl_dart_jit, fl_dart_aot, fl_dart_unlinked)");

    let root = match env::var("FL_DART_SDK_OUT") {
        Ok(v) if v == NO_VM => {
            println!(
                "cargo:warning=fl-dart-sys: FL_DART_SDK_OUT=none, building without a \
                 Dart VM. Anything that links this crate will fail."
            );
            println!("cargo:rustc-cfg=fl_dart_unlinked");
            return;
        }
        Ok(v) => PathBuf::from(v),
        Err(_) => {
            let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
            manifest
                .join("../../third_party/dart-prebuilt")
                .join(PINNED_VERSION)
        }
    };
    println!("cargo:rerun-if-changed={}", root.display());

    check_version(&root);

    let target = env::var("TARGET").expect("set by cargo");
    let sanitizer = sanitizer();
    // A sanitized Rust build links the VM built with the same sanitizer, in
    // its own directory: `jit-asan`, `jit-tsan`. The variant follows
    // `-Zsanitizer`, so the two halves cannot be paired wrongly by hand.
    let variant = match sanitizer.as_deref() {
        None => mode.to_owned(),
        Some(s) if mode == "aot" => panic!(
            "fl-dart-sys: -Zsanitizer={s} needs the `jit` feature: sanitized VMs are \
             JIT builds, since an AOT snapshot's feature string must name the same \
             sanitizer and the stock SDK's do not"
        ),
        Some("address") => format!("{mode}-asan"),
        Some("thread") => format!("{mode}-tsan"),
        Some(other) => panic!(
            "fl-dart-sys: -Zsanitizer={other} has no prebuilt VM; the pipeline builds \
             address and thread"
        ),
    };
    let lib_dir = root.join("lib").join(&target).join(&variant);
    let manifest = lib_dir.join("link.txt");
    let text = fs::read_to_string(&manifest).unwrap_or_else(|e| {
        panic!(
            "fl-dart-sys: no {variant} libraries for {target}: cannot read {}: {e}\n{FETCH_HINT}",
            manifest.display()
        )
    });
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    // Downstream build scripts (the CLI's, which wants the snapshots beside the
    // libraries) read this as DEP_DART_ROOT.
    println!("cargo:root={}", root.display());

    let mut instrumented = false;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (kind, name) = line.split_once('=').unwrap_or_else(|| {
            panic!(
                "fl-dart-sys: {}:{}: expected `kind=name`, got `{raw}`",
                manifest.display(),
                n + 1
            )
        });
        match kind.trim() {
            "static" => {
                let file = lib_dir.join(format!("lib{}.a", name.trim()));
                if !file.is_file() {
                    panic!(
                        "fl-dart-sys: {} names {}, which is not there\n{FETCH_HINT}",
                        manifest.display(),
                        file.display()
                    );
                }
                println!("cargo:rustc-link-lib=static={}", name.trim());
            }
            // The archives were built against one C++ runtime; linking another
            // produces a binary that links and then fails in ways that are slow
            // to trace back here. The pipeline knows which; this obeys.
            "cxx" | "system" => println!("cargo:rustc-link-lib=dylib={}", name.trim()),
            // The sanitizer the archive was instrumented with. Its runtime is
            // not in the archive: rustc links its own for `-Zsanitizer`, and
            // two runtimes in one process do not work. A mismatch here means
            // FL_DART_SDK_OUT points at the wrong build.
            "sanitizer" => {
                if sanitizer.as_deref() != Some(name.trim()) {
                    panic!(
                        "fl-dart-sys: {} is a {} build, but this build has -Zsanitizer={}",
                        manifest.display(),
                        name.trim(),
                        sanitizer.as_deref().unwrap_or("<none>")
                    );
                }
                instrumented = true;
            }
            other => panic!(
                "fl-dart-sys: {}:{}: unknown directive `{other}`",
                manifest.display(),
                n + 1
            ),
        }
    }
    // An uninstrumented VM under a sanitized build links, and then the
    // sanitizer sees only the Rust half. That is a skip, so it is an error.
    if let (Some(s), false) = (&sanitizer, instrumented) {
        panic!(
            "fl-dart-sys: this build has -Zsanitizer={s}, but {} declares no \
             `sanitizer=` line: the VM there is not instrumented",
            manifest.display()
        );
    }
}

/// The `-Zsanitizer` of the build, as cargo reports the target's
/// `cfg(sanitize)`. One at a time: no prebuilt VM carries two.
fn sanitizer() -> Option<String> {
    let v = env::var("CARGO_CFG_SANITIZE").ok()?;
    if v.contains(',') {
        panic!("fl-dart-sys: -Zsanitizer={v}: one sanitizer at a time");
    }
    Some(v)
}

/// A VM other than the one the bindings came from is refused here, at build
/// time, rather than discovered as a struct-layout disagreement at run time.
fn check_version(root: &Path) {
    let path = root.join("VERSION");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "fl-dart-sys: cannot read {}: {e}\n{FETCH_HINT}",
            path.display()
        )
    });
    println!("cargo:rerun-if-changed={}", path.display());
    let field = |key: &str| {
        text.lines()
            .filter_map(|l| l.split_once('='))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().to_owned())
    };
    let version = field("version");
    let revision = field("revision");
    let hash = field("snapshot_hash");
    if version.as_deref() != Some(PINNED_VERSION)
        || revision.as_deref() != Some(PINNED_REVISION)
        || hash.as_deref() != Some(PINNED_SNAPSHOT_HASH)
    {
        panic!(
            "fl-dart-sys: {} is Dart {} ({}, snapshot {}), but these bindings are \
             for Dart {PINNED_VERSION} ({PINNED_REVISION}, snapshot \
             {PINNED_SNAPSHOT_HASH}). Fetch the pinned artifacts, or \
             regenerate the bindings for the new pin with `cargo xtask bindgen`.",
            path.display(),
            version.as_deref().unwrap_or("<no version>"),
            revision.as_deref().unwrap_or("<no revision>"),
            hash.as_deref().unwrap_or("<no snapshot_hash>"),
        );
    }
}
