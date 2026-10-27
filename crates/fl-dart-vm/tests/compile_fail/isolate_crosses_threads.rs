// Thread affinity: an isolate is entered by one thread at a time, and the
// type keeps it on the thread that made it.
use fl_dart_vm::Isolate;

fn move_to_another_thread(isolate: Isolate<'static>) {
    std::thread::spawn(move || drop(isolate));
}

fn main() {}
