//! Dead keys and compose sequences (brief M4 D6): xkbcommon's compose
//! tables, in the place GTK's `GtkIMContextSimple` has under the Linux
//! embedder. No system input method: IBus and Fcitx need a session.
//!
//! The signals are GTK's, in GTK's order: a sequence begun shows what has
//! been typed of it as the text being composed (a dead key as its accent,
//! the compose key as a middle dot), and one finished takes that away and
//! commits the character. A key outside any sequence types its character.
//!
//! The tables are the locale's `Compose` file, which libxkbcommon finds
//! under `/usr/share/X11/locale` (libX11's data, what every Wayland
//! toolkit reads). Not ported: GTK's Ctrl+Shift+U hex input.

use std::ffi::OsStr;
use std::fmt;

use fl_input::text::{typed_character, ImEvent, InputMethod};
use fl_input::xkb::{state, XkbKey};
use xkbcommon::xkb;
use xkbcommon::xkb::compose;

/// Keysyms a sequence is shown by.
mod keysym {
    pub const MULTI_KEY: u32 = 0xff20;
    pub const DEAD_FIRST: u32 = 0xfe50;
}

/// The longest sequence shown: the tables' longest are well under it, and
/// a table read from a file is not trusted to stop.
const MAX_SEQUENCE: usize = 32;

/// A compose table that could not be had: no `Compose` file for the
/// locale, or libX11's data not installed.
#[derive(Debug)]
pub struct ComposeError {
    locale: String,
}

impl fmt::Display for ComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no compose table for the locale {:?}", self.locale)
    }
}

impl std::error::Error for ComposeError {}

/// An input method that composes by the locale's compose table.
pub struct Compose {
    state: compose::State,
    /// The keysyms of the sequence in hand.
    sequence: Vec<u32>,
    // After `state`, which holds the table, which holds the context: both
    // are kept so every reference to them is this value's.
    _table: compose::Table,
    _context: xkb::Context,
}

// SAFETY: a compose state, its table and their context are plain heap
// objects with no thread of their own; libxkbcommon's rule is that one is
// not used from two threads at once. Their reference counts are not
// atomic, and every reference to these three is held by this value (the
// context is made here and given to nothing else), so moving the value
// moves them all, and `&mut self` keeps the use to one thread at a time.
unsafe impl Send for Compose {}

impl Compose {
    /// The compose table of a locale: `en_US.UTF-8`, `C`.
    pub fn from_locale(locale: &str) -> Result<Compose, ComposeError> {
        let error = || ComposeError {
            locale: locale.to_owned(),
        };
        // The binding makes a C string of it.
        if locale.is_empty() || locale.contains('\0') {
            return Err(error());
        }
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let table = compose::Table::new_from_locale(
            &context,
            OsStr::new(locale),
            compose::COMPILE_NO_FLAGS,
        )
        .map_err(|()| error())?;
        let state = compose::State::new(&table, compose::STATE_NO_FLAGS);
        if state.get_raw_ptr().is_null() {
            return Err(error());
        }
        Ok(Compose {
            state,
            sequence: Vec::new(),
            _table: table,
            _context: context,
        })
    }

    /// The locale the environment names for character handling, as
    /// libxkbcommon says to find it: `LC_ALL`, `LC_CTYPE`, `LANG`, the
    /// first that is set and not empty, or `C`.
    pub fn locale_from_env() -> String {
        ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|v| !v.is_empty())
            .unwrap_or_else(|| "C".to_owned())
    }

    /// The sequence in hand as it is shown while composing.
    fn preedit(&self) -> String {
        self.sequence.iter().filter_map(|k| shown(*k)).collect()
    }

    /// Ends the sequence in hand: nothing is being composed, and the
    /// framework is told so if it was shown anything.
    fn end(&mut self, events: &mut Vec<ImEvent>) {
        self.state.reset();
        if !self.sequence.is_empty() {
            self.sequence.clear();
            events.push(ImEvent::PreeditChanged {
                text: String::new(),
                cursor: 0,
            });
            events.push(ImEvent::PreeditEnd);
        }
    }
}

/// What a keysym of a sequence is shown as (`gtk_im_context_simple_get_
/// preedit_string`): the compose key as a middle dot, a dead key as its
/// accent standing alone, anything else as its character.
fn shown(keyval: u32) -> Option<char> {
    if keyval == keysym::MULTI_KEY {
        return Some('·');
    }
    let accent = match keyval.checked_sub(keysym::DEAD_FIRST) {
        Some(0x00) => 0x60,  // dead_grave
        Some(0x01) => 0xb4,  // dead_acute
        Some(0x02) => 0x5e,  // dead_circumflex
        Some(0x03) => 0x7e,  // dead_tilde
        Some(0x04) => 0xaf,  // dead_macron
        Some(0x05) => 0x2d8, // dead_breve
        Some(0x06) => 0x2d9, // dead_abovedot
        Some(0x07) => 0xa8,  // dead_diaeresis
        Some(0x08) => 0x2da, // dead_abovering
        Some(0x09) => 0x2dd, // dead_doubleacute
        Some(0x0a) => 0x2c7, // dead_caron
        Some(0x0b) => 0xb8,  // dead_cedilla
        Some(0x0c) => 0x2db, // dead_ogonek
        Some(0x0d) => 0x37a, // dead_iota
        _ => xkb::keysym_to_utf32(xkb::Keysym::new(keyval)),
    };
    char::from_u32(accent).filter(|c| *c != '\0')
}

impl InputMethod for Compose {
    fn filter_key(&mut self, key: &XkbKey, events: &mut Vec<ImEvent>) -> bool {
        // A release is no part of a sequence, and neither is a shortcut's
        // key: Control and Alt are GTK's modifiers that type no text.
        if !key.pressed || key.state & (state::CONTROL | state::MOD1) != 0 {
            return false;
        }
        let fed = self.state.feed(xkb::Keysym::new(key.keyval));
        if fed == compose::FeedResult::Ignored {
            // A modifier's key: a sequence goes on across it.
            return false;
        }
        match self.state.status() {
            compose::Status::Composing => {
                if self.sequence.len() >= MAX_SEQUENCE {
                    self.end(events);
                    return true;
                }
                if self.sequence.is_empty() {
                    events.push(ImEvent::PreeditStart);
                }
                self.sequence.push(key.keyval);
                let text = self.preedit();
                let cursor = text.encode_utf16().count();
                events.push(ImEvent::PreeditChanged { text, cursor });
                true
            }
            compose::Status::Composed => {
                let text = self.state.utf8();
                self.end(events);
                if let Some(text) = text {
                    events.push(ImEvent::Commit(text));
                }
                true
            }
            compose::Status::Cancelled => {
                // No such sequence: it is dropped, and the key with it.
                self.end(events);
                true
            }
            compose::Status::Nothing => match typed_character(key) {
                Some(c) => {
                    events.push(ImEvent::Commit(c.to_string()));
                    true
                }
                None => false,
            },
        }
    }

    fn reset(&mut self) {
        self.state.reset();
        self.sequence.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use fl_input::xkb::{keycode_named, Keymap};

    use super::*;
    use crate::XkbKeymap;

    #[allow(clippy::expect_used)]
    fn compose() -> Compose {
        Compose::from_locale("en_US.UTF-8")
            .expect("the locale's Compose file is installed (libX11's data)")
    }

    #[allow(clippy::expect_used)]
    fn intl() -> XkbKeymap {
        XkbKeymap::from_layout("us", "intl").expect("the layout is installed (xkeyboard-config)")
    }

    /// A key tapped: what the input method made of the press, and that
    /// the release is not its.
    #[allow(clippy::expect_used)]
    fn tap(im: &mut Compose, map: &mut XkbKeymap, name: &str) -> (bool, Vec<ImEvent>) {
        let code = keycode_named(name).expect("a key Flutter names");
        let mut events = Vec::new();
        let taken = im.filter_key(&map.key(Duration::ZERO, code, true), &mut events);
        let mut none = Vec::new();
        assert!(!im.filter_key(&map.key(Duration::ZERO, code, false), &mut none));
        assert!(none.is_empty());
        (taken, events)
    }

    fn preedit(text: &str) -> ImEvent {
        ImEvent::PreeditChanged {
            text: text.to_owned(),
            cursor: text.encode_utf16().count(),
        }
    }

    fn commit(text: &str) -> ImEvent {
        ImEvent::Commit(text.to_owned())
    }

    #[test]
    fn a_key_outside_a_sequence_types_its_character() {
        let (mut im, mut map) = (compose(), intl());
        assert_eq!(tap(&mut im, &mut map, "keyA"), (true, vec![commit("a")]));
        // Keys with no character are not text input's.
        assert_eq!(tap(&mut im, &mut map, "enter"), (false, vec![]));
        assert_eq!(tap(&mut im, &mut map, "arrowLeft"), (false, vec![]));
        assert_eq!(tap(&mut im, &mut map, "backspace"), (false, vec![]));
    }

    /// The dead acute of `us(intl)`, then e: GTK's signals for it.
    #[test]
    fn a_dead_key_composes_with_the_key_after_it() {
        let (mut im, mut map) = (compose(), intl());
        assert_eq!(
            tap(&mut im, &mut map, "quote"),
            (true, vec![ImEvent::PreeditStart, preedit("´")])
        );
        assert_eq!(
            tap(&mut im, &mut map, "keyE"),
            (true, vec![preedit(""), ImEvent::PreeditEnd, commit("é")])
        );
        // And the state is clear for the next.
        assert_eq!(tap(&mut im, &mut map, "keyE"), (true, vec![commit("e")]));
    }

    #[test]
    fn shift_held_within_a_sequence_does_not_end_it() {
        let (mut im, mut map) = (compose(), intl());
        tap(&mut im, &mut map, "quote");
        let shift = keycode_named("shiftLeft").unwrap_or_default();
        let mut events = Vec::new();
        assert!(!im.filter_key(&map.key(Duration::ZERO, shift, true), &mut events));
        assert_eq!(
            tap(&mut im, &mut map, "keyE"),
            (true, vec![preedit(""), ImEvent::PreeditEnd, commit("É")])
        );
        assert!(events.is_empty());
    }

    #[test]
    fn a_sequence_the_table_lacks_is_dropped_with_its_last_key() {
        let (mut im, mut map) = (compose(), intl());
        tap(&mut im, &mut map, "quote");
        assert_eq!(
            tap(&mut im, &mut map, "keyQ"),
            (true, vec![preedit(""), ImEvent::PreeditEnd])
        );
        assert_eq!(tap(&mut im, &mut map, "keyQ"), (true, vec![commit("q")]));
    }

    #[test]
    fn a_shortcut_s_key_is_no_text() {
        let (mut im, mut map) = (compose(), intl());
        let control = keycode_named("controlLeft").unwrap_or_default();
        let mut events = Vec::new();
        im.filter_key(&map.key(Duration::ZERO, control, true), &mut events);
        assert_eq!(tap(&mut im, &mut map, "keyV"), (false, vec![]));
        assert!(events.is_empty());
    }

    #[test]
    fn reset_abandons_a_sequence() {
        let (mut im, mut map) = (compose(), intl());
        tap(&mut im, &mut map, "quote");
        im.reset();
        assert_eq!(tap(&mut im, &mut map, "keyE"), (true, vec![commit("e")]));
    }

    #[test]
    fn a_sequence_is_shown_by_its_keys() {
        assert_eq!(shown(keysym::MULTI_KEY), Some('·'));
        assert_eq!(shown(0xfe50), Some('`'));
        assert_eq!(shown(0xfe57), Some('¨'));
        assert_eq!(shown(0x61), Some('a'));
        assert_eq!(shown(0xffe1), None, "Shift_L has no character");
    }

    #[test]
    fn a_locale_with_no_table_is_an_error() {
        assert!(Compose::from_locale("").is_err());
        assert!(Compose::from_locale("en\0US").is_err());
        let e = Compose::from_locale("xx_NOWHERE.nothing")
            .map(|_| ())
            .unwrap_err();
        assert_eq!(
            e.to_string(),
            "no compose table for the locale \"xx_NOWHERE.nothing\""
        );
    }

    /// It crosses to the UI thread, which is what `InputMethod` asks.
    #[test]
    fn it_moves_to_another_thread() {
        let mut im = compose();
        let mut map = intl();
        tap(&mut im, &mut map, "quote");
        let key = map.key(
            Duration::ZERO,
            keycode_named("keyE").unwrap_or_default(),
            true,
        );
        let events = std::thread::spawn(move || {
            let mut events = Vec::new();
            im.filter_key(&key, &mut events);
            events
        })
        .join()
        .unwrap_or_default();
        assert_eq!(events.last(), Some(&commit("é")));
    }
}
