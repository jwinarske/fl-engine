// Enter/exit pairing: the scope that exits an isolate cannot outlive the
// isolate it exits.
use fl_dart_vm::{Isolate, Vm};

fn make(_vm: &Vm) -> Isolate<'_> {
    loop {}
}

fn outlive(vm: &Vm) {
    let scope;
    {
        let mut isolate = make(vm);
        scope = isolate.enter().unwrap();
    }
    drop(scope);
}

fn main() {}
