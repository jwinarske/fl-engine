//! The finalizable-handle peer milestone 2 builds on (brief §13), through the
//! safe API rather than the binary.
//!
//! `HandleScope::new_finalizable` ties a Rust value to a Dart object and hands
//! back no handle: a finalizable handle is unsafe to use once its object may
//! have been collected, so the API keeps it. What it promises is the peer's
//! lifetime: dropped exactly once, when the object is collected or its
//! isolate group shuts down, whichever is first. Collection timing is the
//! GC's, so the test pins the shutdown half: after shutdown the peer is gone,
//! and nothing drops it again.
//!
//! Its own test binary: a `Vm` is once per process.

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{compile, MODE};
use fl_dart_vm::{IsolateConfig, Vm, VmConfig};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Peer(Arc<AtomicUsize>);

impl Drop for Peer {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn finalizable_peer_is_dropped_once_by_isolate_shutdown() {
    let prog = compile("hello", MODE.0);
    let vm = Vm::init(VmConfig::default()).unwrap();
    let cfg = IsolateConfig {
        print: Arc::new(|_| {}),
        ..IsolateConfig::default()
    };
    #[cfg(feature = "jit")]
    let dill = std::fs::read(&prog).unwrap();
    #[cfg(feature = "jit")]
    let mut isolate = vm.isolate_from_kernel(&dill, cfg).unwrap();
    #[cfg(feature = "aot")]
    let snapshot = Arc::new(fl_dart_vm::AotSnapshot::open(&prog).unwrap());
    #[cfg(feature = "aot")]
    let mut isolate = vm.isolate_from_aot(&snapshot, cfg).unwrap();

    let drops = Arc::new(AtomicUsize::new(0));
    let peers = 100;
    isolate.enter().unwrap().with(|s| {
        for i in 0..peers {
            let h = s.new_string(&format!("owned by Dart {i}")).unwrap();
            s.new_finalizable(h, Peer(Arc::clone(&drops)), 64).unwrap();
        }
    });
    isolate.shutdown();
    assert_eq!(
        drops.load(Ordering::SeqCst),
        peers,
        "every peer dropped by shutdown"
    );
    drop(vm);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        peers,
        "and none dropped twice"
    );
}
