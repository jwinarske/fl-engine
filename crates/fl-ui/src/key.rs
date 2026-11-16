//! Key messages (brief M4 §2, D2): a `KeyData` packet as
//! `_unpackKeyData` reads it on `flutter/keydata`, the framework's answers
//! on it and on the legacy `flutter/keyevent`, and `flutter/keyboard`'s
//! `getKeyboardState`.
//!
//! The packet is the engine's `KeyDataPacket` (`lib/ui/window/
//! key_data_packet.cc`), made of an event as `FlutterEngineSendKeyEvent`
//! makes it.

use std::collections::BTreeMap;
use std::sync::Mutex;

use fl_input::{KeyData, KeyEventType};

/// The channel `KeyData` packets are sent on (`kFlutterKeyDataChannel`).
pub const KEY_DATA_CHANNEL: &str = "flutter/keydata";
/// The channel of the legacy raw key message.
pub const KEY_EVENT_CHANNEL: &str = "flutter/keyevent";

/// `kKeyDataFieldCount`: timestamp, type, physical, logical, synthesized,
/// device type.
pub const FIELD_COUNT: usize = 6;

/// A `KeyDataPacket`'s bytes: the character's length in bytes, the six
/// fields, eight bytes each in host order, then the character as UTF-8.
/// The device type is a keyboard's, which is what
/// `FlutterEngineSendKeyEvent` defaults to and the Linux embedder leaves.
pub fn encode_packet(data: &KeyData) -> Vec<u8> {
    let character = data.character.as_deref().unwrap_or("");
    let mut out = Vec::with_capacity((1 + FIELD_COUNT) * 8 + character.len());
    let kind: u64 = match data.kind {
        KeyEventType::Down => 0,
        KeyEventType::Up => 1,
        KeyEventType::Repeat => 2,
    };
    let fields: [u64; 1 + FIELD_COUNT] = [
        character.len() as u64,
        u64::try_from(data.timestamp.as_micros()).unwrap_or(u64::MAX),
        kind,
        data.physical,
        data.logical,
        u64::from(data.synthesized),
        0, // KeyEventDeviceType::kKeyboard
    ];
    for f in fields {
        out.extend(f.to_ne_bytes());
    }
    out.extend(character.as_bytes());
    out
}

/// Whether the framework's answer to a `KeyData` packet says it handled
/// the key: one byte, not 0 (`FlutterEngineSendKeyEvent`'s callback). No
/// answer, a null one or any other length is not handled.
pub fn key_data_handled(response: Option<&[u8]>) -> bool {
    matches!(response, Some([b]) if *b != 0)
}

/// Whether the framework's answer on `flutter/keyevent` says it handled
/// the key: JSON, `{"handled":true}` (`fl_key_event_channel_send_finish`).
/// No answer, or one that is not that, is not handled.
pub fn key_event_handled(response: Option<&[u8]>) -> bool {
    let Some(text) = response.and_then(|r| std::str::from_utf8(r).ok()) else {
        return false;
    };
    // The framework's JSON codec writes the one member and no spaces; an
    // answer spaced otherwise is still read.
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    compact == "{\"handled\":true}"
}

/// The keys the framework has been told are down, physical to logical:
/// what `getKeyboardState` answers. Kept from the `KeyData` dispatched,
/// so it is what the framework has seen and no more.
static PRESSED: Mutex<BTreeMap<u64, u64>> = Mutex::new(BTreeMap::new());

/// Notes a `KeyData` as dispatched: a down adds its key, an up removes it.
pub fn note_dispatched(data: &KeyData) {
    if data.is_empty() {
        return;
    }
    let mut pressed = PRESSED.lock().unwrap_or_else(|e| e.into_inner());
    match data.kind {
        KeyEventType::Down => {
            pressed.insert(data.physical, data.logical);
        }
        KeyEventType::Up => {
            pressed.remove(&data.physical);
        }
        KeyEventType::Repeat => {}
    }
}

/// Forgets every key: a new isolate has been told nothing.
pub fn reset_keyboard_state() {
    PRESSED.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// `getKeyboardState`'s answer (`fl_keyboard_handler`'s
/// `get_keyboard_state`): a `StandardMethodCodec` success envelope of a
/// map, physical key to logical key.
pub(crate) fn keyboard_state_reply() -> Vec<u8> {
    let pressed = PRESSED.lock().unwrap_or_else(|e| e.into_inner());
    encode_keyboard_state(&pressed)
}

fn encode_keyboard_state(pressed: &BTreeMap<u64, u64>) -> Vec<u8> {
    // Success, then a map (type 13) and its size.
    let mut out = vec![0, 13];
    write_size(&mut out, pressed.len());
    for (physical, logical) in pressed {
        write_int(&mut out, *physical);
        write_int(&mut out, *logical);
    }
    out
}

/// `StandardMessageCodec.writeSize`.
fn write_size(out: &mut Vec<u8>, n: usize) {
    if n < 254 {
        out.push(n as u8);
    } else if let Ok(n) = u16::try_from(n) {
        out.push(254);
        out.extend(n.to_ne_bytes());
    } else {
        out.push(255);
        out.extend((n as u32).to_ne_bytes());
    }
}

/// A key id as the embedder's `fl_value_new_int` is written: an int32
/// (type 3) if it is one, else an int64 (type 4).
fn write_int(out: &mut Vec<u8>, v: u64) {
    let v = v as i64;
    if let Ok(v) = i32::try_from(v) {
        out.push(3);
        out.extend(v.to_ne_bytes());
    } else {
        out.push(4);
        out.extend(v.to_ne_bytes());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn field(bytes: &[u8], i: usize) -> u64 {
        let raw: [u8; 8] = bytes[i * 8..i * 8 + 8].try_into().unwrap_or_default();
        u64::from_ne_bytes(raw)
    }

    /// The layout `_unpackKeyData` reads: the size, five fields it takes,
    /// the device type it skips, then the character.
    #[test]
    fn a_packet_is_laid_out_as_the_framework_unpacks_it() {
        let data = KeyData {
            timestamp: Duration::from_micros(12_345_000),
            kind: KeyEventType::Repeat,
            physical: 0x0007_0004,
            logical: 0xe9,
            character: Some("é".to_owned()),
            synthesized: true,
        };
        let bytes = encode_packet(&data);
        assert_eq!(bytes.len(), 7 * 8 + 2);
        let fields: Vec<u64> = (0..7).map(|i| field(&bytes, i)).collect();
        assert_eq!(fields, [2, 12_345_000, 2, 0x0007_0004, 0xe9, 1, 0]);
        assert_eq!(&bytes[56..], "é".as_bytes());
    }

    #[test]
    fn a_packet_with_no_character_ends_at_its_fields() {
        let bytes = encode_packet(&KeyData::empty());
        assert_eq!(bytes, [0; 56]);
        let up = KeyData {
            kind: KeyEventType::Up,
            ..KeyData::empty()
        };
        assert_eq!(field(&encode_packet(&up), 2), 1);
    }

    #[test]
    fn answers_are_read_as_the_embedder_reads_them() {
        assert!(key_data_handled(Some(&[1])));
        assert!(!key_data_handled(Some(&[0])));
        assert!(!key_data_handled(Some(&[1, 1])));
        assert!(!key_data_handled(Some(&[])));
        assert!(!key_data_handled(None));

        assert!(key_event_handled(Some(br#"{"handled":true}"#)));
        assert!(key_event_handled(Some(b"{ \"handled\": true }")));
        assert!(!key_event_handled(Some(br#"{"handled":false}"#)));
        assert!(!key_event_handled(Some(br#"{"nothandled":true}"#)));
        assert!(!key_event_handled(Some(&[0xff])));
        assert!(!key_event_handled(None));
    }

    /// `KeyboardChannelGetPressedState`'s answer: key A down.
    #[test]
    fn the_keyboard_state_is_a_standard_codec_map() {
        let mut pressed = BTreeMap::new();
        assert_eq!(encode_keyboard_state(&pressed), [0, 13, 0]);
        pressed.insert(0x0007_0004, 0x61);
        pressed.insert(0x0007_00e1, 0x2_0000_0102);
        let mut want = vec![0, 13, 2];
        want.push(3);
        want.extend(0x0007_0004_i32.to_ne_bytes());
        want.push(3);
        want.extend(0x61_i32.to_ne_bytes());
        want.push(3);
        want.extend(0x0007_00e1_i32.to_ne_bytes());
        want.push(4);
        want.extend(0x2_0000_0102_i64.to_ne_bytes());
        assert_eq!(encode_keyboard_state(&pressed), want);
    }
}
