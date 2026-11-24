//! dart:io's blocking-call concurrency, as `VmConfig::io_concurrency` sets it
//! before the VM starts: the value dart:io holds is the one asked for, not
//! the SDK's 32.
//!
//! Its own test binary: a `Vm` is once per process.

#![allow(clippy::unwrap_used, clippy::panic)]

use fl_dart_vm::{Vm, VmConfig};

#[test]
fn the_vm_holds_the_io_concurrency_it_was_given() {
    let vm = Vm::init(VmConfig {
        io_concurrency: Some(3),
        ..VmConfig::default()
    })
    .unwrap();
    assert_eq!(vm.io_concurrency(), 3);
}
