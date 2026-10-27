// Enter/exit pairing: Dart_ExitIsolate must run on the thread that entered,
// so an entered scope cannot be sent elsewhere to be dropped.
use fl_dart_vm::IsolateScope;

fn exit_elsewhere(scope: IsolateScope<'static>) {
    std::thread::spawn(move || drop(scope));
}

fn main() {}
