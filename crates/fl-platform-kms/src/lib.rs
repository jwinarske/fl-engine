//! fl-engine's KMS platform, through drmkit (brief M4 §4, §7, D3, D9).
//! Its keymap: an XKB keymap from the system's rules, which every key of an
//! XKB source is resolved against, the script's as the boards'. Its input
//! method: dead keys and compose sequences from xkbcommon's compose tables.
//! Its seat, which the card and the devices are held through, and its
//! display: emblema's `ScanoutOutput` over drmkit's modesetting. Its
//! devices: libinput's, through drmkit-input, opened through the seat. The
//! cursor is to come.
//!
//! Links the system's libxkbcommon, through drmkit-input's `Keyboard` and
//! beside it, libseat, through drmkit-session, and libinput and libudev,
//! through drmkit-input.
//! Nothing above `fl-input` names this crate (D8).

mod compose;
mod devices;
mod display;
mod keymap;
mod seat;

pub use compose::{Compose, ComposeError};
pub use devices::DevicesConfig;
pub use display::{Display, DisplayError, Flip, Pacing};
pub use keymap::{KeymapError, XkbKeymap};
pub use seat::{Activity, Seat, SeatError};
