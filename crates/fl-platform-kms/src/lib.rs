//! fl-engine's KMS platform, through drmkit (brief M4 §4, §7, D3, D9). So
//! far its keymap: an XKB keymap from the system's rules, which every key
//! of an XKB source is resolved against, the script's as the boards'.
//! Display, session, devices and cursor come with stage 6.
//!
//! Links the system's libxkbcommon, through drmkit-input's `Keyboard`.
//! Nothing above `fl-input` names this crate (D8).

mod keymap;

pub use keymap::{KeymapError, XkbKeymap};
