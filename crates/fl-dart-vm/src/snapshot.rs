//! Program inputs: kernel dills (JIT) and AOT snapshots, and the version checks
//! that run before either reaches the VM.
//!
//! A snapshot is valid only for the exact VM build that wrote it, and the VM's
//! own refusal is easy to miss as that -- or, for a dill, can arrive as a
//! confusing compile error far into loading. Both formats carry their producer
//! in a fixed header, so it is read here first and a mismatch becomes
//! [`DartError::SnapshotVersionMismatch`] naming both sides.

#[cfg(feature = "aot")]
use std::ffi::{c_void, CStr, CString};
#[cfg(feature = "aot")]
use std::os::raw::{c_char, c_int};
#[cfg(feature = "aot")]
use std::path::Path;

use fl_dart_sys as sys;

use crate::error::DartError;

const KERNEL_MAGIC: u32 = 0x90AB_CDEF;
/// `Snapshot::kMagicValue` in `runtime/vm/snapshot.h`, stored little-endian.
const SNAPSHOT_MAGIC: u32 = 0xDCDC_F5F5;
/// Magic, then a 64-bit length, then a 64-bit kind: where the version hash starts.
const SNAPSHOT_HASH_OFFSET: usize = 4 + 8 + 8;
const SNAPSHOT_HASH_LEN: usize = 32;
/// The kernel header's SDK hash: the first ten characters of the producing
/// SDK's git revision. A dill compiled without one (`0000000000`, which is
/// what the platform dills ship with) is accepted by any VM.
const KERNEL_SDK_HASH_LEN: usize = 10;
const KERNEL_NO_SDK_HASH: &[u8] = b"0000000000";

/// Checks a kernel dill's header against the linked VM.
pub fn check_kernel(dill: &[u8]) -> Result<(), DartError> {
    let word = |at: usize| {
        dill.get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    if word(0) != Some(KERNEL_MAGIC) {
        return Err(DartError::Compilation(
            "not a kernel binary (bad magic)".to_owned(),
        ));
    }
    let format = word(4).unwrap_or(0);
    let hash = dill.get(8..8 + KERNEL_SDK_HASH_LEN).unwrap_or_default();
    let want_hash = &sys::DART_SDK_REVISION.as_bytes()[..KERNEL_SDK_HASH_LEN];
    if format != sys::DART_KERNEL_FORMAT_VERSION
        || (hash != want_hash && hash != KERNEL_NO_SDK_HASH)
    {
        return Err(DartError::SnapshotVersionMismatch {
            expected: format!(
                "kernel format {} from Dart {} ({})",
                sys::DART_KERNEL_FORMAT_VERSION,
                sys::DART_SDK_VERSION,
                String::from_utf8_lossy(want_hash)
            ),
            found: format!("kernel format {format} ({})", String::from_utf8_lossy(hash)),
        });
    }
    Ok(())
}

/// Checks an AOT snapshot's data section against the linked VM.
pub fn check_aot_header(data: &[u8]) -> Result<(), DartError> {
    let magic = data
        .get(0..4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if magic != Some(SNAPSHOT_MAGIC) {
        return Err(DartError::Compilation(
            "not a Dart snapshot (bad magic)".to_owned(),
        ));
    }
    let hash = data
        .get(SNAPSHOT_HASH_OFFSET..SNAPSHOT_HASH_OFFSET + SNAPSHOT_HASH_LEN)
        .unwrap_or_default();
    if hash != sys::DART_SNAPSHOT_HASH.as_bytes() {
        return Err(DartError::SnapshotVersionMismatch {
            expected: format!(
                "snapshot {} (Dart {})",
                sys::DART_SNAPSHOT_HASH,
                sys::DART_SDK_VERSION
            ),
            found: format!("snapshot {}", String::from_utf8_lossy(hash)),
        });
    }
    Ok(())
}

/// Recognizes the VM's own wording for a version mismatch, for the cases the
/// header checks above do not see first (a snapshot handed over as bytes).
pub(crate) fn mismatch_from_message(msg: &str) -> Option<DartError> {
    // "Wrong full snapshot version, expected '<hash>' found '<hash>'"
    let rest = msg.split_once("snapshot version, expected '")?.1;
    let (expected, rest) = rest.split_once('\'')?;
    let found = rest.split_once("found '")?.1.split_once('\'')?.0;
    Some(DartError::SnapshotVersionMismatch {
        expected: expected.to_owned(),
        found: found.to_owned(),
    })
}

/// An AOT snapshot loaded with `dlopen`, the way the Flutter engine loads the
/// `libapp.so` its tooling builds (`DartSnapshot`'s `NativeLibrary` +
/// `SymbolMapping` path in `runtime/dart_snapshot.cc`).
///
/// Since Dart 3.13 a snapshot is one data section and one text section,
/// `_kDartSnapshotData` and `_kDartSnapshotText`; the separate VM and isolate
/// snapshots older embedders loaded four symbols for are gone, as is any
/// snapshot argument to `Dart_Initialize`. Flutter's `libapp.so` and the ELF
/// `dart compile aot-snapshot` writes have this same shape and both load this
/// way. (Running one also needs the VM started with `--precompilation`, which
/// `Vm::init` passes in AOT builds; without it the VM treats AOT code as JIT
/// code and crashes at the first native call.)
///
/// The text section is executable code mapped by the dynamic loader; it has to
/// stay mapped for as long as anything created from it can run, so the isolate
/// group holds an `Arc` of this and the library is closed only after the
/// group's cleanup.
#[cfg(feature = "aot")]
#[derive(Debug)]
pub struct AotSnapshot {
    handle: *mut c_void,
    data: *const u8,
    text: *const u8,
}

#[cfg(feature = "aot")]
// SAFETY: the handle is only dlclose'd in Drop and the two pointers are
// read-only mapped sections; nothing here is tied to a thread.
unsafe impl Send for AotSnapshot {}
#[cfg(feature = "aot")]
// SAFETY: as above -- no interior mutation.
unsafe impl Sync for AotSnapshot {}

#[cfg(feature = "aot")]
impl AotSnapshot {
    /// `dlopen`s an AOT ELF snapshot -- a `libapp.so` from Flutter's tooling,
    /// or `dart compile aot-snapshot` output -- and checks its version.
    ///
    /// A snapshot is machine code, and `dlopen` runs its ELF initializers, so
    /// opening one runs it: only open snapshots from a source you would run a
    /// binary from. The version check below happens after the library is
    /// mapped, so it protects against mismatched builds, not hostile files.
    pub fn open(path: &Path) -> Result<AotSnapshot, DartError> {
        // dlopen reads a name without a '/' as a library to search for on
        // the loader path, not a file in the current directory, so
        // `fl-dart run-aot hello.aot` failed. An absolute path is always
        // opened as given; canonicalizing also turns a missing file into an
        // error that says so.
        let abs = std::fs::canonicalize(path)
            .map_err(|e| DartError::Api(format!("{}: {e}", path.display())))?;
        let c = CString::new(abs.as_os_str().as_encoded_bytes())
            .map_err(|_| DartError::Api(format!("NUL in path {}", path.display())))?;
        // SAFETY: c is a valid C string; RTLD_LOCAL keeps the snapshot's
        // symbols from resolving anything else in the process.
        let handle = unsafe { dlopen(c.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        if handle.is_null() {
            return Err(DartError::Api(format!(
                "dlopen {}: {}",
                path.display(),
                dl_error()
            )));
        }
        // The struct is built once, after both lookups. An earlier version
        // built a placeholder to look symbols up through and then replaced
        // it; the raw handle is `Copy`, so the placeholder's `Drop` dlclose'd
        // the library at the end of this function and the VM read zeros.
        let lookup = |name: &CStr| match symbol(handle, name) {
            Ok(p) => Ok(p),
            Err(e) => {
                // SAFETY: handle came from dlopen above and nothing else
                // holds it yet.
                unsafe { dlclose(handle) };
                Err(e)
            }
        };
        let data = lookup(c"_kDartSnapshotData")?;
        let text = lookup(c"_kDartSnapshotText")?;
        let snap = AotSnapshot { handle, data, text };
        // SAFETY: data points at a mapped snapshot section, which starts with
        // at least a full header (magic, length, kind, hash) when it is one;
        // the magic check fails first on anything that is not.
        let header = unsafe {
            std::slice::from_raw_parts(snap.data, SNAPSHOT_HASH_OFFSET + SNAPSHOT_HASH_LEN)
        };
        check_aot_header(header)?;
        Ok(snap)
    }

    pub(crate) fn data(&self) -> *const u8 {
        self.data
    }

    pub(crate) fn text(&self) -> *const u8 {
        self.text
    }
}

#[cfg(feature = "aot")]
impl Drop for AotSnapshot {
    fn drop(&mut self) {
        // SAFETY: handle came from dlopen and is closed exactly once.
        unsafe { dlclose(self.handle) };
    }
}

/// `dlsym` on `handle`. A free function rather than a method, so it needs no
/// `AotSnapshot` to exist yet (see `open`).
#[cfg(feature = "aot")]
fn symbol(handle: *mut c_void, name: &CStr) -> Result<*const u8, DartError> {
    // SAFETY: handle is a live dlopen handle; name is NUL-terminated.
    let p = unsafe { dlsym(handle, name.as_ptr()) };
    if p.is_null() {
        Err(DartError::Api(format!(
            "snapshot has no symbol {}: {}",
            name.to_string_lossy(),
            dl_error()
        )))
    } else {
        Ok(p.cast())
    }
}

#[cfg(feature = "aot")]
fn dl_error() -> String {
    // SAFETY: dlerror returns null or a thread-local C string.
    unsafe { crate::error::cstr(dlerror()) }
}

#[cfg(feature = "aot")]
const RTLD_NOW: c_int = 2;
#[cfg(feature = "aot")]
const RTLD_LOCAL: c_int = 0;

#[cfg(feature = "aot")]
#[link(name = "dl")]
extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *mut c_char;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dill_header(format: u32, hash: &[u8]) -> Vec<u8> {
        let mut v = KERNEL_MAGIC.to_be_bytes().to_vec();
        v.extend_from_slice(&format.to_be_bytes());
        v.extend_from_slice(hash);
        v
    }

    #[test]
    fn kernel_from_pinned_sdk_is_accepted() {
        let h = dill_header(
            sys::DART_KERNEL_FORMAT_VERSION,
            &sys::DART_SDK_REVISION.as_bytes()[..10],
        );
        assert_eq!(check_kernel(&h), Ok(()));
    }

    #[test]
    fn kernel_without_sdk_hash_is_accepted() {
        let h = dill_header(sys::DART_KERNEL_FORMAT_VERSION, b"0000000000");
        assert_eq!(check_kernel(&h), Ok(()));
    }

    #[test]
    fn kernel_from_other_sdk_names_both() {
        let h = dill_header(sys::DART_KERNEL_FORMAT_VERSION, b"deadbeef00");
        match check_kernel(&h) {
            Err(DartError::SnapshotVersionMismatch { expected, found }) => {
                assert!(expected.contains("416e33b0ff"), "{expected}");
                assert!(found.contains("deadbeef00"), "{found}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn kernel_format_mismatch_is_a_version_mismatch() {
        let h = dill_header(137, &sys::DART_SDK_REVISION.as_bytes()[..10]);
        assert!(matches!(
            check_kernel(&h),
            Err(DartError::SnapshotVersionMismatch { .. })
        ));
    }

    #[test]
    fn non_kernel_is_not_a_version_mismatch() {
        assert!(matches!(
            check_kernel(b"\x7fELF...."),
            Err(DartError::Compilation(_))
        ));
    }

    fn aot_header(hash: &str) -> Vec<u8> {
        let mut v = SNAPSHOT_MAGIC.to_le_bytes().to_vec();
        v.extend_from_slice(&[0; 16]);
        v.extend_from_slice(hash.as_bytes());
        v
    }

    #[test]
    fn aot_from_pinned_sdk_is_accepted() {
        assert_eq!(
            check_aot_header(&aot_header(sys::DART_SNAPSHOT_HASH)),
            Ok(())
        );
    }

    #[test]
    fn aot_from_other_sdk_names_both() {
        let other = "ffffffffffffffffffffffffffffffff";
        match check_aot_header(&aot_header(other)) {
            Err(DartError::SnapshotVersionMismatch { expected, found }) => {
                assert!(expected.contains(sys::DART_SNAPSHOT_HASH));
                assert!(found.contains(other));
            }
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn vm_mismatch_message_is_parsed() {
        let m = "Wrong full snapshot version, expected 'aaa' found 'bbb'";
        assert_eq!(
            mismatch_from_message(m),
            Some(DartError::SnapshotVersionMismatch {
                expected: "aaa".into(),
                found: "bbb".into()
            })
        );
        assert_eq!(mismatch_from_message("some other error"), None);
    }
}
