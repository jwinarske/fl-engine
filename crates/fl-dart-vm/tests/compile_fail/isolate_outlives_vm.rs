// Dart_Cleanup must not run under a live isolate: isolates borrow the Vm.
use fl_dart_vm::{Isolate, Vm};

fn make(_vm: &Vm) -> Isolate<'_> {
    loop {}
}

fn outlive(vm: Vm) {
    let isolate = make(&vm);
    drop(vm);
    drop(isolate);
}

fn main() {}
