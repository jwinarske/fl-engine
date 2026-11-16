//! fl-engine's KMS platform, through drmkit (brief M4 §4, §7, D3, D9). So
//! far its keymap: an XKB keymap from the system's rules, which every key
//! of an XKB source is resolved against, the script's as the boards'.
//! And its input method: dead keys and compose sequences from xkbcommon's
//! compose tables. Display, session, devices and cursor come with stage 6.
//!
//! Links the system's libxkbcommon, through drmkit-input's `Keyboard` and
//! beside it.
//! Nothing above `fl-input` names this crate (D8).

mod compose;
mod keymap;

pub use compose::{Compose, ComposeError};
pub use keymap::{KeymapError, XkbKeymap};
