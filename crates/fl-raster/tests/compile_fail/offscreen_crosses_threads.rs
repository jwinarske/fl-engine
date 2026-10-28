// Thread placement: an Offscreen holds the device and is made on the raster
// thread; it cannot be sent to another.
use fl_raster::Offscreen;

fn main() {
    let target = Offscreen::new(1, 1).ok();
    std::thread::spawn(move || drop(target));
}
