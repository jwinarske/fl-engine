// Thread placement (brief M3 §6): the glyph atlas and its device image stay
// on the raster thread; the cache cannot be sent to another.
use fl_raster::Glyphs;

fn main() {
    let glyphs = Glyphs::default();
    std::thread::spawn(move || drop(glyphs));
}
