//! What an app's frames needed of the device's memory, kept in a file
//! between runs.
//!
//! A device takes its memory from the driver in blocks, and where the driver
//! backs a block with pages as it is made (a VisionFive 2's, a Pi 5's) the
//! size of the first is paid in full at the start. Too large is memory held
//! for nothing; too small is a second block, made in the frame that needs
//! it. What an app needs is not known before it has drawn: one with no
//! layers uses about nine of its target's frames and one with them fifteen
//! or more. So a run is sized by a guess, the most it allocated at once is
//! kept here, and the next run of that app at that size is sized by that.
//!
//! The file is lines of text, one for an app at a size, and is kept as the
//! pipelines' is: read only if it is a regular file no larger than such a
//! file could be, a line taken only if every field of it reads, written
//! beside itself and renamed. A line is only ever raised, so a run that
//! needed less does not make the next one short. Removing the file starts
//! again.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use emblema::Context;

/// The lines kept: the apps a machine runs at the sizes it runs them, the
/// oldest dropped past this.
const LINES: usize = 64;
/// The longest the file could be, and the most of it read.
const LIMIT: usize = LINES * 96;

/// Where a user's record is kept: `fl-engine/memory` beside the pipelines
/// ([`crate::compiled::default_path`]).
pub fn default_path() -> Option<PathBuf> {
    Some(crate::compiled::default_path()?.with_file_name("memory"))
}

/// Which app a line is of: its file's path, length and time of writing, so
/// that an app built again starts again. A number, there being nothing in a
/// path that could not be in a line.
pub fn key(app: &Path) -> u64 {
    let path = fs::canonicalize(app).unwrap_or_else(|_| app.to_owned());
    let (len, written) = fs::metadata(&path).map_or((0, 0), |m| {
        let written = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos() as u64);
        (m.len(), written)
    });
    // FNV-1a: the same number for the same app on every run, which the
    // standard library's hasher does not promise.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(path.as_os_str().as_encoded_bytes());
    feed(&len.to_le_bytes());
    feed(&written.to_le_bytes());
    hash
}

/// A file and the app whose line in it is this run's.
#[derive(Debug, Clone)]
pub struct Kept {
    pub path: PathBuf,
    pub key: u64,
}

/// One line: an app at a size, and the most it allocated at once, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Line {
    key: u64,
    width: u32,
    height: u32,
    need: u64,
}

impl Line {
    fn parse(text: &str) -> Option<Line> {
        let mut fields = text.split(' ');
        let line = Line {
            key: u64::from_str_radix(fields.next()?, 16).ok()?,
            width: fields.next()?.parse().ok()?,
            height: fields.next()?.parse().ok()?,
            need: fields.next()?.parse().ok()?,
        };
        fields.next().is_none().then_some(line)
    }

    fn is_of(&self, other: &Line) -> bool {
        (self.key, self.width, self.height) == (other.key, other.width, other.height)
    }
}

fn read(path: &Path) -> Vec<Line> {
    let text = (|| {
        let file = fs::File::open(path).ok()?;
        let meta = file.metadata().ok()?;
        if !meta.is_file() || meta.len() > LIMIT as u64 {
            return None;
        }
        let mut text = String::new();
        file.take(LIMIT as u64).read_to_string(&mut text).ok()?;
        Some(text)
    })();
    text.map_or_else(Vec::new, |t| {
        t.lines().filter_map(Line::parse).take(LINES).collect()
    })
}

/// What a device allocated at the most, as one number: the device's own
/// memory, and the host's up to a host block of it, which on a driver with
/// one kind of memory is in the same blocks.
pub(crate) fn need(ctx: &Context, host_block: u64) -> u64 {
    ctx.memory_peak().map_or(0, |peak| {
        peak.device.saturating_add(peak.host.min(host_block))
    })
}

/// A run's line: what the file had for it at the start, and where it goes.
pub(crate) struct Record {
    path: PathBuf,
    line: Line,
}

impl Record {
    /// The line of `kept`'s app at this size, 0 for its need if the file
    /// has none.
    pub(crate) fn load(kept: Kept, width: u32, height: u32) -> Record {
        let mut line = Line {
            key: kept.key,
            width,
            height,
            need: 0,
        };
        if let Some(had) = read(&kept.path).iter().find(|l| l.is_of(&line)) {
            line.need = had.need;
        }
        Record {
            path: kept.path,
            line,
        }
    }

    /// What a run before needed, if one is kept.
    pub(crate) fn remembered(&self) -> Option<u64> {
        (self.line.need > 0).then_some(self.line.need)
    }

    /// Whether this run has needed more than the file says.
    pub(crate) fn stale(&self, need: u64) -> bool {
        need > self.line.need
    }

    /// Raises the line to `need` and writes the file, this line first and
    /// the others as they were read now, not at the start: another app's
    /// run may have written since. A file that cannot be written is tried
    /// again when more is needed.
    pub(crate) fn save(&mut self, need: u64) {
        if !self.stale(need) {
            return;
        }
        self.line.need = need;
        let mut lines = vec![self.line];
        lines.extend(
            read(&self.path)
                .into_iter()
                .filter(|l| !l.is_of(&self.line)),
        );
        lines.truncate(LINES);
        let text: String = lines
            .iter()
            .map(|l| format!("{:016x} {} {} {}\n", l.key, l.width, l.height, l.need))
            .collect();
        let _ = crate::compiled::write(&self.path, text.as_bytes(), LIMIT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fl-raster-memory-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = fs::remove_file(&path);
        path
    }

    fn kept(path: &Path, key: u64) -> Kept {
        Kept {
            path: path.to_owned(),
            key,
        }
    }

    #[test]
    fn a_need_is_kept_for_its_app_at_its_size_and_only_raised() {
        let path = file("raised");
        let mut a = Record::load(kept(&path, 1), 1280, 720);
        assert_eq!(a.remembered(), None);
        a.save(40 << 20);
        assert_eq!(
            Record::load(kept(&path, 1), 1280, 720).remembered(),
            Some(40 << 20)
        );
        // Another size, another app: lines of their own.
        assert_eq!(Record::load(kept(&path, 1), 1920, 1080).remembered(), None);
        assert_eq!(Record::load(kept(&path, 2), 1280, 720).remembered(), None);
        // A run that needed less leaves it; one that needed more raises it.
        let mut again = Record::load(kept(&path, 1), 1280, 720);
        assert!(!again.stale(30 << 20));
        again.save(30 << 20);
        assert_eq!(
            Record::load(kept(&path, 1), 1280, 720).remembered(),
            Some(40 << 20)
        );
        again.save(50 << 20);
        assert_eq!(
            Record::load(kept(&path, 1), 1280, 720).remembered(),
            Some(50 << 20)
        );
    }

    #[test]
    fn another_app_s_line_written_meanwhile_is_kept() {
        let path = file("meanwhile");
        let mut a = Record::load(kept(&path, 1), 800, 600);
        let mut b = Record::load(kept(&path, 2), 800, 600);
        b.save(7);
        a.save(9);
        assert_eq!(Record::load(kept(&path, 1), 800, 600).remembered(), Some(9));
        assert_eq!(Record::load(kept(&path, 2), 800, 600).remembered(), Some(7));
    }

    #[test]
    fn the_file_holds_so_many_lines_the_newest() {
        let path = file("many");
        for key in 0..(LINES as u64 + 8) {
            Record::load(kept(&path, key), 1, 1).save(key + 1);
        }
        assert!(fs::metadata(&path).unwrap().len() <= LIMIT as u64);
        assert_eq!(read(&path).len(), LINES);
        assert_eq!(Record::load(kept(&path, 0), 1, 1).remembered(), None);
        let last = LINES as u64 + 7;
        assert_eq!(
            Record::load(kept(&path, last), 1, 1).remembered(),
            Some(last + 1)
        );
    }

    #[test]
    fn a_line_that_does_not_read_is_no_line() {
        for bad in [
            "",
            "1 2 3",
            "1 2 3 4 5",
            "zz 2 3 4",
            "1 -2 3 4",
            "1 2 3 4.5",
            "1 2 3 99999999999999999999999",
            "1  2 3 4",
        ] {
            assert_eq!(Line::parse(bad), None, "{bad:?}");
        }
        let path = file("bad");
        fs::write(&path, "garbage\n0000000000000001 4 5 6\n\u{0}\n").unwrap();
        assert_eq!(Record::load(kept(&path, 1), 4, 5).remembered(), Some(6));
        // A file too long to be one of these is not read at all.
        fs::write(&path, "0000000000000001 4 5 6\n".repeat(LINES * 8)).unwrap();
        assert_eq!(Record::load(kept(&path, 1), 4, 5).remembered(), None);
        // Nor is something that is not a file.
        assert_eq!(
            Record::load(kept(path.parent().unwrap(), 1), 4, 5).remembered(),
            None
        );
    }

    #[test]
    fn an_app_built_again_is_another_app() {
        let path = file("app");
        fs::write(&path, b"one").unwrap();
        let one = key(&path);
        assert_eq!(one, key(&path));
        fs::write(&path, b"another").unwrap();
        assert_ne!(one, key(&path));
        // One that is not there has a key all the same, its path's.
        let _ = fs::remove_file(&path);
        assert_eq!(key(&path), key(&path));
    }
}
