//! A Flutter app's asset bundle (brief M3 §2): the directory a Linux bundle
//! keeps as `data/flutter_assets`, read the way the engine reads it.
//!
//! Every read goes through the rules of the M3 review (§15): a key is a
//! relative path that stays inside the bundle after symlinks are resolved;
//! only regular files are read, and only up to a size limit. A key can come
//! from data the app received, so none of this assumes it was written by the
//! app's author.
//!
//! No VM, no dart:ui: the natives that serve `flutter/assets` and
//! `ImmutableBuffer` are fl-ui's, over this.

use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// The default largest file read: 256 MiB.
pub const DEFAULT_LIMIT: u64 = 256 << 20;

/// Why a read failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetError {
    /// The key is not a relative path inside the bundle: empty, absolute,
    /// with a `..` or a NUL, or resolving outside it through a symlink.
    BadKey(String),
    /// Nothing is there.
    NotFound(String),
    /// Something is there, but not a regular file: a directory, a device,
    /// a FIFO.
    NotRegular(String),
    TooLarge {
        what: String,
        size: u64,
        limit: u64,
    },
    Io(String),
    /// `FontManifest.json` is not the shape the tool writes.
    Manifest(String),
}

impl std::fmt::Display for AssetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetError::BadKey(k) => write!(f, "not an asset key: {k:?}"),
            AssetError::NotFound(k) => write!(f, "no asset {k:?}"),
            AssetError::NotRegular(k) => write!(f, "{k:?} is not a regular file"),
            AssetError::TooLarge { what, size, limit } => {
                write!(f, "{what:?} is {size} bytes, over the {limit}-byte limit")
            }
            AssetError::Io(e) => write!(f, "{e}"),
            AssetError::Manifest(e) => write!(f, "FontManifest.json: {e}"),
        }
    }
}

impl std::error::Error for AssetError {}

/// An asset bundle on disk.
#[derive(Debug, Clone)]
pub struct Bundle {
    /// Canonical: symlinks resolved, so a containment check compares like
    /// with like.
    root: PathBuf,
    limit: u64,
}

impl Bundle {
    /// The bundle at `dir`, read up to [`DEFAULT_LIMIT`] a file.
    pub fn open(dir: &Path) -> Result<Bundle, AssetError> {
        let root = dir
            .canonicalize()
            .map_err(|e| AssetError::Io(format!("{}: {e}", dir.display())))?;
        if !root.is_dir() {
            return Err(AssetError::Io(format!(
                "{}: not a directory",
                dir.display()
            )));
        }
        Ok(Bundle {
            root,
            limit: DEFAULT_LIMIT,
        })
    }

    /// The same bundle with another per-file limit.
    pub fn with_limit(self, limit: u64) -> Bundle {
        Bundle { limit, ..self }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The asset at `key`, as `flutter/assets` and `ImmutableBuffer.fromAsset`
    /// ask for it: the key exactly as dart:ui sends it, already URI-encoded,
    /// so not decoded here (the engine's `DirectoryAssetBundle` does not
    /// decode either).
    pub fn load(&self, key: &str) -> Result<Vec<u8>, AssetError> {
        let path = self.resolve(key)?;
        read_limited(&path, key, self.limit)
    }

    /// The path `key` names, checked: relative, no `..`, and inside the
    /// bundle once resolved.
    fn resolve(&self, key: &str) -> Result<PathBuf, AssetError> {
        let bad = || AssetError::BadKey(key.to_owned());
        if key.is_empty() || key.contains('\0') {
            return Err(bad());
        }
        let rel = Path::new(key);
        if !rel
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(bad());
        }
        let joined = self.root.join(rel);
        let real = match joined.canonicalize() {
            Ok(p) => p,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(AssetError::NotFound(key.to_owned()))
            }
            Err(e) => return Err(AssetError::Io(format!("{key:?}: {e}"))),
        };
        // A symlink inside the bundle may point anywhere; its target must
        // still be in the bundle.
        if !real.starts_with(&self.root) {
            return Err(bad());
        }
        Ok(real)
    }

    /// The font families `FontManifest.json` declares, in order. No manifest
    /// is no fonts. Only names and asset keys: the bytes are read when a
    /// family is first used, not before `main`.
    pub fn fonts(&self) -> Result<Vec<FontFamily>, AssetError> {
        match self.load("FontManifest.json") {
            Ok(bytes) => parse_font_manifest(&bytes),
            Err(AssetError::NotFound(_)) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }
}

/// A regular file at `path`, up to `limit` bytes: what
/// `ImmutableBuffer.fromFilePath` reads. Any path, as `dart:io` already
/// allows the app; the regular-file and size rules still hold, so a device
/// or a FIFO is refused rather than read forever.
pub fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>, AssetError> {
    let shown = path.display().to_string();
    match std::fs::metadata(path) {
        Ok(_) => read_limited(path, &shown, limit),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(AssetError::NotFound(shown)),
        Err(e) => Err(AssetError::Io(format!("{shown}: {e}"))),
    }
}

/// Reads a regular file of at most `limit` bytes. The type is checked
/// before opening, so a FIFO is refused rather than blocking the open, and
/// again on the open file, with the size, so a file swapped in between is
/// not read unchecked. The size is enforced while reading too, so a file
/// that grows is still cut off at the limit.
fn read_limited(path: &Path, what: &str, limit: u64) -> Result<Vec<u8>, AssetError> {
    let io = |e: std::io::Error| AssetError::Io(format!("{what:?}: {e}"));
    let not_regular = || AssetError::NotRegular(what.to_owned());
    if !std::fs::metadata(path).map_err(io)?.is_file() {
        return Err(not_regular());
    }
    let file = File::open(path).map_err(io)?;
    let meta = file.metadata().map_err(io)?;
    if !meta.is_file() {
        return Err(not_regular());
    }
    let too_large = |size| AssetError::TooLarge {
        what: what.to_owned(),
        size,
        limit,
    };
    if meta.len() > limit {
        return Err(too_large(meta.len()));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve(usize::try_from(meta.len()).unwrap_or(usize::MAX))
        .map_err(|e| AssetError::Io(format!("{what:?}: {e}")))?;
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > limit {
        return Err(too_large(bytes.len() as u64));
    }
    Ok(bytes)
}

/// One family of `FontManifest.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFamily {
    pub family: String,
    pub fonts: Vec<FontAsset>,
}

/// One face of a family: its asset key and what the manifest says of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontAsset {
    pub asset: String,
    /// 100 to 900, as `pubspec.yaml` gives it.
    pub weight: Option<u16>,
    /// `"italic"`, or absent for normal.
    pub style: Option<String>,
}

/// `[{"family": …, "fonts": [{"asset": …, "weight": …, "style": …}]}]`, as
/// the flutter tool writes it.
pub fn parse_font_manifest(bytes: &[u8]) -> Result<Vec<FontFamily>, AssetError> {
    use serde_json::Value;
    let bad = |what: &str| AssetError::Manifest(what.to_owned());
    let v: Value =
        serde_json::from_slice(bytes).map_err(|e| AssetError::Manifest(e.to_string()))?;
    let families = v.as_array().ok_or_else(|| bad("not a list"))?;
    families
        .iter()
        .map(|f| {
            let family = f
                .get("family")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("a family without a name"))?
                .to_owned();
            let fonts = f
                .get("fonts")
                .and_then(Value::as_array)
                .ok_or_else(|| bad("a family without fonts"))?
                .iter()
                .map(|font| {
                    Ok(FontAsset {
                        asset: font
                            .get("asset")
                            .and_then(Value::as_str)
                            .ok_or_else(|| bad("a font without an asset"))?
                            .to_owned(),
                        weight: font
                            .get("weight")
                            .and_then(Value::as_u64)
                            .and_then(|w| u16::try_from(w).ok()),
                        style: font.get("style").and_then(Value::as_str).map(str::to_owned),
                    })
                })
                .collect::<Result<_, AssetError>>()?;
            Ok(FontFamily { family, fonts })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// A bundle in a fresh directory under the system temp dir.
    fn bundle(name: &str) -> (PathBuf, Bundle) {
        let dir = std::env::temp_dir().join(format!("fl-assets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("fonts")).unwrap();
        std::fs::write(dir.join("fonts/a.ttf"), b"font").unwrap();
        std::fs::write(dir.join("with space.txt"), b"spaced").unwrap();
        let b = Bundle::open(&dir).unwrap();
        (dir, b)
    }

    #[test]
    fn loads_keys_inside_the_bundle() {
        let (_d, b) = bundle("load");
        assert_eq!(b.load("fonts/a.ttf").unwrap(), b"font");
        assert_eq!(b.load("./fonts/a.ttf").unwrap(), b"font");
        assert_eq!(b.load("with space.txt").unwrap(), b"spaced");
        assert!(matches!(
            b.load("fonts/none.ttf"),
            Err(AssetError::NotFound(_))
        ));
    }

    #[test]
    fn keys_cannot_leave_the_bundle() {
        let (d, b) = bundle("escape");
        for key in [
            "",
            "/etc/passwd",
            "../outside",
            "fonts/../../outside",
            "fonts/a.ttf\0",
        ] {
            assert!(matches!(b.load(key), Err(AssetError::BadKey(_))), "{key:?}");
        }
        // A symlink inside that points outside.
        let outside = d.with_extension("outside");
        std::fs::write(&outside, b"secret").unwrap();
        std::os::unix::fs::symlink(&outside, d.join("link")).unwrap();
        assert!(matches!(b.load("link"), Err(AssetError::BadKey(_))));
        let _ = std::fs::remove_file(outside);
    }

    #[test]
    fn only_regular_files_within_the_limit() {
        let (d, b) = bundle("limits");
        assert!(matches!(b.load("fonts"), Err(AssetError::NotRegular(_))));
        std::fs::write(d.join("big"), vec![0u8; 100]).unwrap();
        let small = b.clone().with_limit(99);
        assert!(matches!(
            small.load("big"),
            Err(AssetError::TooLarge {
                size: 100,
                limit: 99,
                ..
            })
        ));
        assert_eq!(b.load("big").unwrap().len(), 100);
        // A device is not read, however small its claimed size.
        assert!(matches!(
            read_file(Path::new("/dev/zero"), 1 << 20),
            Err(AssetError::NotRegular(_))
        ));
    }

    #[test]
    fn reads_the_manifest_the_tool_writes() {
        let m =
            br#"[{"family":"MaterialIcons","fonts":[{"asset":"fonts/MaterialIcons-Regular.otf"}]},
            {"family":"Roboto","fonts":[{"asset":"fonts/Roboto-Bold.ttf","weight":700},
            {"asset":"fonts/Roboto-Italic.ttf","style":"italic"}]}]"#;
        let f = parse_font_manifest(m).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].family, "MaterialIcons");
        assert_eq!(f[1].fonts[0].weight, Some(700));
        assert_eq!(f[1].fonts[1].style.as_deref(), Some("italic"));
        assert!(parse_font_manifest(b"{}").is_err());
        assert!(parse_font_manifest(br#"[{"fonts":[]}]"#).is_err());
    }

    #[test]
    fn no_manifest_is_no_fonts() {
        let (_d, b) = bundle("nofonts");
        assert_eq!(b.fonts().unwrap(), Vec::new());
    }
}
