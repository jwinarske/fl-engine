// Peer lifetime: the value `with` lends is borrowed for the closure only.
// Returning the borrow would let a native keep it past its call, past the
// point the finalizer or `dispose` may drop the value.
use fl_dart_ffi::peer;

fn keep(raw: *mut std::ffi::c_void) -> Option<&'static mut u32> {
    // SAFETY: not reached; this must not compile.
    unsafe { peer::with::<u32, _>(raw, |v| v) }
}

fn main() {
    let _ = keep(std::ptr::null_mut());
}
