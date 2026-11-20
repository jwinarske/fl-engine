//! The pipelines the driver compiled, kept in a file between runs.
//!
//! A pipeline built for the first time is compiled by the driver, and where
//! the driver keeps nothing itself (IMG's on a VisionFive 2, where each is
//! most of a second) every run compiles again, in the frame that first draws
//! with it. emblema builds its pipelines through a cache it can hand over as
//! bytes and take back; this is the file they are kept in.
//!
//! The file is the driver's own bytes, checked by emblema to be this
//! device's and this driver build's before the driver sees them. It is
//! read only if it is a regular file of a size a cache could be, and written
//! beside itself and renamed, so a run that is killed leaves the old one.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use emblema::vulkan::PIPELINE_CACHE_LIMIT;
use emblema::Context;

/// Where a user's pipelines are kept: `fl-engine/pipelines` under
/// `$XDG_CACHE_HOME`, or under `$HOME/.cache`. None with neither.
pub fn default_path() -> Option<PathBuf> {
    let absolute = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    let cache =
        absolute("XDG_CACHE_HOME").or_else(|| absolute("HOME").map(|h| h.join(".cache")))?;
    Some(cache.join("fl-engine").join("pipelines"))
}

/// A device's pipelines and the file they are kept in.
pub(crate) struct Compiled {
    path: PathBuf,
    /// How many pipelines the context had when the file was last as it is.
    kept: usize,
    /// Whether the file's bytes were taken at the start.
    pub(crate) loaded: bool,
}

fn count(ctx: &Context) -> usize {
    match ctx {
        Context::Vulkan(vk) => vk.pipeline_count(),
        #[allow(unreachable_patterns)]
        _ => 0,
    }
}

fn read(path: &Path) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > PIPELINE_CACHE_LIMIT as u64 {
        return None;
    }
    let mut data = Vec::new();
    file.take(PIPELINE_CACHE_LIMIT as u64)
        .read_to_end(&mut data)
        .ok()?;
    Some(data)
}

impl Compiled {
    /// Hands the file's bytes to the device, if it has any that are its own.
    /// No file, or one of another driver, is a first run: nothing is wrong.
    pub(crate) fn load(ctx: &mut Context, path: PathBuf) -> Compiled {
        let loaded = match (read(&path), &mut *ctx) {
            (Some(data), Context::Vulkan(vk)) => vk.load_pipeline_cache(&data).unwrap_or(false),
            #[allow(unreachable_patterns)]
            _ => false,
        };
        Compiled {
            path,
            kept: count(ctx),
            loaded,
        }
    }

    /// Whether a pipeline has been built since the file was written.
    pub(crate) fn stale(&self, ctx: &Context) -> bool {
        count(ctx) != self.kept
    }

    /// Writes the file if a pipeline has been built since it was. A file
    /// that cannot be written is tried again when the next is built: the run
    /// goes on without it.
    pub(crate) fn save(&mut self, ctx: &Context) {
        if !self.stale(ctx) {
            return;
        }
        self.kept = count(ctx);
        let data = match ctx {
            Context::Vulkan(vk) => vk.pipeline_cache_data(),
            #[allow(unreachable_patterns)]
            _ => return,
        };
        if let Ok(data) = data {
            let _ = write(&self.path, &data, PIPELINE_CACHE_LIMIT);
        }
    }
}

/// `data` to `path`, by a file beside it renamed over it: readable by its
/// owner alone, in a directory made the same way if there is none. Nothing
/// is written of nothing, or of more than `limit`.
pub(crate) fn write(path: &Path, data: &[u8], limit: usize) -> io::Result<()> {
    if data.is_empty() || data.len() > limit {
        return Ok(());
    }
    let dir = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let mut beside = path.as_os_str().to_owned();
    beside.push(format!(".{}", std::process::id()));
    let beside = PathBuf::from(beside);
    // Not through a link somebody left there: made new, or not at all.
    let _ = fs::remove_file(&beside);
    let done = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&beside)
        .and_then(|mut file| file.write_all(data))
        .and_then(|()| fs::rename(&beside, path));
    if done.is_err() {
        let _ = fs::remove_file(&beside);
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_written_whole_and_read_back() {
        let dir = std::env::temp_dir().join(format!("fl-compiled-{}", std::process::id()));
        let path = dir.join("deep").join("pipelines");
        write(&path, b"first", PIPELINE_CACHE_LIMIT).expect("write");
        assert_eq!(read(&path).as_deref(), Some(&b"first"[..]));
        write(&path, b"second, and longer", PIPELINE_CACHE_LIMIT).expect("write over");
        assert_eq!(read(&path).as_deref(), Some(&b"second, and longer"[..]));
        // Nothing is left beside it, and nothing empty replaces it.
        write(&path, b"", PIPELINE_CACHE_LIMIT).expect("nothing");
        assert_eq!(read(&path).as_deref(), Some(&b"second, and longer"[..]));
        let left: Vec<_> = fs::read_dir(path.parent().expect("parent"))
            .expect("dir")
            .collect();
        assert_eq!(left.len(), 1);
        // A directory in its place is not read, and no file is no file.
        assert!(read(&dir).is_none());
        assert!(read(&dir.join("none")).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
