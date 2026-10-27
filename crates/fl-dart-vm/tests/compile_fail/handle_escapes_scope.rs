// Handle lifetime: a Dart_Handle dies with its API scope, so it cannot be
// returned from the closure that scope runs.
use fl_dart_vm::IsolateScope;

fn escape(scope: &mut IsolateScope<'_>) {
    let h = scope.with(|s| s.null());
    let _ = h;
}

fn main() {}
