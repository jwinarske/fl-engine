//! Key events as data (brief M4 §2, D2, D8): `embedder.h`'s
//! `FlutterKeyEvent`, and what one key press or release is delivered as.

use std::time::Duration;

use crate::xkb::XkbKey;

/// `FlutterKeyEventType`, `KeyEventType` in key.dart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyEventType {
    #[default]
    Down,
    Up,
    Repeat,
}

/// `FlutterKeyEvent`: one change of one key, as the framework's
/// `HardwareKeyboard` is told it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyData {
    /// When, on the frame clock ([`EventSink::now`](crate::EventSink::now)).
    pub timestamp: Duration,
    pub kind: KeyEventType,
    /// The key's place on the keyboard: a USB HID usage, or a platform's
    /// own code in its plane.
    pub physical: u64,
    /// What the key means on the layout: a `LogicalKeyboardKey` id.
    pub logical: u64,
    /// What a down or a repeat types, if anything.
    pub character: Option<String>,
    /// Made up by the source to keep the framework's key state right, not
    /// a key the user moved.
    pub synthesized: bool,
}

impl KeyData {
    /// The embedder's `kEmptyEvent`: sent for a native event that is no
    /// change of any key, since the framework expects one `KeyData` at
    /// least for each.
    pub fn empty() -> KeyData {
        KeyData::default()
    }

    /// Whether this is [`KeyData::empty`]: no key at all.
    pub fn is_empty(&self) -> bool {
        self.physical == 0 && self.logical == 0
    }
}

/// One native key event, as the framework is to be told it (D2): the
/// [`KeyData`] it makes, in order, and its message for the legacy
/// `flutter/keyevent` channel.
///
/// The framework answers the `KeyData` that is neither synthesized nor
/// empty, and the message; the event is handled if either answer says so.
/// With no such `KeyData` the event counts as handled, as the embedder
/// counts a release of a key it never saw pressed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyEvent {
    /// Never empty: one [`KeyData::empty`] if nothing else.
    pub data: Vec<KeyData>,
    /// The `flutter/keyevent` message, JSON as the platform's
    /// `RawKeyEventData` reads it; `None` for a source that has none.
    pub raw: Option<Vec<u8>>,
    /// The key as the keymap resolved it, for text input, which is
    /// offered a key the framework did not handle
    /// ([`TextInput::filter_key`](crate::text::TextInput::filter_key));
    /// `None` for an event that is no key's, which text input is not
    /// offered.
    pub native: Option<XkbKey>,
}

impl KeyEvent {
    /// When it happened: its first `KeyData`'s time.
    pub fn timestamp(&self) -> Duration {
        self.data.first().map_or(Duration::ZERO, |d| d.timestamp)
    }

    /// The `KeyData` whose answer says whether the framework handled the
    /// key, if there is one.
    pub fn answered(&self) -> Option<usize> {
        self.data
            .iter()
            .position(|d| !d.synthesized && !d.is_empty())
    }
}
