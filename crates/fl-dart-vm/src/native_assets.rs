//! Native assets: the libraries an app's build hooks made, found by the asset
//! id an `@Native` declaration names, as the engine finds them
//! (`InitDartFFIForIsolateGroup` and `NativeAssetsDlopen` in
//! `runtime/dart_isolate.cc`).
//!
//! The VM asks the isolate group's embedder to open an asset id; an id the
//! embedder does not know falls through to the VM's own lookup in the
//! process, as before. An id it knows and cannot open is an error that names
//! the library, not a missing symbol.

use std::collections::HashMap;
use std::ffi::{c_void, CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};

use fl_dart_sys as sys;

use crate::error::{cstr, malloc_string};

/// Where an isolate group's code assets are: each id's kind and path, as the
/// bundle's `NativeAssetsManifest.json` gives them for this platform, and the
/// directories a bare library name is looked for in.
///
/// The flutter tool writes `["absolute", "libfoo.so"]` for a Linux bundle:
/// a file name, which the engine opens with `dlopen` as it is, and which is
/// found because the engine's library carries `RUNPATH $ORIGIN` and sits in
/// the bundle's `lib/` beside it. Here the search is explicit: each of
/// `search` in order, then the loader's own search.
#[derive(Debug, Default, Clone)]
pub struct NativeAssets {
    assets: HashMap<String, Vec<String>>,
    search: Vec<PathBuf>,
}

impl NativeAssets {
    /// `assets` maps an asset id to its kind (`absolute`, `relative`,
    /// `system`, `process`, `executable`) and, for the first three, a path.
    /// A relative path, and a bare file name, are looked for in `search`.
    pub fn new(assets: HashMap<String, Vec<String>>, search: Vec<PathBuf>) -> NativeAssets {
        NativeAssets { assets, search }
    }

    /// No assets: every id falls through to the VM's process lookup.
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// Opens `id`'s library. `Ok(null)` is an id this does not map, or one
    /// mapped to the process (`RTLD_DEFAULT`, which is null): the VM then
    /// looks in the process, as the engine's does.
    fn open(&self, id: &str) -> Result<*mut c_void, String> {
        let Some(location) = self.assets.get(id) else {
            return Ok(std::ptr::null_mut());
        };
        let path = location.get(1).map(String::as_str);
        match (location.first().map(String::as_str), path) {
            (Some("absolute"), Some(p)) if p.contains('/') => open(Path::new(p)),
            (Some("absolute" | "relative"), Some(p)) => match self.find(p) {
                Some(found) => open(&found),
                // Not beside the app: the loader's search, as the engine's
                // `dlopen` of the bare name does.
                None => open(Path::new(p)),
            },
            (Some("system"), Some(p)) => open(Path::new(p)),
            (Some("process"), _) => Ok(std::ptr::null_mut()),
            (Some("executable"), _) => {
                // SAFETY: a null name opens the program itself.
                let h = unsafe { dlopen(std::ptr::null(), RTLD_LAZY) };
                if h.is_null() {
                    Err(format!("Failed to open the executable: {}", dl_error()))
                } else {
                    Ok(h)
                }
            }
            _ => Err(format!(
                "Asset '{id}' has a location this embedder does not know: {location:?}"
            )),
        }
    }

    /// The first of `search` that has `name`.
    fn find(&self, name: &str) -> Option<PathBuf> {
        self.search
            .iter()
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    }

    /// What the VM says is available when an id is not found.
    fn available(&self) -> String {
        if self.assets.is_empty() {
            return "No available native assets.".to_owned();
        }
        let mut ids: Vec<&str> = self.assets.keys().map(String::as_str).collect();
        ids.sort_unstable();
        format!("Available native assets: {}.", ids.join(", "))
    }
}

/// `dlopen(path, RTLD_LAZY)`, as the VM's `Utils::LoadDynamicLibrary` opens
/// a library.
fn open(path: &Path) -> Result<*mut c_void, String> {
    let shown = path.display();
    let c = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| format!("Failed to load dynamic library '{shown}': a NUL in its path"))?;
    // SAFETY: c is a valid C string.
    let h = unsafe { dlopen(c.as_ptr(), RTLD_LAZY) };
    if h.is_null() {
        Err(format!(
            "Failed to load dynamic library '{shown}': {}",
            dl_error()
        ))
    } else {
        Ok(h)
    }
}

fn dl_error() -> String {
    // SAFETY: dlerror returns null or a thread-local C string.
    unsafe { cstr(dlerror()) }
}

/// Gives the current isolate group the resolver. Called once per group,
/// right after it is made and before any of its code runs.
///
/// # Safety
/// An isolate of a group made by this crate must be current, with its
/// group data a `GroupData` (see `crate::isolate::group_native_assets`).
pub(crate) unsafe fn install() {
    let mut api = sys::NativeAssetsApi {
        dlopen_absolute: None,
        dlopen_relative: None,
        dlopen_system: None,
        dlopen_process: None,
        dlopen_executable: None,
        dlsym: Some(on_dlsym),
        dlclose: None,
        dlopen: Some(on_dlopen),
        available_assets: Some(on_available_assets),
        // Not v2: with it, an id this does not map is an error and the VM
        // stops looking, so an `@Native` with no asset, which the process
        // provides, would no longer resolve. The engine sets `dlopen` too.
        dlopen_v2: None,
    };
    // SAFETY: per this function's contract; the VM copies the struct.
    unsafe { sys::Dart_InitializeNativeAssetsResolver(&mut api) };
}

/// The current group's assets. The VM calls the callbacks below on a mutator
/// thread with its isolate set aside but its group current.
fn current() -> Option<&'static NativeAssets> {
    // SAFETY: the VM calls these callbacks with a group of this crate's
    // current (see `install`).
    unsafe { crate::isolate::group_native_assets() }
}

/// Writes `msg` to the VM's error out-pointer, which it frees with free().
///
/// # Safety
/// `error` is null or a valid out-pointer.
unsafe fn set_error(error: *mut *mut c_char, msg: &str) {
    if !error.is_null() {
        // SAFETY: per this function's contract.
        unsafe { *error = malloc_string(msg) };
    }
}

unsafe extern "C" fn on_dlopen(asset_id: *const c_char, error: *mut *mut c_char) -> *mut c_void {
    let r = std::panic::catch_unwind(|| {
        // SAFETY: the VM passes the id as a NUL-terminated string.
        let id = unsafe { CStr::from_ptr(asset_id) }.to_string_lossy();
        match current() {
            Some(assets) => assets.open(&id),
            None => Ok(std::ptr::null_mut()),
        }
    });
    match r.unwrap_or_else(|_| Err("panicked opening a native asset".to_owned())) {
        Ok(h) => h,
        Err(msg) => {
            // SAFETY: the VM's out-pointer.
            unsafe { set_error(error, &msg) };
            std::ptr::null_mut()
        }
    }
}

unsafe extern "C" fn on_dlsym(
    handle: *mut c_void,
    symbol: *const c_char,
    error: *mut *mut c_char,
) -> *mut c_void {
    // SAFETY: a handle on_dlopen returned and a NUL-terminated name; dlerror
    // is cleared first so a stale message is not taken for this lookup's.
    unsafe {
        dlerror();
        let p = dlsym(handle, symbol);
        let e = dlerror();
        if !e.is_null() {
            let msg = format!("Failed to lookup symbol '{}': {}", cstr(symbol), cstr(e));
            set_error(error, &msg);
        }
        p
    }
}

unsafe extern "C" fn on_available_assets() -> *mut c_char {
    let text = std::panic::catch_unwind(|| {
        current().map_or_else(
            || "No available native assets.".to_owned(),
            NativeAssets::available,
        )
    })
    .unwrap_or_default();
    malloc_string(&text)
}

const RTLD_LAZY: c_int = 1;

#[link(name = "dl")]
extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *mut c_char;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn assets(entries: &[(&str, &[&str])], search: Vec<PathBuf>) -> NativeAssets {
        NativeAssets::new(
            entries
                .iter()
                .map(|(id, loc)| {
                    (
                        (*id).to_owned(),
                        loc.iter().map(|s| (*s).to_owned()).collect(),
                    )
                })
                .collect(),
            search,
        )
    }

    #[test]
    fn an_unmapped_id_falls_through_to_the_process() {
        let a = assets(&[], vec![]);
        assert!(a.open("package:p/q.dart").unwrap().is_null());
        assert_eq!(a.available(), "No available native assets.");
    }

    #[test]
    fn a_bare_name_is_found_in_the_search_directories_first() {
        let dir = std::env::temp_dir().join(format!("fl-native-assets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Not a library: the open fails, and its message names the file it
        // found in the directory, not the bare name.
        std::fs::write(dir.join("libnot.so"), b"not an ELF").unwrap();
        let a = assets(
            &[("package:p/q.dart", &["absolute", "libnot.so"])],
            vec![PathBuf::from("/nonexistent"), dir.clone()],
        );
        let e = a.open("package:p/q.dart").unwrap_err();
        assert!(
            e.contains(&dir.join("libnot.so").display().to_string()),
            "{e}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_library_that_is_not_there_is_an_error_naming_it() {
        let a = assets(
            &[("package:p/q.dart", &["absolute", "libfl_no_such_lib.so"])],
            vec![],
        );
        let e = a.open("package:p/q.dart").unwrap_err();
        assert!(e.contains("libfl_no_such_lib.so"), "{e}");
        assert_eq!(a.available(), "Available native assets: package:p/q.dart.");
    }

    #[test]
    fn a_system_library_opens_and_resolves() {
        let a = assets(&[("package:p/q.dart", &["system", "libm.so.6"])], vec![]);
        let h = a.open("package:p/q.dart").unwrap();
        assert!(!h.is_null());
        let mut err: *mut c_char = std::ptr::null_mut();
        // SAFETY: a live handle and a NUL-terminated name.
        let p = unsafe { on_dlsym(h, c"cos".as_ptr(), &mut err) };
        assert!(!p.is_null() && err.is_null());
        // SAFETY: as above.
        let p = unsafe { on_dlsym(h, c"fl_no_such_symbol".as_ptr(), &mut err) };
        assert!(p.is_null() && !err.is_null());
        // SAFETY: a malloc'd message we own.
        let msg = unsafe { crate::error::take_c_error(err) };
        assert!(
            msg.starts_with("Failed to lookup symbol 'fl_no_such_symbol'"),
            "{msg}"
        );
    }

    #[test]
    fn process_and_executable() {
        let a = assets(
            &[
                ("package:p/a.dart", &["process"]),
                ("package:p/b.dart", &["executable"]),
            ],
            vec![],
        );
        assert!(a.open("package:p/a.dart").unwrap().is_null());
        assert!(!a.open("package:p/b.dart").unwrap().is_null());
        let bad = assets(&[("package:p/c.dart", &["elsewhere", "x"])], vec![]);
        assert!(bad.open("package:p/c.dart").is_err());
    }
}
