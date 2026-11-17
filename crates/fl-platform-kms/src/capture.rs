//! What a display is showing, read back from the card and written as a
//! PNG: every plane on its CRTC, the cursor's among them, composed as the
//! card composes them (drmkit-capture). For looking at what a board shows
//! from somewhere else.

use std::path::Path;

use drmkit_core::Device;

use crate::{CursorTarget, Seat};

/// Writes what `target`'s CRTC is showing to `path`, and says its size.
/// The card is the seat's, so this is the session's own view of it: with
/// the session away there is nothing to read. A plane whose buffer cannot
/// be read (tiled, or not RGB) is left out, as drmkit-capture says.
pub fn capture(seat: &Seat, target: &CursorTarget, path: &Path) -> Result<(u32, u32), String> {
    let fd = seat
        .take_device(&target.path)
        .map_err(|e| format!("{}: {e}", target.path.display()))?;
    let device = Device::from_owned_fd(fd);
    // The planes and where they are, which the kernel shows a client that
    // asked for both. The display asked on this card already, but after a
    // switch of terminals the seat's descriptor is a new one, and may be
    // read here before the display has taken it again.
    device
        .enable_universal_planes()
        .and_then(|()| device.enable_atomic())
        .map_err(|e| format!("{}: {e}", target.path.display()))?;
    let image = drmkit_capture::snapshot(&device, target.crtc_id).map_err(|e| e.to_string())?;
    drmkit_capture::write_png(&image, path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((image.width(), image.height()))
}
