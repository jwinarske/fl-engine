//! Isolates, the scopes that enter them, and the handles those scopes own.
//!
//! The embedding API has three rules that C++ embedders keep by convention and
//! that crash when broken. Each is a type here:
//!
//! - **Thread affinity.** An isolate is entered by one thread at a time.
//!   [`Isolate`] is neither `Send` nor `Sync`, and entering it takes `&mut`.
//! - **Enter/exit pairing.** [`Isolate::enter`] returns an [`IsolateScope`]
//!   that exits on drop, and borrows the isolate so it cannot outlive it.
//! - **Handle lifetime.** A `Dart_Handle` is valid until the API scope it was
//!   made in exits. [`Handle<'s>`] borrows its [`HandleScope<'s>`], and
//!   [`IsolateScope::with`] takes a closure generic over `'s`, so no handle
//!   can be returned from or stored outside the scope that made it.
//!
//! `tests/compile_fail` holds a case for each that must not compile.

use std::ffi::{c_void, CString};
use std::marker::PhantomData;
use std::os::raw::c_char;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use fl_dart_sys as sys;

use crate::error::{check, string_value, take_c_error, DartError};
use crate::vm::Vm;

/// Where `print()` goes.
pub type PrintSink = Arc<dyn Fn(&str) + Send + Sync>;

/// How to create an isolate.
#[derive(Clone)]
pub struct IsolateConfig {
    /// The root script's URI, as Dart code sees it in stack traces.
    pub script_uri: String,
    /// The isolate's debug name.
    pub name: String,
    /// Receives each line `print()` writes. Defaults to the process's stdout.
    pub print: PrintSink,
}

impl Default for IsolateConfig {
    fn default() -> Self {
        IsolateConfig {
            script_uri: "main.dart".to_owned(),
            name: "main".to_owned(),
            print: Arc::new(|line| println!("{line}")),
        }
    }
}

impl std::fmt::Debug for IsolateConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IsolateConfig")
            .field("script_uri", &self.script_uri)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// What an isolate group needs kept alive for as long as the VM may run its
/// code: the kernel it was loaded from (the VM reads it lazily) or the mapped
/// AOT snapshot (the VM executes it). Released by the VM's group cleanup
/// callback, which is the first moment the VM promises it is done with them.
struct GroupData {
    #[cfg(feature = "jit")]
    _kernel: Box<[u8]>,
    #[cfg(feature = "aot")]
    _aot: Arc<crate::AotSnapshot>,
}

/// Per-isolate state the callbacks into Rust need.
struct IsolateData {
    print: PrintSink,
}

/// An isolate created by this process. Not `Send`, not `Sync`: entering binds
/// it to the calling thread, and the type is what keeps it to one thread.
pub struct Isolate<'vm> {
    raw: sys::Dart_Isolate,
    shut_down: bool,
    _vm: PhantomData<&'vm Vm>,
    _not_send: PhantomData<*const ()>,
}

impl std::fmt::Debug for Isolate<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Isolate").field("raw", &self.raw).finish()
    }
}

impl Vm {
    /// Creates an isolate group from a kernel dill, as `dart compile kernel`
    /// writes it (with the platform libraries linked in, which is that
    /// command's default).
    #[cfg(feature = "jit")]
    pub fn isolate_from_kernel(
        &self,
        dill: &[u8],
        cfg: IsolateConfig,
    ) -> Result<Isolate<'_>, DartError> {
        crate::snapshot::check_kernel(dill)?;
        let kernel: Box<[u8]> = dill.into();
        let (ptr, len) = (kernel.as_ptr(), kernel.len());
        let group = GroupData { _kernel: kernel };
        // SAFETY: the kernel bytes live in `group`, which the VM keeps until it
        // calls cleanup_group.
        unsafe {
            self.create(
                group,
                cfg,
                |uri, name, flags, gd, id, err| {
                    sys::Dart_CreateIsolateGroupFromKernel(
                        uri,
                        name,
                        ptr,
                        len as isize,
                        flags,
                        gd,
                        id,
                        err,
                    )
                },
                |_| check(sys::Dart_LoadScriptFromKernel(ptr, len as isize)).map(drop),
            )
        }
    }

    /// Creates an isolate group from an AOT snapshot.
    #[cfg(feature = "aot")]
    pub fn isolate_from_aot(
        &self,
        snapshot: &Arc<crate::AotSnapshot>,
        cfg: IsolateConfig,
    ) -> Result<Isolate<'_>, DartError> {
        let (data, text) = (snapshot.data(), snapshot.text());
        let group = GroupData {
            _aot: Arc::clone(snapshot),
        };
        // SAFETY: the snapshot stays mapped while `group` holds it.
        unsafe {
            self.create(
                group,
                cfg,
                |uri, name, flags, gd, id, err| {
                    sys::Dart_CreateIsolateGroup(uri, name, data, text, flags, gd, id, err)
                },
                |_| Ok(()),
            )
        }
    }

    /// The part of isolate creation both modes share.
    ///
    /// Ownership of the group and isolate data passes to the VM only once a
    /// group exists, and on some failure paths the VM shuts the half-made
    /// isolate down itself -- running our cleanup callbacks -- and on others it
    /// does not. Both are held as `Arc`s with this function keeping one
    /// reference throughout, so afterwards the count says which happened and
    /// nothing is freed twice or leaked.
    ///
    /// # Safety
    /// `create_raw` calls a `Dart_CreateIsolateGroup*` function with the
    /// arguments it is given; `load` may call API functions that need a
    /// current isolate with an open scope.
    unsafe fn create(
        &self,
        group: GroupData,
        cfg: IsolateConfig,
        create_raw: impl FnOnce(
            *const c_char,
            *const c_char,
            *mut sys::Dart_IsolateFlags,
            *mut c_void,
            *mut c_void,
            *mut *mut c_char,
        ) -> sys::Dart_Isolate,
        load: impl FnOnce(()) -> Result<(), DartError>,
    ) -> Result<Isolate<'_>, DartError> {
        let uri = cstring(&cfg.script_uri)?;
        let name = cstring(&cfg.name)?;
        let group = Arc::new(group);
        let data = Arc::new(IsolateData {
            print: Arc::clone(&cfg.print),
        });
        let gd = Arc::into_raw(Arc::clone(&group)) as *mut c_void;
        let id = Arc::into_raw(Arc::clone(&data)) as *mut c_void;

        // SAFETY: an all-zero struct is a valid argument to the initializer.
        let mut flags: sys::Dart_IsolateFlags = unsafe { std::mem::zeroed() };
        // SAFETY: flags is a valid out-pointer.
        unsafe { sys::Dart_IsolateFlagsInitialize(&mut flags) };

        if !no_current_isolate() {
            // SAFETY: gd/id came from Arc::into_raw above and the VM never saw them.
            unsafe { reclaim(gd, id) };
            return Err(DartError::Api(
                "an isolate is already entered on this thread".into(),
            ));
        }

        let mut err: *mut c_char = std::ptr::null_mut();
        let raw = create_raw(uri.as_ptr(), name.as_ptr(), &mut flags, gd, id, &mut err);
        if raw.is_null() {
            // SAFETY: a malloc'd message we now own.
            let msg = unsafe { take_c_error(err) };
            release_if_vm_did_not(&group, &data, gd, id);
            return Err(
                crate::snapshot::mismatch_from_message(&msg).unwrap_or(DartError::Compilation(msg))
            );
        }

        // The new isolate is current, with no scope open.
        // SAFETY: there is a current isolate (just created) for the whole block.
        let setup = unsafe {
            sys::Dart_EnterScope();
            let r = load(()).and_then(|()| install_hooks());
            sys::Dart_ExitScope();
            r
        };
        if let Err(e) = setup {
            // SAFETY: raw is current; shutting it down runs our cleanup
            // callbacks, which release the VM's references to gd and id.
            unsafe { sys::Dart_ShutdownIsolate() };
            release_if_vm_did_not(&group, &data, gd, id);
            return Err(e);
        }
        // SAFETY: raw is current and has no open scope.
        unsafe { sys::Dart_ExitIsolate() };
        // SAFETY: raw is a live isolate that is no longer current.
        let err = unsafe { sys::Dart_IsolateMakeRunnable(raw) };
        if !err.is_null() {
            // SAFETY: a malloc'd message we now own; raw is entered only to be
            // shut down.
            let msg = unsafe { take_c_error(err) };
            unsafe {
                sys::Dart_EnterIsolate(raw);
                sys::Dart_ShutdownIsolate();
            }
            release_if_vm_did_not(&group, &data, gd, id);
            return Err(DartError::Api(msg));
        }
        Ok(Isolate {
            raw,
            shut_down: false,
            _vm: PhantomData,
            _not_send: PhantomData,
        })
    }
}

/// After a failed creation: if the VM never took ownership of the data it was
/// offered, release the references it would have released.
fn release_if_vm_did_not(
    group: &Arc<GroupData>,
    data: &Arc<IsolateData>,
    gd: *mut c_void,
    id: *mut c_void,
) {
    // Two references each means ours plus the one handed to the VM, which it
    // has therefore not given back.
    if Arc::strong_count(group) == 2 {
        // SAFETY: gd is the reference Arc::into_raw produced and the VM has not
        // released; it never will, since no group exists.
        drop(unsafe { Arc::from_raw(gd as *const GroupData) });
    }
    if Arc::strong_count(data) == 2 {
        // SAFETY: as above, for the isolate data.
        drop(unsafe { Arc::from_raw(id as *const IsolateData) });
    }
}

/// # Safety
/// Both pointers came from `Arc::into_raw` and were never given to the VM.
unsafe fn reclaim(gd: *mut c_void, id: *mut c_void) {
    // SAFETY: per this function's contract.
    unsafe {
        drop(Arc::from_raw(gd as *const GroupData));
        drop(Arc::from_raw(id as *const IsolateData));
    }
}

fn cstring(s: &str) -> Result<CString, DartError> {
    CString::new(s).map_err(|_| DartError::Api(format!("NUL in {s:?}")))
}

fn no_current_isolate() -> bool {
    // SAFETY: callable at any time once the VM is initialized.
    unsafe { sys::Dart_CurrentIsolate().is_null() }
}

impl<'vm> Isolate<'vm> {
    /// Enters the isolate on the current thread until the returned scope drops.
    ///
    /// Fails if another isolate is already entered on this thread: the type
    /// system keeps any one isolate to one thread, but cannot see that a
    /// thread is already inside a different one.
    pub fn enter(&mut self) -> Result<IsolateScope<'_>, DartError> {
        if !no_current_isolate() {
            return Err(DartError::Api(
                "an isolate is already entered on this thread".into(),
            ));
        }
        // SAFETY: raw is live (not shut down: shutdown consumes self), no
        // isolate is current on this thread, and `&mut self` means no other
        // scope of ours has it entered.
        unsafe { sys::Dart_EnterIsolate(self.raw) };
        Ok(IsolateScope {
            raw: self.raw,
            _isolate: PhantomData,
            _not_send: PhantomData,
        })
    }

    /// Shuts the isolate down. Dropping an `Isolate` does the same; this is
    /// the spelling that says so.
    pub fn shutdown(mut self) {
        self.shutdown_inner();
    }

    fn shutdown_inner(&mut self) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        if !no_current_isolate() {
            // Only reachable by dropping one isolate inside another's scope on
            // the same thread. Shutting this one down would mean leaving that
            // one mid-scope; leaking it is the lesser harm, and it is reported.
            eprintln!(
                "fl-dart-vm: isolate dropped while another is entered on this thread; leaked"
            );
            return;
        }
        // SAFETY: raw is live and nothing is current on this thread.
        unsafe {
            sys::Dart_EnterIsolate(self.raw);
            sys::Dart_ShutdownIsolate();
        }
    }
}

impl Drop for Isolate<'_> {
    fn drop(&mut self) {
        self.shutdown_inner();
    }
}

/// An isolate entered on the current thread. Exits on drop.
pub struct IsolateScope<'i> {
    raw: sys::Dart_Isolate,
    _isolate: PhantomData<&'i mut ()>,
    _not_send: PhantomData<*const ()>,
}

impl IsolateScope<'_> {
    /// Runs `f` inside a `Dart_EnterScope`/`Dart_ExitScope` pair. Every handle
    /// `f` makes is released when it returns, and the signature is what makes
    /// that safe: `f` must work for any `'s`, so nothing carrying `'s` can
    /// leave it.
    pub fn with<R>(&mut self, f: impl for<'s> FnOnce(&mut HandleScope<'s>) -> R) -> R {
        struct Exit;
        impl Drop for Exit {
            fn drop(&mut self) {
                // SAFETY: paired with the EnterScope below, on the same thread
                // and isolate, even if `f` unwinds.
                unsafe { sys::Dart_ExitScope() };
            }
        }
        // SAFETY: this scope's isolate is current on this thread.
        unsafe { sys::Dart_EnterScope() };
        let _exit = Exit;
        let mut scope = HandleScope {
            isolate: self.raw,
            _s: PhantomData,
        };
        f(&mut scope)
    }
}

impl Drop for IsolateScope<'_> {
    fn drop(&mut self) {
        // SAFETY: entered in Isolate::enter on this thread (the type is !Send).
        unsafe { sys::Dart_ExitIsolate() };
    }
}

/// An open API scope: the lifetime every [`Handle`] made in it borrows.
pub struct HandleScope<'s> {
    isolate: sys::Dart_Isolate,
    // Invariant in 's, so a scope cannot be passed off as a longer-lived one.
    _s: PhantomData<fn(&'s ()) -> &'s ()>,
}

/// A reference to a Dart object, valid while the scope that made it is open.
#[derive(Clone, Copy)]
pub struct Handle<'s> {
    raw: sys::Dart_Handle,
    _s: PhantomData<fn(&'s ()) -> &'s ()>,
}

impl std::fmt::Debug for Handle<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Handle").field(&self.raw).finish()
    }
}

impl<'s> HandleScope<'s> {
    fn wrap(&self, raw: sys::Dart_Handle) -> Result<Handle<'s>, DartError> {
        // SAFETY: this scope is open on the current isolate, which made `raw`.
        unsafe { check(raw) }.map(|raw| Handle {
            raw,
            _s: PhantomData,
        })
    }

    /// Runs the root library's `main` the way the standalone VM does: through
    /// `dart:isolate`'s `_startMainIsolate`, which is what lets `main` take
    /// `List<String>` arguments and what schedules it as the isolate's first
    /// message rather than calling it on this stack. It runs on the next
    /// [`run_loop`](Self::run_loop).
    pub fn invoke_main(&mut self, args: &[&str]) -> Result<(), DartError> {
        // SAFETY: an open scope on the current isolate is all these need.
        let root = self.wrap(unsafe { sys::Dart_RootLibrary() })?;
        // SAFETY: as above.
        if unsafe { sys::Dart_IsNull(root.raw) } {
            return Err(DartError::Api("no root library".into()));
        }
        let main = self.get_field(root, "main")?;
        // SAFETY: as above.
        if !unsafe { sys::Dart_IsClosure(main.raw) } {
            return Err(DartError::Api(
                "the root library has no `main` function".into(),
            ));
        }
        let argv = if args.is_empty() {
            self.null()
        } else {
            self.new_string_list(args)?
        };
        let isolate_lib = self.lookup_library("dart:isolate")?;
        self.invoke(isolate_lib, "_startMainIsolate", &[main, argv])
            .map(drop)
    }

    /// Handles messages until the isolate has no open receive ports.
    pub fn run_loop(&mut self) -> Result<(), DartError> {
        // SAFETY: the current isolate is this scope's.
        self.wrap(unsafe { sys::Dart_RunLoop() }).map(drop)
    }

    pub fn null(&mut self) -> Handle<'s> {
        // SAFETY: Dart_Null never fails and needs only a current isolate.
        Handle {
            raw: unsafe { sys::Dart_Null() },
            _s: PhantomData,
        }
    }

    pub fn new_string(&mut self, s: &str) -> Result<Handle<'s>, DartError> {
        // SAFETY: the pointer/length pair describes `s`, which the VM copies.
        self.wrap(unsafe { sys::Dart_NewStringFromUTF8(s.as_ptr(), s.len() as isize) })
    }

    pub fn new_int(&mut self, v: i64) -> Handle<'s> {
        // SAFETY: Dart_NewInteger cannot fail for any i64.
        Handle {
            raw: unsafe { sys::Dart_NewInteger(v) },
            _s: PhantomData,
        }
    }

    pub fn to_i64(&mut self, h: Handle<'s>) -> Result<i64, DartError> {
        let mut v = 0i64;
        // SAFETY: h belongs to this scope; v is a valid out-pointer.
        unsafe { check(sys::Dart_IntegerToInt64(h.raw, &mut v)) }?;
        Ok(v)
    }

    pub fn to_rust_string(&mut self, h: Handle<'s>) -> Result<String, DartError> {
        // SAFETY: h belongs to this scope.
        unsafe { string_value(h.raw) }
    }

    /// A `List<String>`, the type `main`'s arguments have.
    pub fn new_string_list(&mut self, items: &[&str]) -> Result<Handle<'s>, DartError> {
        let core = self.lookup_library("dart:core")?;
        let name = self.new_string("String")?;
        // SAFETY: core and name are live handles of this scope.
        let ty = self.wrap(unsafe {
            sys::Dart_GetNonNullableType(core.raw, name.raw, 0, std::ptr::null_mut())
        })?;
        let empty = self.new_string("")?;
        // SAFETY: as above.
        let list = self.wrap(unsafe {
            sys::Dart_NewListOfTypeFilled(ty.raw, empty.raw, items.len() as isize)
        })?;
        for (i, item) in items.iter().enumerate() {
            let s = self.new_string(item)?;
            // SAFETY: i is in bounds of the list just created.
            self.wrap(unsafe { sys::Dart_ListSetAt(list.raw, i as isize, s.raw) })?;
        }
        Ok(list)
    }

    pub fn lookup_library(&mut self, uri: &str) -> Result<Handle<'s>, DartError> {
        let u = self.new_string(uri)?;
        // SAFETY: u is a live string handle.
        self.wrap(unsafe { sys::Dart_LookupLibrary(u.raw) })
    }

    pub fn get_field(&mut self, target: Handle<'s>, name: &str) -> Result<Handle<'s>, DartError> {
        let n = self.new_string(name)?;
        // SAFETY: both handles are live in this scope.
        self.wrap(unsafe { sys::Dart_GetField(target.raw, n.raw) })
    }

    pub fn set_field(
        &mut self,
        target: Handle<'s>,
        name: &str,
        value: Handle<'s>,
    ) -> Result<(), DartError> {
        let n = self.new_string(name)?;
        // SAFETY: all handles are live in this scope.
        self.wrap(unsafe { sys::Dart_SetField(target.raw, n.raw, value.raw) })
            .map(drop)
    }

    pub fn invoke(
        &mut self,
        target: Handle<'s>,
        name: &str,
        args: &[Handle<'s>],
    ) -> Result<Handle<'s>, DartError> {
        let n = self.new_string(name)?;
        let mut raw: Vec<sys::Dart_Handle> = args.iter().map(|h| h.raw).collect();
        // SAFETY: every handle is live in this scope; raw outlives the call.
        self.wrap(unsafe {
            sys::Dart_Invoke(target.raw, n.raw, raw.len() as i32, raw.as_mut_ptr())
        })
    }

    /// A handle that outlives this scope. Deleted with
    /// [`PersistentHandle::delete`], never on drop: deleting one needs its
    /// isolate entered, which a `Drop` cannot promise.
    pub fn new_persistent(&mut self, h: Handle<'s>) -> Result<PersistentHandle, DartError> {
        // SAFETY: h is live in this scope.
        let raw = unsafe { sys::Dart_NewPersistentHandle(h.raw) };
        if raw.is_null() {
            return Err(DartError::Api("Dart_NewPersistentHandle failed".into()));
        }
        LIVE_PERSISTENT.fetch_add(1, Ordering::Relaxed);
        Ok(PersistentHandle {
            raw,
            isolate: self.isolate,
            _not_send: PhantomData,
        })
    }

    /// Attaches `peer` to `h`'s object, dropped when the object is collected
    /// (or when its isolate group shuts down, whichever is first).
    pub fn new_finalizable<T: Send + 'static>(
        &mut self,
        h: Handle<'s>,
        peer: T,
        external_size: usize,
    ) -> Result<(), DartError> {
        extern "C" fn finalize<T>(_isolate_callback_data: *mut c_void, peer: *mut c_void) {
            let _ = std::panic::catch_unwind(|| {
                // SAFETY: peer is the Box<T> leaked below, finalized exactly once.
                drop(unsafe { Box::from_raw(peer as *mut T) });
            });
        }
        let peer = Box::into_raw(Box::new(peer)) as *mut c_void;
        // SAFETY: h is live; the callback matches the peer's type.
        let fh = unsafe {
            sys::Dart_NewFinalizableHandle(h.raw, peer, external_size as isize, Some(finalize::<T>))
        };
        if fh.is_null() {
            // SAFETY: the VM did not take the peer.
            drop(unsafe { Box::from_raw(peer as *mut T) });
            return Err(DartError::Api(
                "Dart_NewFinalizableHandle failed (not an instance?)".into(),
            ));
        }
        Ok(())
    }

    /// The raw handle, for calls this layer does not wrap yet.
    pub fn raw(&self, h: Handle<'s>) -> sys::Dart_Handle {
        h.raw
    }
}

static LIVE_PERSISTENT: AtomicUsize = AtomicUsize::new(0);

/// Persistent handles created and not yet deleted, process-wide. For the leak
/// lanes: a nonzero count at shutdown is a leak that names its kind.
pub fn live_persistent_handles() -> usize {
    LIVE_PERSISTENT.load(Ordering::Relaxed)
}

/// A handle that survives its scope. No `Drop`: see
/// [`HandleScope::new_persistent`].
#[must_use = "a PersistentHandle leaks unless deleted"]
pub struct PersistentHandle {
    raw: sys::Dart_PersistentHandle,
    isolate: sys::Dart_Isolate,
    _not_send: PhantomData<*const ()>,
}

impl PersistentHandle {
    /// The object, as a handle of `scope`. Fails if `scope` is another isolate's.
    pub fn get<'s>(&self, scope: &mut HandleScope<'s>) -> Result<Handle<'s>, DartError> {
        self.same_isolate(scope)?;
        // SAFETY: raw is live and belongs to the isolate scope has entered.
        scope.wrap(unsafe { sys::Dart_HandleFromPersistent(self.raw) })
    }

    /// Deletes the handle. Fails, handing it back, if `scope` is another
    /// isolate's.
    pub fn delete(self, scope: &mut HandleScope<'_>) -> Result<(), (PersistentHandle, DartError)> {
        if let Err(e) = self.same_isolate(scope) {
            return Err((self, e));
        }
        // SAFETY: raw is live and its isolate is current.
        unsafe { sys::Dart_DeletePersistentHandle(self.raw) };
        LIVE_PERSISTENT.fetch_sub(1, Ordering::Relaxed);
        Ok(())
    }

    fn same_isolate(&self, scope: &HandleScope<'_>) -> Result<(), DartError> {
        if scope.isolate == self.isolate {
            Ok(())
        } else {
            Err(DartError::Api(
                "persistent handle used in another isolate".into(),
            ))
        }
    }
}

/// The core-library hooks a standalone embedder installs: `print()` to the
/// isolate's [`PrintSink`], and microtask scheduling.
///
/// Not yet installed: `VMLibraryHooks.eventHandlerSendData`, through which
/// `Timer` asks the embedder for a wakeup. In the standalone VM and in Flutter
/// that is dart:io's event handler; see `docs/m1-status.md`, "Timers".
///
/// `print` in the VM's core libraries calls `dart:_internal`'s `_printClosure`,
/// which throws until an embedder sets it. The standalone VM sets it to
/// `dart:_builtin`'s `_print`, whose one native, `Builtin_PrintString`, is
/// resolved by the embedder (`runtime/bin/dartutils.cc`,
/// `PrepareBuiltinLibrary`). This does the same, with the native resolved to
/// Rust instead of to stdout.
///
/// # Safety
/// A current isolate with an open scope.
unsafe fn install_hooks() -> Result<(), DartError> {
    // SAFETY: per this function's contract.
    unsafe {
        let lib = |uri: &str| -> Result<sys::Dart_Handle, DartError> {
            let s = check(sys::Dart_NewStringFromUTF8(
                uri.as_ptr(),
                uri.len() as isize,
            ))?;
            check(sys::Dart_LookupLibrary(s))
        };
        let name = |n: &str| sys::Dart_NewStringFromUTF8(n.as_ptr(), n.len() as isize);
        let builtin = lib("dart:_builtin")?;
        let internal = lib("dart:_internal")?;
        check(sys::Dart_SetNativeResolver(
            builtin,
            Some(resolve_builtin),
            None,
        ))?;
        let print = check(sys::Dart_Invoke(
            builtin,
            name("_getPrintClosure"),
            0,
            std::ptr::null_mut(),
        ))?;
        check(sys::Dart_SetField(internal, name("_printClosure"), print))?;

        // Microtasks: dart:async schedules them through a closure the embedder
        // installs, and dart:isolate's hooks install the timer factory. Both as
        // `PrepareAsyncLibrary`/`PrepareIsolateLibrary` in dartutils.cc.
        let isolate_lib = lib("dart:isolate")?;
        let async_lib = lib("dart:async")?;
        let mut sched = [check(sys::Dart_Invoke(
            isolate_lib,
            name("_getIsolateScheduleImmediateClosure"),
            0,
            std::ptr::null_mut(),
        ))?];
        check(sys::Dart_Invoke(
            async_lib,
            name("_setScheduleImmediateClosure"),
            1,
            sched.as_mut_ptr(),
        ))?;
        check(sys::Dart_Invoke(
            isolate_lib,
            name("_setupHooks"),
            0,
            std::ptr::null_mut(),
        ))?;
    }
    Ok(())
}

extern "C" fn resolve_builtin(
    name: sys::Dart_Handle,
    _argc: i32,
    auto_setup_scope: *mut bool,
) -> sys::Dart_NativeFunction {
    std::panic::catch_unwind(|| {
        // SAFETY: the VM calls resolvers with a current isolate and scope.
        let n = unsafe { string_value(name) }.ok()?;
        if !auto_setup_scope.is_null() {
            // SAFETY: a valid out-pointer from the VM.
            unsafe { *auto_setup_scope = true };
        }
        match n.as_str() {
            "Builtin_PrintString" => Some(builtin_print_string as unsafe extern "C" fn(_)),
            _ => None,
        }
    })
    .ok()
    .flatten()
}

unsafe extern "C" fn builtin_print_string(args: sys::Dart_NativeArguments) {
    let _ = std::panic::catch_unwind(|| {
        // SAFETY: a native call with auto scope: a current isolate and scope.
        unsafe {
            let Ok(s) = string_value(sys::Dart_GetNativeArgument(args, 0)) else {
                return;
            };
            let data = sys::Dart_CurrentIsolateData() as *const IsolateData;
            if let Some(data) = data.as_ref() {
                (data.print)(&s);
            }
        }
    });
}

// Callbacks registered in Dart_InitializeParams. Each releases one reference
// handed to the VM at creation, and none may unwind into VM frames.

pub(crate) extern "C" fn on_shutdown(_group: *mut c_void, _isolate: *mut c_void) {}

pub(crate) extern "C" fn on_cleanup(_group: *mut c_void, isolate: *mut c_void) {
    let _ = std::panic::catch_unwind(|| {
        if !isolate.is_null() {
            // SAFETY: the reference Arc::into_raw produced in `create`.
            drop(unsafe { Arc::from_raw(isolate as *const IsolateData) });
        }
    });
}

pub(crate) extern "C" fn on_cleanup_group(group: *mut c_void) {
    let _ = std::panic::catch_unwind(|| {
        if !group.is_null() {
            // SAFETY: the reference Arc::into_raw produced in `create`.
            drop(unsafe { Arc::from_raw(group as *const GroupData) });
        }
    });
}
