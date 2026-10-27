// Handle lifetime, by the side door: stored in a variable that outlives the
// scope instead of returned from it.
use fl_dart_vm::{Handle, IsolateScope};

fn smuggle(scope: &mut IsolateScope<'_>) {
    let mut kept: Option<Handle<'_>> = None;
    scope.with(|s| kept = Some(s.null()));
    let _ = kept;
}

fn main() {}
