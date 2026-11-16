//! The legacy `flutter/keyevent` message (D2), in the GTK shape the
//! framework's `RawKeyEventDataLinux` reads: `fl_key_channel_responder` and
//! `fl_key_event_channel`, ported. The whole channel is here, to go when
//! the framework drops `RawKeyEvent`.

use super::{keysym, state, XkbKey};

/// What the channel responder remembers between events: whether each lock
/// key is held, which is what it reports the lock bits as.
#[derive(Debug, Default)]
pub struct RawChannel {
    shift_lock_pressed: bool,
    caps_lock_pressed: bool,
    num_lock_pressed: bool,
}

impl RawChannel {
    /// The message for one key: JSON, UTF-8. `specified_logical` is the
    /// layout's logical key, or 0.
    pub fn message(&mut self, key: &XkbKey, specified_logical: u64) -> Vec<u8> {
        // The lock bits say whether a lock key is held, not whether the
        // lock is on: the framework toggles its own on each press.
        let mut modifiers = key.state & !(state::LOCK | state::MOD2);
        match key.keyval {
            keysym::NUM_LOCK => self.num_lock_pressed = key.pressed,
            keysym::CAPS_LOCK => self.caps_lock_pressed = key.pressed,
            keysym::SHIFT_LOCK => self.shift_lock_pressed = key.pressed,
            _ => {}
        }
        if self.shift_lock_pressed || self.caps_lock_pressed {
            modifiers |= state::LOCK;
        }
        if self.num_lock_pressed {
            modifiers |= state::MOD2;
        }
        let kind = if key.pressed { "keydown" } else { "keyup" };
        let mut json = format!(
            "{{\"type\":\"{kind}\",\"keymap\":\"linux\",\"scanCode\":{},\"toolkit\":\"gtk\",\
             \"keyCode\":{},\"modifiers\":{modifiers}",
            key.keycode, key.keyval
        );
        if key.unicode != 0 {
            json.push_str(&format!(",\"unicodeScalarValues\":{}", key.unicode));
        }
        if specified_logical != 0 {
            json.push_str(&format!(",\"specifiedLogicalKey\":{specified_logical}"));
        }
        json.push('}');
        json.into_bytes()
    }
}
