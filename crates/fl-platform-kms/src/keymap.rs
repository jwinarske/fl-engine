//! The XKB keymap (brief M4 §5, D3): drmkit-input's `Keyboard`, answering
//! what GDK answers the Linux embedder.

use std::fmt;
use std::time::Duration;

use drmkit_input::{KeyEvent, Keyboard, KeymapNames};
use fl_input::xkb::{state, Keymap, XkbKey};
use xkbcommon::xkb;

/// XKB keycodes are evdev codes plus this.
const EVDEV_OFFSET: u16 = 8;

/// A keymap that could not be had: names that do not compile, a layout
/// that is not installed.
#[derive(Debug)]
pub struct KeymapError {
    names: String,
}

impl fmt::Display for KeymapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no XKB keymap for {}", self.names)
    }
}

impl std::error::Error for KeymapError {}

/// A keymap and the state of the keys pressed against it: one for a seat.
pub struct XkbKeymap {
    keyboard: Keyboard,
}

impl XkbKeymap {
    /// The keymap of a layout and a variant of it (`us`, `intl`; `de`,
    /// empty), on the `evdev` rules and the `pc105` model, with no
    /// options. Named in full, so the environment's `XKB_DEFAULT_*` do not
    /// change it.
    pub fn from_layout(layout: &str, variant: &str) -> Result<XkbKeymap, KeymapError> {
        XkbKeymap::from_names(&KeymapNames {
            rules: "evdev".to_owned(),
            model: "pc105".to_owned(),
            layout: layout.to_owned(),
            variant: variant.to_owned(),
            // Not empty: an empty option list is the environment's.
            options: ",".to_owned(),
        })
    }

    /// The keymap of RMLVO names; an empty one is the system's default.
    pub fn from_names(names: &KeymapNames) -> Result<XkbKeymap, KeymapError> {
        match Keyboard::from_names(names) {
            Ok(keyboard) => Ok(XkbKeymap { keyboard }),
            Err(_) => Err(KeymapError {
                names: format!("{names:?}"),
            }),
        }
    }

    /// Whether a key held down repeats: a letter does, Shift does not.
    pub fn repeats(&self, keycode: u16) -> bool {
        self.keyboard
            .should_repeat(u32::from(keycode.saturating_sub(EVDEV_OFFSET)))
    }

    /// The XKB keycodes of the keys that are down.
    pub fn held(&self) -> Vec<u16> {
        self.keyboard
            .held_keys()
            .iter()
            .filter_map(|evdev| u16::try_from(*evdev).ok()?.checked_add(EVDEV_OFFSET))
            .collect()
    }

    /// The modifiers and locks in effect, as GDK reports an event's state.
    ///
    /// The logo key is reported as Mod4, as Super, and as what the
    /// embedder reads for a Meta key, `GDK_META_MASK`: its Meta keys are
    /// the logo keys, and its legacy message's Meta is `GDK_SUPER_MASK`.
    fn state(&self) -> u32 {
        let mods = self.keyboard.modifiers();
        let leds = self.keyboard.leds();
        let mut s = 0;
        if mods.shift {
            s |= state::SHIFT;
        }
        if mods.ctrl {
            s |= state::CONTROL;
        }
        if mods.alt {
            s |= state::MOD1;
        }
        if mods.logo {
            s |= state::MOD4 | state::SUPER | state::META;
        }
        if leds.caps_lock {
            s |= state::LOCK;
        }
        if leds.num_lock {
            s |= state::MOD2;
        }
        s
    }
}

impl Keymap for XkbKeymap {
    fn key(&mut self, time: Duration, keycode: u16, pressed: bool) -> XkbKey {
        // GDK's state is the one the key was moved in: read first.
        let state = self.state();
        let evdev = u32::from(keycode.saturating_sub(EVDEV_OFFSET));
        let mut event = KeyEvent {
            key: evdev,
            pressed,
            repeat: pressed && self.keyboard.held_keys().contains(&evdev),
            ..KeyEvent::default()
        };
        self.keyboard.process_key(&mut event);
        XkbKey {
            time,
            pressed,
            keycode,
            keyval: event.sym,
            unicode: xkb::keysym_to_utf32(xkb::Keysym::new(event.sym)),
            state,
            // drmkit's `Keyboard` does not say which layout is in effect.
            // A keymap of one layout, which `from_layout` makes, has 0.
            group: 0,
        }
    }

    fn lookup(&self, keycode: u16, group: u8, level: u8) -> u32 {
        let keymap = self.keyboard.keymap();
        if u32::from(group) >= keymap.num_layouts_for_key(u32::from(keycode).into()) {
            return 0;
        }
        keymap
            .key_get_syms_by_level(u32::from(keycode).into(), group.into(), level.into())
            .first()
            .map_or(0, |sym| sym.raw())
    }
}

#[cfg(test)]
mod tests {
    use fl_input::xkb::{keycode_named, Keys};
    use fl_input::KeyEventType;

    use super::*;

    #[allow(clippy::expect_used)]
    fn keymap(layout: &str, variant: &str) -> XkbKeymap {
        XkbKeymap::from_layout(layout, variant).expect("the layout is installed (xkeyboard-config)")
    }

    #[allow(clippy::expect_used)]
    fn code(name: &str) -> u16 {
        keycode_named(name).expect("a key Flutter names")
    }

    fn press(map: &mut XkbKeymap, name: &str, pressed: bool) -> XkbKey {
        map.key(Duration::ZERO, code(name), pressed)
    }

    #[test]
    fn a_key_is_resolved_in_the_state_before_it() {
        let mut map = keymap("us", "");
        let shift = press(&mut map, "shiftLeft", true);
        assert_eq!((shift.keyval, shift.state, shift.unicode), (0xffe1, 0, 0));
        let a = press(&mut map, "keyA", true);
        assert_eq!((a.keyval, a.state, a.unicode), (0x41, state::SHIFT, 0x41));
        press(&mut map, "keyA", false);
        let up = press(&mut map, "shiftLeft", false);
        assert_eq!(up.state, state::SHIFT);
        let a = press(&mut map, "keyA", true);
        assert_eq!((a.keyval, a.state), (0x61, 0));
    }

    #[test]
    fn locks_show_in_the_state_from_the_event_after() {
        let mut map = keymap("us", "");
        assert_eq!(press(&mut map, "capsLock", true).state, 0);
        assert_eq!(press(&mut map, "capsLock", false).state, state::LOCK);
        let a = press(&mut map, "keyA", true);
        assert_eq!((a.keyval, a.state), (0x41, state::LOCK));
        press(&mut map, "keyA", false);
        // The second press leaves the lock on until its release.
        assert_eq!(press(&mut map, "capsLock", true).state, state::LOCK);
        assert_eq!(press(&mut map, "capsLock", false).state, state::LOCK);
        assert_eq!(press(&mut map, "keyA", true).state, 0);

        assert_eq!(press(&mut map, "numLock", true).state, 0);
        let one = press(&mut map, "numpad1", true);
        assert_eq!(
            (one.keyval, one.state, one.unicode),
            (0xffb1, state::MOD2, 0x31)
        );
    }

    #[test]
    fn modifiers_are_reported_as_gdk_reports_them() {
        let mut map = keymap("us", "");
        press(&mut map, "controlLeft", true);
        press(&mut map, "altLeft", true);
        press(&mut map, "metaLeft", true);
        let a = press(&mut map, "keyA", true);
        assert_eq!(
            a.state,
            state::CONTROL | state::MOD1 | state::MOD4 | state::SUPER | state::META
        );
        // Control changes what the key types, not its keysym.
        assert_eq!((a.keyval, a.unicode), (0x61, 0x61));
    }

    #[test]
    fn a_down_for_a_key_down_is_a_repeat_and_changes_nothing() {
        let mut map = keymap("us", "");
        press(&mut map, "capsLock", true);
        // Not a second press: the lock stays on.
        press(&mut map, "capsLock", true);
        press(&mut map, "capsLock", false);
        assert_eq!(press(&mut map, "keyA", true).state, state::LOCK);
    }

    #[test]
    fn a_layout_s_keysyms_are_looked_up_by_level() {
        let map = keymap("fr", "");
        assert_eq!(map.lookup(code("keyA"), 0, 0), 0x71, "q");
        assert_eq!(map.lookup(code("keyA"), 0, 1), 0x51, "Q");
        assert_eq!(map.lookup(code("digit1"), 0, 0), 0x26, "ampersand");
        assert_eq!(map.lookup(code("digit1"), 0, 1), 0x31, "1");
        // No such group, no such key.
        assert_eq!(map.lookup(code("keyA"), 3, 0), 0);
        assert_eq!(map.lookup(0x07, 0, 0), 0);
    }

    #[test]
    fn a_keymap_not_installed_is_an_error() {
        assert!(XkbKeymap::from_layout("no-such-layout", "").is_err());
    }

    /// The whole path on real keymaps: what `CorrectLogicalKeyForLayouts`
    /// checks on mock data.
    #[test]
    fn logical_keys_are_derived_from_real_layouts() {
        let tap = |map: &mut XkbKeymap, keys: &mut Keys, name: &str| {
            let key = press(map, name, true);
            let event = keys.handle(&key, |c, g, l| map.lookup(c, g, l));
            let up = press(map, name, false);
            keys.handle(&up, |c, g, l| map.lookup(c, g, l));
            assert_eq!(event.data.len(), 1);
            assert_eq!(event.data[0].kind, KeyEventType::Down);
            (
                event.data[0].logical,
                event.data[0].character.clone().unwrap_or_default(),
            )
        };
        let (mut map, mut keys) = (keymap("fr", ""), Keys::new());
        assert_eq!(tap(&mut map, &mut keys, "keyA"), (0x71, "q".to_owned()));
        assert_eq!(
            tap(&mut map, &mut keys, "semicolon"),
            (0x6d, "m".to_owned())
        );
        assert_eq!(tap(&mut map, &mut keys, "digit1"), (0x31, "&".to_owned()));

        let (mut map, mut keys) = (keymap("ru", ""), Keys::new());
        assert_eq!(tap(&mut map, &mut keys, "keyA"), (0x61, "ф".to_owned()));
        assert_eq!(
            tap(&mut map, &mut keys, "bracketLeft"),
            (0x5b, "х".to_owned())
        );
        assert_eq!(tap(&mut map, &mut keys, "digit1"), (0x31, "1".to_owned()));
    }
}
