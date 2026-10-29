// Thread placement (brief M3 §6): the GPU image cache holds the device's
// images and stays on the raster thread; it cannot be sent to another.
use fl_raster::GpuImages;

fn main() {
    let images = GpuImages::default();
    std::thread::spawn(move || drop(images));
}
