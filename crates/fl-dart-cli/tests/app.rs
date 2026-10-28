//! Flutter framework apps through dart:ui (brief M2 §8), via the real binary.
//!
//! Each app under `apps/` is built with the pinned Flutter release's own
//! compilers (`tools/flutter-app/build.sh`), so these need `FLUTTER_ROOT`
//! and are `#[ignore]`d; the app lane in ci.yml runs them:
//!
//!     FLUTTER_ROOT=/path/to/flutter cargo test -p fl-dart-cli --test app -- --ignored

#![cfg(feature = "aot")]
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

fn build_app(name: &str) -> PathBuf {
    let flutter = std::env::var_os("FLUTTER_ROOT")
        .expect("FLUTTER_ROOT must name the pinned Flutter release");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = common::out_dir().join("apps");
    let st = Command::new(repo.join("tools/flutter-app/build.sh"))
        .arg(&flutter)
        .arg(repo.join("apps").join(name))
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        st.status.success(),
        "building {name}: {}",
        String::from_utf8_lossy(&st.stderr)
    );
    out.join(format!("{name}.so"))
}

#[test]
#[ignore = "app lane: needs FLUTTER_ROOT"]
fn colored_box_renders_its_first_frame() {
    let app = build_app("colored_box");
    let o = common::fl_dart(&["run-app".as_ref(), app.as_os_str(), "800x600@2".as_ref()]);
    assert!(o.status.success(), "{o:?}");
    // The warm-up frame: the root transform layer (the DPR scale), one
    // picture, one drawRect (docs/m2-dart-ui.md §3).
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "frame 1: view 0 800x600 layers=1 pictures=1 draws=1\n"
    );
}
