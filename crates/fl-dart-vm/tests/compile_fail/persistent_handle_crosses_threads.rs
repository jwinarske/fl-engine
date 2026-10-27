// A persistent handle names an object in one isolate, and deleting it needs
// that isolate entered on the deleting thread.
use fl_dart_vm::PersistentHandle;

fn move_to_another_thread(h: PersistentHandle) {
    std::thread::spawn(move || drop(h));
}

fn main() {}
