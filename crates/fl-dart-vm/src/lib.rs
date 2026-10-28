//! A safe host for the Dart VM.
//!
//! [`Vm`] initializes the process's one VM; an [`Isolate`] is created from a
//! kernel dill (`jit`) or an [`AotSnapshot`] (`aot`); [`Isolate::enter`] and
//! [`IsolateScope::with`] give a [`HandleScope`] to run Dart in; and a
//! [`NativePort`] carries messages back into Dart from any thread.
//!
//! ```no_run
//! # #[cfg(feature = "aot")] fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use std::sync::Arc;
//! use fl_dart_vm::{AotSnapshot, IsolateConfig, Vm, VmConfig};
//!
//! let vm = Vm::init(VmConfig::default())?;
//! let snapshot = Arc::new(AotSnapshot::open("hello.aot".as_ref())?);
//! let mut isolate = vm.isolate_from_aot(&snapshot, IsolateConfig::default())?;
//! isolate.enter()?.with(|s| {
//!     s.invoke_main(&[])?;
//!     s.run_loop()
//! })?;
//! isolate.shutdown();
//! # Ok(()) }
//! # #[cfg(not(feature = "aot"))] fn main() {}
//! ```

mod error;
mod isolate;
mod port;
mod snapshot;
mod vm;

pub use error::{DartError, PortClosed, VmError};
pub use isolate::{
    live_persistent_handles, Handle, HandleScope, Isolate, IsolateConfig, IsolateScope,
    NativeResolver, PersistentHandle, PrintSink,
};
pub use port::{CObject, NativePort};
#[cfg(feature = "aot")]
pub use snapshot::AotSnapshot;
pub use snapshot::{check_aot_header, check_kernel};
pub use vm::{Vm, VmConfig};

/// The raw bindings, for what this layer does not wrap yet.
pub use fl_dart_sys as sys;
