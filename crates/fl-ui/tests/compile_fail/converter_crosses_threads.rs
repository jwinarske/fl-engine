// Thread placement: the packet converter remembers every pointer there is,
// on the UI thread, where packets are dispatched; an input source's thread
// cannot be handed it.
use fl_ui::pointer::Converter;

fn main() {
    let converter = Converter::new();
    std::thread::spawn(move || drop(converter));
}
