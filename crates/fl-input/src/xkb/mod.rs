//! Keys from an XKB keyboard, made into what the framework is told (brief
//! M4 §2, §5, D2, D7): the Linux embedder's key path below GDK, ported.
//! Every source with an XKB keymap shares it: the boards' devices, the
//! script, later a Wayland seat.
//!
//! A source resolves each key against its keymap into an [`XkbKey`], what
//! GDK hands the embedder, and gives it to [`Keys::handle`]:
//!
//! * [`Layout`] derives the key's logical key where the layout's own
//!   characters do not give one (`fl_keyboard_manager`'s `guarantee_layout`)
//! * [`Tracker`] makes the [`KeyData`](crate::KeyData), with synthesized
//!   events where the keymap's modifier and lock state disagree with what
//!   was sent (`fl_key_embedder_responder`)
//! * [`RawChannel`] makes the `flutter/keyevent` message
//!   (`fl_key_channel_responder`, `fl_key_event_channel`)
//!
//! No keymap is linked here: [`Keymap`] is what a source's keymap answers.

mod layout;
mod raw;
mod tables;
mod tracker;

use std::time::Duration;

pub use layout::Layout;
pub use raw::RawChannel;
pub use tracker::Tracker;

use crate::KeyEvent;

/// Modifier state bits, GDK's `GdkModifierType`: [`XkbKey::state`].
pub mod state {
    pub const SHIFT: u32 = 1 << 0;
    /// Caps Lock, or Shift Lock.
    pub const LOCK: u32 = 1 << 1;
    pub const CONTROL: u32 = 1 << 2;
    /// Alt.
    pub const MOD1: u32 = 1 << 3;
    /// Num Lock.
    pub const MOD2: u32 = 1 << 4;
    pub const MOD4: u32 = 1 << 6;
    pub const SUPER: u32 = 1 << 26;
    /// What the embedder reads as either Meta key held.
    pub const META: u32 = 1 << 28;
}

/// Keysyms the key path names.
pub mod keysym {
    pub const NUM_LOCK: u32 = 0xff7f;
    pub const CAPS_LOCK: u32 = 0xffe5;
    pub const SHIFT_LOCK: u32 = 0xffe6;
}

/// Bits of a key id below its plane (`kValueMask`).
const VALUE_MASK: u64 = 0x000_ffff_ffff;
/// The plane of keys that are a Unicode character (`kUnicodePlane`).
const UNICODE_PLANE: u64 = 0x000_0000_0000;
/// The plane of GTK's own codes, for a key no table has (`kGtkPlane`).
const GTK_PLANE: u64 = 0x015_0000_0000;

/// One key going down or coming up, resolved against the keymap: the
/// embedder's `FlKeyEvent`, a `GdkEventKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct XkbKey {
    /// When, on the frame clock.
    pub time: Duration,
    /// Down, not up. A down for a key already down is a repeat.
    pub pressed: bool,
    /// The XKB keycode: the evdev code plus 8.
    pub keycode: u16,
    /// The keysym the key gives with the state it was moved in.
    pub keyval: u32,
    /// The keysym's character (`gdk_keyval_to_unicode`,
    /// `xkb_keysym_to_utf32`), or 0 if it has none.
    pub unicode: u32,
    /// The modifiers and locks in effect before this event ([`state`]).
    pub state: u32,
    /// The layout in effect, where the keymap has several.
    pub group: u8,
}

/// What a source's keymap answers.
pub trait Keymap {
    /// The key with XKB keycode `keycode` going down or coming up at
    /// `time`: resolved against the state before it, which it then
    /// changes. A down for a key already down is a repeat, and changes
    /// nothing.
    fn key(&mut self, time: Duration, keycode: u16, pressed: bool) -> XkbKey;

    /// The keysym of a key at a shift level of a layout, or 0
    /// (`gdk_keymap_lookup_key`).
    fn lookup(&self, keycode: u16, group: u8, level: u8) -> u32;
}

/// The XKB keycode of the key Flutter names `name`, as
/// `PhysicalKeyboardKey` does: `keyA`, `shiftLeft`, `numpad1`.
pub fn keycode_named(name: &str) -> Option<u16> {
    tables::XKB_TO_PHYSICAL
        .iter()
        .find(|(_, _, n)| *n == name)
        .map(|(code, _, _)| *code)
}

/// The physical key of an XKB keycode: its USB HID usage, or the keycode
/// in GTK's plane for one the table lacks (`event_to_physical_key`).
pub fn physical_key(keycode: u16) -> u64 {
    match tables::XKB_TO_PHYSICAL.binary_search_by_key(&keycode, |(code, _, _)| *code) {
        Ok(i) => tables::XKB_TO_PHYSICAL[i].1,
        Err(_) => (u64::from(keycode) & VALUE_MASK) | GTK_PLANE,
    }
}

/// The logical key of a keysym: the table's, its character lowercased if
/// it is Latin-1, or the keysym in GTK's plane (`event_to_logical_key`).
pub fn logical_key(keyval: u32) -> u64 {
    match tables::KEYVAL_TO_LOGICAL.binary_search_by_key(&keyval, |(k, _)| *k) {
        Ok(i) => tables::KEYVAL_TO_LOGICAL[i].1,
        Err(_) if keyval < 256 => (to_lower(u64::from(keyval)) & VALUE_MASK) | UNICODE_PLANE,
        Err(_) => (u64::from(keyval) & VALUE_MASK) | GTK_PLANE,
    }
}

/// Lowercases a Latin-1 letter; anything else is itself.
fn to_lower(n: u64) -> u64 {
    match n {
        0x41..=0x5a => n + 0x20,
        0xc0..=0xde if n != 0xd7 => n + 0x20,
        _ => n,
    }
}

/// A keyboard's key path, a source's own: `fl_keyboard_manager` without
/// its waiting, which is the shell's.
#[derive(Debug, Default)]
pub struct Keys {
    layout: Layout,
    tracker: Tracker,
    raw: RawChannel,
}

impl Keys {
    pub fn new() -> Keys {
        Keys::default()
    }

    /// One key, as the framework is to be told it. `lookup` is the
    /// keymap's [`Keymap::lookup`].
    pub fn handle(&mut self, key: &XkbKey, lookup: impl Fn(u16, u8, u8) -> u32) -> KeyEvent {
        let specified = self.layout.logical_key(key.group, key.keycode, lookup);
        let mut data = Vec::new();
        self.tracker.handle(key, specified, &mut data);
        KeyEvent {
            data,
            raw: Some(self.raw.message(key, specified)),
        }
    }

    /// Brings the modifiers the framework has been told are down in line
    /// with `state`, as an event that is not a key's reports it
    /// (`fl_keyboard_manager_sync_modifier_if_needed`). `None` if they
    /// agree already.
    pub fn sync_modifiers(&mut self, state: u32, time: Duration) -> Option<KeyEvent> {
        let mut data = Vec::new();
        self.tracker.sync_modifiers(state, time, &mut data);
        (!data.is_empty()).then_some(KeyEvent { data, raw: None })
    }

    /// The layout was changed: every derived logical key is forgotten
    /// (`keymap_keys_changed_cb`).
    pub fn keymap_changed(&mut self) {
        self.layout = Layout::default();
    }

    /// The keys the framework has been told are down, physical to logical.
    pub fn pressed(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.tracker.pressed()
    }
}

#[cfg(test)]
mod keys_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// `KeyMappingTest.HasExpectedValues`.
    #[test]
    fn has_expected_values() {
        let goals = tables::LAYOUT_GOALS;
        assert!(goals.contains(&(0x41, 0x20, false)), "space");
        assert!(goals.contains(&(0x13, 0x30, true)), "digit 0");
        assert!(goals.contains(&(0x26, 0x61, true)), "key A");
        assert!(goals.contains(&(0x15, 0x3d, false)), "equal");
        assert!(
            goals.contains(&(0x5e, 0x2_0000_0020, false)),
            "intl backslash"
        );
    }

    #[test]
    fn the_tables_are_sorted_for_searching() {
        assert!(tables::XKB_TO_PHYSICAL.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(tables::KEYVAL_TO_LOGICAL
            .windows(2)
            .all(|w| w[0].0 < w[1].0));
        assert_eq!(tables::XKB_TO_PHYSICAL.len(), 217);
        assert_eq!(tables::KEYVAL_TO_LOGICAL.len(), 171);
        assert_eq!(tables::LAYOUT_GOALS.len(), 49);
    }

    /// The evdev code plus 8 is the XKB keycode, so the embedder's table
    /// serves (brief M4 §2): `KEY_A` is 30, `KEY_LEFTSHIFT` 42.
    #[test]
    fn keys_are_found_by_keycode_and_by_name() {
        assert_eq!(physical_key(30 + 8), 0x0007_0004);
        assert_eq!(physical_key(42 + 8), 0x0007_00e1);
        assert_eq!(keycode_named("keyA"), Some(0x26));
        assert_eq!(keycode_named("capsLock"), Some(0x42));
        assert_eq!(keycode_named("nope"), None);
        // No such key: GTK's plane.
        assert_eq!(physical_key(0x08), 0x15_0000_0008);
    }

    #[test]
    fn logical_keys_come_from_the_table_then_the_character() {
        assert_eq!(logical_key(0xffe1), 0x2_0000_0102, "Shift_L");
        assert_eq!(logical_key(0x61), 0x61, "a");
        assert_eq!(logical_key(0x41), 0x61, "A");
        assert_eq!(logical_key(0xc9), 0xe9, "Eacute");
        assert_eq!(logical_key(0xd7), 0xd7, "multiplication is no letter");
        assert_eq!(logical_key(0x6c1), 0x15_0000_06c1, "Cyrillic_a");
    }
}
