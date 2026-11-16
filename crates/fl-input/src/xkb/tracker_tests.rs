//! `fl_key_embedder_responder_test.cc`, ported: each case under its name,
//! event for event. Converted by script from the embedder's file, so the
//! comments are the embedder's.
//!
//! Left out: `TapLetterKeysBetweenCapsLockEventsReversed`, the case for
//! GDK turning its Caps Lock bit an event late, which a source that reads
//! its keymap before the event never does (see the module's notes). What
//! the embedder's cases check of answers is the shell's, not the
//! tracker's.

use std::time::Duration;

use super::super::{state, XkbKey};
use super::Tracker;
use crate::{KeyData, KeyEventType};

/// A tracker and what it sent: the embedder's mock engine.
#[derive(Default)]
struct Rig {
    tracker: Tracker,
    records: Vec<KeyData>,
    now: u64,
}

impl Rig {
    /// One native event at `time`, milliseconds, with the layout's logical
    /// key or 0.
    fn send(
        &mut self,
        time: u64,
        pressed: bool,
        keycode: u16,
        keyval: u32,
        state: u32,
        specified: u64,
    ) {
        let key = XkbKey {
            time: Duration::from_millis(time),
            pressed,
            keycode,
            keyval,
            unicode: unicode(keyval),
            state,
            group: 0,
        };
        self.tracker.handle(&key, specified, &mut self.records);
    }

    /// The embedder's `send_key_event` shorthand: a millisecond after the
    /// last, from 2.
    fn next(&mut self, pressed: bool, keycode: u16, keyval: u32, state: u32) {
        self.now += 1;
        self.send(self.now + 1, pressed, keycode, keyval, state, 0);
    }
}

/// `gdk_keyval_to_unicode`, for the keysyms these cases use.
fn unicode(keyval: u32) -> u32 {
    match keyval {
        0x20..=0xff => keyval,
        // KP_0 to KP_9.
        0xffb0..=0xffb9 => keyval - 0xffb0 + 0x30,
        _ => 0,
    }
}

const CODE_ALT_LEFT: u16 = 0x40;
const CODE_ALT_RIGHT: u16 = 0x6c;
const CODE_CAPS_LOCK: u16 = 0x42;
const CODE_CONTROL_LEFT: u16 = 0x25;
const CODE_CONTROL_RIGHT: u16 = 0x69;
const CODE_DIGIT1: u16 = 0x0a;
const CODE_KEY_A: u16 = 0x26;
const CODE_NUMPAD1: u16 = 0x57;
const CODE_NUM_LOCK: u16 = 0x4d;
const CODE_SHIFT_LEFT: u16 = 0x32;
const CODE_SHIFT_RIGHT: u16 = 0x3e;
const KEY_1: u32 = 0x0031;
const KEY_A: u32 = 0x0041;
const KEY_AMPERSAND: u32 = 0x0026;
const KEY_CAPS_LOCK: u32 = 0xffe5;
const KEY_CONTROL_L: u32 = 0xffe3;
const KEY_CONTROL_R: u32 = 0xffe4;
const KEY_EXCLAM: u32 = 0x0021;
const KEY_ISO_LEVEL3_SHIFT: u32 = 0xfe03;
const KEY_ISO_NEXT_GROUP: u32 = 0xfe08;
const KEY_KP_1: u32 = 0xffb1;
const KEY_KP_END: u32 = 0xff9c;
const KEY_LOWER_A: u32 = 0x0061;
const KEY_LOWER_Q: u32 = 0x0071;
const KEY_META_L: u32 = 0xffe7;
const KEY_META_R: u32 = 0xffe8;
const KEY_NUM_LOCK: u32 = 0xff7f;
const KEY_SHIFT_L: u32 = 0xffe1;
const KEY_SHIFT_R: u32 = 0xffe2;
const LOGICAL_ALT_LEFT: u64 = 0x00200000104;
const LOGICAL_CAPS_LOCK: u64 = 0x00100000104;
const LOGICAL_CONTROL_LEFT: u64 = 0x00200000100;
const LOGICAL_DIGIT1: u64 = 0x00000000031;
const LOGICAL_EXCLAMATION: u64 = 0x00000000021;
const LOGICAL_GROUP_NEXT: u64 = 0x00100000709;
const LOGICAL_KEY_A: u64 = 0x00000000061;
const LOGICAL_KEY_Q: u64 = 0x00000000071;
const LOGICAL_META_LEFT: u64 = 0x00200000106;
const LOGICAL_META_RIGHT: u64 = 0x00200000107;
const LOGICAL_NUMPAD1: u64 = 0x00200000231;
const LOGICAL_NUM_LOCK: u64 = 0x0010000010a;
const LOGICAL_SHIFT_LEFT: u64 = 0x00200000102;
const LOGICAL_SHIFT_RIGHT: u64 = 0x00200000103;
const PHYSICAL_ALT_LEFT: u64 = 0x000700e2;
const PHYSICAL_ALT_RIGHT: u64 = 0x000700e6;
const PHYSICAL_CAPS_LOCK: u64 = 0x00070039;
const PHYSICAL_CONTROL_LEFT: u64 = 0x000700e0;
const PHYSICAL_DIGIT1: u64 = 0x0007001e;
const PHYSICAL_KEY_A: u64 = 0x00070004;
const PHYSICAL_META_LEFT: u64 = 0x000700e3;
const PHYSICAL_NUMPAD1: u64 = 0x00070059;
const PHYSICAL_NUM_LOCK: u64 = 0x00070053;
const PHYSICAL_SHIFT_LEFT: u64 = 0x000700e1;
const PHYSICAL_SHIFT_RIGHT: u64 = 0x000700e5;

/// Not the embedder's: one logical key down on two keys, the later let
/// go, then the modifier gone from the state. The key still down comes
/// up, where the embedder names the key let go and leaves the other down.
#[test]
fn a_modifier_down_on_two_keys_comes_up_on_the_one_still_down() {
    let mut r = Rig::default();
    // Shift_L from the left Shift key, then from the right one.
    r.send(1, true, CODE_SHIFT_LEFT, KEY_SHIFT_L, 0, 0);
    r.send(2, true, CODE_SHIFT_RIGHT, KEY_SHIFT_L, state::SHIFT, 0);
    r.send(3, false, CODE_SHIFT_RIGHT, KEY_SHIFT_L, state::SHIFT, 0);
    r.records.clear();

    // A letter with no Shift in its state: the left key's release was missed.
    r.send(4, true, CODE_KEY_A, KEY_LOWER_A, 0, 0);
    assert_eq!(r.records.len(), 2);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_SHIFT_LEFT);
    assert_eq!(record.logical, LOGICAL_SHIFT_LEFT);
    assert!(record.synthesized);
    assert_eq!(
        r.tracker.pressed().collect::<Vec<_>>(),
        [(PHYSICAL_KEY_A, LOGICAL_KEY_A)]
    );
}

/// Not the embedder's: what is kept is bounded by the keyboard, whatever
/// keysyms a source sends.
#[test]
fn keysyms_without_end_keep_nothing() {
    let mut r = Rig::default();
    for keyval in 0x1000..0x2000 {
        r.send(1, true, CODE_KEY_A, keyval, 0, 0);
        r.send(2, false, CODE_KEY_A, keyval, 0, 0);
    }
    assert_eq!(r.tracker.pressed().count(), 0);
    assert!(r.tracker.mapping.is_empty());
    r.send(3, true, CODE_SHIFT_LEFT, KEY_SHIFT_L, 0, 0);
    assert_eq!(r.tracker.mapping.len(), 1);
}

/// Basic key presses
///
/// `SendKeyEvent`.
#[test]
fn send_key_event() {
    let mut r = Rig::default();

    // On a QWERTY keyboard, press key Q (physically key A), and release.
    // Key down
    r.send(12345, true, CODE_KEY_A, KEY_LOWER_A, 0, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(12345000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), Some("a"));
    assert!(!record.synthesized);
    r.records.clear();

    // Key up
    r.send(12346, false, CODE_KEY_A, KEY_LOWER_A, 0, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(12346000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // On an AZERTY keyboard, press key Q (physically key A), and release.
    // Key down
    r.send(12347, true, CODE_KEY_A, KEY_LOWER_Q, 0, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(12347000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_Q);
    assert_eq!(record.character.as_deref(), Some("q"));
    assert!(!record.synthesized);
    r.records.clear();

    // Key up
    r.send(12348, false, CODE_KEY_A, KEY_LOWER_Q, 0, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(12348000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_Q);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
}

/// Basic key presses, but uses the specified logical key if it is not 0.
///
/// `UsesSpecifiedLogicalKey`.
#[test]
fn uses_specified_logical_key() {
    let mut r = Rig::default();

    // On an AZERTY keyboard, press physical key 1, and release.
    // Key down
    r.send(12345, true, CODE_DIGIT1, KEY_AMPERSAND, 0, LOGICAL_DIGIT1);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(12345000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_DIGIT1);
    assert_eq!(record.logical, LOGICAL_DIGIT1);
    assert_eq!(record.character.as_deref(), Some("&"));
    assert!(!record.synthesized);
}

/// Press Shift, key A, then release Shift, key A.
///
/// `PressShiftDuringLetterKeyTap`.
#[test]
fn press_shift_during_letter_key_tap() {
    let mut r = Rig::default();

    // Press shift right
    r.send(101, true, CODE_SHIFT_RIGHT, KEY_SHIFT_R, 0, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_SHIFT_RIGHT);
    assert_eq!(record.logical, LOGICAL_SHIFT_RIGHT);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Press key A
    r.send(102, true, CODE_KEY_A, KEY_A, state::SHIFT, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), Some("A"));
    assert!(!record.synthesized);
    r.records.clear();

    // Release shift right
    r.send(103, false, CODE_SHIFT_RIGHT, KEY_SHIFT_R, state::SHIFT, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_SHIFT_RIGHT);
    assert_eq!(record.logical, LOGICAL_SHIFT_RIGHT);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Release key A
    r.send(104, false, CODE_KEY_A, KEY_A, 0, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
}

/// Press or release Numpad 1 between presses/releases of NumLock.
///
/// This tests interaction between lock keys and non-lock keys in cases that do
/// not have events missed.
///
/// This also tests the result of Numpad keys across NumLock taps, which is
/// test-worthy because the keyval for the numpad key will change before and
/// after the NumLock tap, which should not alter the resulting logical key.
///
/// `TapNumPadKeysBetweenNumLockEvents`.
#[test]
fn tap_num_pad_keys_between_num_lock_events() {
    let mut r = Rig::default();

    // Press Numpad 1 (stage 0)
    r.send(101, true, CODE_NUMPAD1, KEY_KP_END, 0, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_NUMPAD1);
    assert_eq!(record.logical, LOGICAL_NUMPAD1);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Press NumLock (stage 0 -> 1)
    r.send(102, true, CODE_NUM_LOCK, KEY_NUM_LOCK, 0, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Release numpad 1 (stage 1)
    r.send(104, false, CODE_NUMPAD1, KEY_KP_1, state::MOD2, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_NUMPAD1);
    assert_eq!(record.logical, LOGICAL_NUMPAD1);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Release NumLock (stage 1 -> 2)
    r.send(103, false, CODE_NUM_LOCK, KEY_NUM_LOCK, state::MOD2, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Press Numpad 1 (stage 2)
    r.send(101, true, CODE_NUMPAD1, KEY_KP_END, state::MOD2, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_NUMPAD1);
    assert_eq!(record.logical, LOGICAL_NUMPAD1);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Press NumLock (stage 2 -> 3)
    r.send(102, true, CODE_NUM_LOCK, KEY_NUM_LOCK, state::MOD2, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Release numpad 1 (stage 3)
    r.send(104, false, CODE_NUMPAD1, KEY_KP_1, state::MOD2, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_NUMPAD1);
    assert_eq!(record.logical, LOGICAL_NUMPAD1);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Release NumLock (stage 3 -> 0)
    r.send(103, false, CODE_NUM_LOCK, KEY_NUM_LOCK, state::MOD2, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
}

/// Press or release digit 1 between presses/releases of Shift.
///
/// GTK will change the virtual key during a key tap, and the embedder
/// should regularize it.
///
/// `ReleaseShiftKeyBetweenDigitKeyEvents`.
#[test]
fn release_shift_key_between_digit_key_events() {
    let mut r = Rig::default();
    let mut state: u32 = 0;

    // Press shift left
    r.send(101, true, CODE_SHIFT_LEFT, KEY_SHIFT_L, state, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_SHIFT_LEFT);
    assert_eq!(record.logical, LOGICAL_SHIFT_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();
    state = state::SHIFT;

    // Press digit 1, which is '!' on a US keyboard
    r.send(102, true, CODE_DIGIT1, KEY_EXCLAM, state, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_DIGIT1);
    assert_eq!(record.logical, LOGICAL_EXCLAMATION);
    assert_eq!(record.character.as_deref(), Some("!"));
    assert!(!record.synthesized);
    r.records.clear();

    // Release shift
    r.send(103, false, CODE_SHIFT_LEFT, KEY_SHIFT_L, state, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_SHIFT_LEFT);
    assert_eq!(record.logical, LOGICAL_SHIFT_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();
    state = 0;

    // Release digit 1, which is "1" because shift has been released.
    r.send(104, false, CODE_DIGIT1, KEY_1, state, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_DIGIT1);
    assert_eq!(record.logical, LOGICAL_EXCLAMATION);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
}

/// Press or release letter key between presses/releases of CapsLock.
///
/// This tests interaction between lock keys and non-lock keys in cases that do
/// not have events missed.
///
/// `TapLetterKeysBetweenCapsLockEvents`.
#[test]
fn tap_letter_keys_between_caps_lock_events() {
    let mut r = Rig::default();

    // Press CapsLock (stage 0 -> 1)
    r.send(101, true, CODE_CAPS_LOCK, KEY_CAPS_LOCK, 0, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_CAPS_LOCK);
    assert_eq!(record.logical, LOGICAL_CAPS_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Press key A (stage 1)
    r.send(102, true, CODE_KEY_A, KEY_A, state::LOCK, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), Some("A"));
    assert!(!record.synthesized);
    r.records.clear();

    // Release CapsLock (stage 1 -> 2)
    r.send(103, false, CODE_CAPS_LOCK, KEY_CAPS_LOCK, state::LOCK, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_CAPS_LOCK);
    assert_eq!(record.logical, LOGICAL_CAPS_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Release key A (stage 2)
    r.send(104, false, CODE_KEY_A, KEY_A, state::LOCK, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Press CapsLock (stage 2 -> 3)
    r.send(105, true, CODE_CAPS_LOCK, KEY_CAPS_LOCK, state::LOCK, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_CAPS_LOCK);
    assert_eq!(record.logical, LOGICAL_CAPS_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Press key A (stage 3)
    r.send(106, true, CODE_KEY_A, KEY_A, state::LOCK, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), Some("A"));
    assert!(!record.synthesized);
    r.records.clear();

    // Release CapsLock (stage 3 -> 0)
    r.send(107, false, CODE_CAPS_LOCK, KEY_CAPS_LOCK, state::LOCK, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_CAPS_LOCK);
    assert_eq!(record.logical, LOGICAL_CAPS_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Release key A (stage 0)
    r.send(108, false, CODE_KEY_A, KEY_LOWER_A, 0, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
}

/// `TurnDuplicateDownEventsToRepeats`.
#[test]
fn turn_duplicate_down_events_to_repeats() {
    let mut r = Rig::default();

    // Press KeyA
    r.send(101, true, CODE_KEY_A, KEY_LOWER_A, 0, 0);
    assert_eq!(r.records.len(), 1);
    r.records.clear();

    // Another KeyA down events, which usually means a repeated event.
    r.send(102, true, CODE_KEY_A, KEY_LOWER_A, 0, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Repeat);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), Some("a"));
    assert!(!record.synthesized);
    r.records.clear();

    // Release KeyA
    r.send(103, false, CODE_KEY_A, KEY_LOWER_Q, 0, 0);
    assert_eq!(r.records.len(), 1);
}

/// `IgnoreAbruptUpEvent`.
#[test]
fn ignore_abrupt_up_event() {
    let mut r = Rig::default();

    // Release KeyA before it was even pressed.
    r.send(103, false, CODE_KEY_A, KEY_LOWER_Q, 0, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.physical, 0);
    assert_eq!(record.logical, 0);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
}

/// Test if missed modifier keys can be detected and synthesized with state
/// information upon events that are for this modifier key.
///
/// `SynthesizeForDesyncPressingStateOnSelfEvents`.
#[test]
fn synthesize_for_desync_pressing_state_on_self_events() {
    let mut r = Rig::default();

    // Test 1: synthesize key down.

    // A key down of control left is missed.
    let mut state: u32 = state::CONTROL;

    // Send a ControlLeft up
    r.send(101, false, CODE_CONTROL_LEFT, KEY_CONTROL_L, state, 0);
    assert_eq!(r.records.len(), 2);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_CONTROL_LEFT);
    assert_eq!(record.logical, LOGICAL_CONTROL_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[1];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_CONTROL_LEFT);
    assert_eq!(record.logical, LOGICAL_CONTROL_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Test 2: synthesize key up.

    // Send a ControlLeft down.
    state = 0;
    r.send(102, true, CODE_CONTROL_LEFT, KEY_CONTROL_L, state, 0);
    assert_eq!(r.records.len(), 1);
    r.records.clear();

    // A key up of control left is missed.
    state = 0;

    // Send another ControlLeft down
    r.send(103, true, CODE_CONTROL_LEFT, KEY_CONTROL_L, state, 0);
    assert_eq!(r.records.len(), 2);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(103000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_CONTROL_LEFT);
    assert_eq!(record.logical, LOGICAL_CONTROL_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[1];
    assert_eq!(record.timestamp, Duration::from_micros(103000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_CONTROL_LEFT);
    assert_eq!(record.logical, LOGICAL_CONTROL_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Send a ControlLeft up to clear up state.
    state = state::CONTROL;
    r.send(104, false, CODE_CONTROL_LEFT, KEY_CONTROL_L, state, 0);
    assert_eq!(r.records.len(), 1);
    r.records.clear();

    // Test 3: synthesize by right modifier.

    // A key down of control right is missed.
    state = state::CONTROL;

    // Send a ControlRight up.
    r.send(105, false, CODE_CONTROL_RIGHT, KEY_CONTROL_R, state, 0);

    // A ControlLeft down is synthesized, with an empty event.
    // Reason: The ControlLeft down is synthesized to synchronize the state
    // showing Control as pressed. The ControlRight event is ignored because
    // the event is considered a duplicate up event.
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(105000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_CONTROL_LEFT);
    assert_eq!(record.logical, LOGICAL_CONTROL_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
}

/// Test if missed modifier keys can be detected and synthesized with state
/// information upon events that are not for this modifier key.
///
/// `SynthesizeForDesyncPressingStateOnNonSelfEvents`.
#[test]
fn synthesize_for_desync_pressing_state_on_non_self_events() {
    let mut r = Rig::default();

    // A key down of control left is missed.
    let mut state: u32 = state::CONTROL;

    // Send a normal event (KeyA down)
    r.send(101, true, CODE_KEY_A, KEY_LOWER_A, state, 0);
    assert_eq!(r.records.len(), 2);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_CONTROL_LEFT);
    assert_eq!(record.logical, LOGICAL_CONTROL_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[1];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), Some("a"));
    assert!(!record.synthesized);
    r.records.clear();

    // A key up of control left is missed.
    state = 0;

    // Send a normal event (KeyA up)
    r.send(102, false, CODE_KEY_A, KEY_A, state, 0);
    assert_eq!(r.records.len(), 2);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_CONTROL_LEFT);
    assert_eq!(record.logical, LOGICAL_CONTROL_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[1];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Test non-default key mapping.

    // Press a key with physical CapsLock and logical ControlLeft.
    state = 0;
    r.send(101, true, CODE_CAPS_LOCK, KEY_CONTROL_L, state, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_CAPS_LOCK);
    assert_eq!(record.logical, LOGICAL_CONTROL_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // The key up of the control left press is missed.
    state = 0;

    // Send a normal event (KeyA down).
    r.send(102, true, CODE_KEY_A, KEY_A, state, 0);

    // The synthesized event should have physical CapsLock and logical
    // ControlLeft.
    assert_eq!(r.records.len(), 2);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_CAPS_LOCK);
    assert_eq!(record.logical, LOGICAL_CONTROL_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[1];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), Some("A"));
    assert!(!record.synthesized);
}

/// Test if missed modifier keys can be detected and synthesized with state
/// information upon events that do not have the standard key mapping.
///
/// `SynthesizeForDesyncPressingStateOnRemappedEvents`.
#[test]
fn synthesize_for_desync_pressing_state_on_remapped_events() {
    let mut r = Rig::default();

    // Press a key with physical CapsLock and logical ControlLeft.
    let mut state: u32 = 0;
    r.send(101, true, CODE_CAPS_LOCK, KEY_CONTROL_L, state, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_CAPS_LOCK);
    assert_eq!(record.logical, LOGICAL_CONTROL_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // The key up of the control left press is missed.
    state = 0;

    // Send a normal event (KeyA down).
    r.send(102, true, CODE_KEY_A, KEY_A, state, 0);

    // The synthesized event should have physical CapsLock and logical
    // ControlLeft.
    assert_eq!(r.records.len(), 2);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_CAPS_LOCK);
    assert_eq!(record.logical, LOGICAL_CONTROL_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[1];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), Some("A"));
    assert!(!record.synthesized);
}

/// Test if missed lock keys can be detected and synthesized with state
/// information upon events that are not for this modifier key.
///
/// `SynthesizeForDesyncLockModeOnNonSelfEvents`.
#[test]
fn synthesize_for_desync_lock_mode_on_non_self_events() {
    let mut r = Rig::default();

    // The NumLock is desynchronized by being enabled.
    let mut state: u32 = state::MOD2;

    // Send a normal event
    r.send(101, true, CODE_KEY_A, KEY_LOWER_A, state, 0);
    assert_eq!(r.records.len(), 2);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[1];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), Some("a"));
    assert!(!record.synthesized);
    r.records.clear();

    // The NumLock is desynchronized by being disabled.
    state = 0;

    // Release key A
    r.send(102, false, CODE_KEY_A, KEY_A, state, 0);
    assert_eq!(r.records.len(), 4);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[1];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[2];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[3];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_KEY_A);
    assert_eq!(record.logical, LOGICAL_KEY_A);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // Release NumLock. Since the previous event should have synthesized NumLock
    // to be released, this should result in only an empty event.
    r.send(103, false, CODE_NUM_LOCK, KEY_NUM_LOCK, state, 0);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.physical, 0);
    assert_eq!(record.logical, 0);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
}

/// Test if missed lock keys can be detected and synthesized with state
/// information upon events that are for this modifier key.
///
/// `SynthesizeForDesyncLockModeOnSelfEvents`.
#[test]
fn synthesize_for_desync_lock_mode_on_self_events() {
    let mut r = Rig::default();

    // The NumLock is desynchronized by being enabled.
    let mut state: u32 = state::MOD2;

    // NumLock down
    r.send(101, true, CODE_NUM_LOCK, KEY_NUM_LOCK, state, 0);
    assert_eq!(r.records.len(), 3);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[1];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[2];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
    r.records.clear();

    // The NumLock is desynchronized by being enabled in a press event.
    state = state::MOD2;

    // NumLock up
    r.send(102, true, CODE_NUM_LOCK, KEY_NUM_LOCK, state, 0);
    assert_eq!(r.records.len(), 4);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[1];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[2];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[3];
    assert_eq!(record.timestamp, Duration::from_micros(102000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(!record.synthesized);
}

/// Ensures that even if the primary event is ignored (due to duplicate
/// key up or down events), key synthesization is still performed.
///
/// `SynthesizationOccursOnIgnoredEvents`.
#[test]
fn synthesization_occurs_on_ignored_events() {
    let mut r = Rig::default();

    // The NumLock is desynchronized by being enabled, and Control is pressed.
    let state: u32 = state::MOD2 | state::CONTROL;

    // Send a KeyA up event, which will be ignored.
    r.send(101, false, CODE_KEY_A, KEY_LOWER_A, state, 0);
    assert_eq!(r.records.len(), 2);
    let record = &r.records[0];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_NUM_LOCK);
    assert_eq!(record.logical, LOGICAL_NUM_LOCK);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
    let record = &r.records[1];
    assert_eq!(record.timestamp, Duration::from_micros(101000));
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_CONTROL_LEFT);
    assert_eq!(record.logical, LOGICAL_CONTROL_LEFT);
    assert_eq!(record.character.as_deref(), None);
    assert!(record.synthesized);
}

/// This test case occurs when the following two cases collide:
///
/// 1. When holding shift, AltRight key gives logical GDK_KEY_Meta_R with the
///    state bitmask still MOD3 (Alt).
/// 2. When holding AltRight, ShiftLeft key gives logical GDK_KEY_ISO_Next_Group
///    with the state bitmask RESERVED_14.
///
/// The resulting event sequence is not perfectly ideal: it had to synthesize
/// AltLeft down because the physical AltRight key corresponds to logical
/// MetaRight at the moment.
///
/// `HandlesShiftAltVersusGroupNext`.
#[test]
fn handles_shift_alt_versus_group_next() {
    let mut r = Rig::default();

    // A convenient shorthand to simulate events.
    r.next(true, CODE_SHIFT_LEFT, KEY_SHIFT_L, 1 << 25);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_SHIFT_LEFT);
    assert_eq!(record.logical, LOGICAL_SHIFT_LEFT);
    assert!(!record.synthesized);
    r.next(true, CODE_ALT_RIGHT, KEY_META_R, state::SHIFT | 1 << 25);
    assert_eq!(r.records.len(), 2);
    let record = &r.records[1];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_ALT_RIGHT);
    assert_eq!(record.logical, LOGICAL_META_RIGHT);
    assert!(!record.synthesized);
    r.next(
        false,
        CODE_SHIFT_LEFT,
        KEY_ISO_NEXT_GROUP,
        state::SHIFT | state::MOD1 | 1 << 25,
    );
    assert_eq!(r.records.len(), 5);
    let record = &r.records[2];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_ALT_LEFT);
    assert_eq!(record.logical, LOGICAL_ALT_LEFT);
    assert!(record.synthesized);
    let record = &r.records[3];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_ALT_RIGHT);
    assert_eq!(record.logical, LOGICAL_META_RIGHT);
    assert!(record.synthesized);
    let record = &r.records[4];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_SHIFT_LEFT);
    assert_eq!(record.logical, LOGICAL_SHIFT_LEFT);
    assert!(!record.synthesized);
    r.next(
        true,
        CODE_SHIFT_LEFT,
        KEY_ISO_NEXT_GROUP,
        state::MOD1 | 1 << 25,
    );
    assert_eq!(r.records.len(), 6);
    let record = &r.records[5];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_SHIFT_LEFT);
    assert_eq!(record.logical, LOGICAL_GROUP_NEXT);
    assert!(!record.synthesized);
    r.next(
        false,
        CODE_ALT_RIGHT,
        KEY_ISO_LEVEL3_SHIFT,
        state::MOD1 | 1 << 13 | 1 << 25,
    );
    assert_eq!(r.records.len(), 7);
    let record = &r.records[6];
    assert_eq!(record.physical, 0);
    assert_eq!(record.logical, 0);
    r.next(false, CODE_SHIFT_LEFT, KEY_SHIFT_L, 1 << 13 | 1 << 25);
    assert_eq!(r.records.len(), 9);
    let record = &r.records[7];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_ALT_LEFT);
    assert_eq!(record.logical, LOGICAL_ALT_LEFT);
    assert!(record.synthesized);
    let record = &r.records[8];
    assert_eq!(record.kind, KeyEventType::Up);
    assert_eq!(record.physical, PHYSICAL_SHIFT_LEFT);
    assert_eq!(record.logical, LOGICAL_GROUP_NEXT);
    assert!(!record.synthesized);
}

/// Shift + AltLeft results in GDK event whose keyval is MetaLeft but whose
/// keycode is either AltLeft or Shift keycode (depending on which one was
/// released last). The physical key is usually deduced from the keycode, but in
/// this case (Shift + AltLeft) a correction is needed otherwise the physical
/// key won't be the MetaLeft one.
/// Regression test for https://github.com/flutter/flutter/issues/96082
///
/// `HandlesShiftAltLeftIsMetaLeft`.
#[test]
fn handles_shift_alt_left_is_meta_left() {
    let mut r = Rig::default();

    // A convenient shorthand to simulate events.

    // ShiftLeft + AltLeft
    r.next(true, CODE_SHIFT_LEFT, KEY_SHIFT_L, 1 << 25);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_SHIFT_LEFT);
    assert_eq!(record.logical, LOGICAL_SHIFT_LEFT);
    assert!(!record.synthesized);
    r.next(true, CODE_ALT_LEFT, KEY_META_L, state::SHIFT | 1 << 25);
    assert_eq!(r.records.len(), 2);
    let record = &r.records[1];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_META_LEFT);
    assert_eq!(record.logical, LOGICAL_META_LEFT);
    assert!(!record.synthesized);
    r.next(false, CODE_ALT_LEFT, KEY_META_L, 1 << 13 | 1 << 25);
    r.next(false, CODE_SHIFT_LEFT, KEY_SHIFT_L, 1 << 25);
    r.records.clear();

    // ShiftRight + AltLeft
    r.next(true, CODE_SHIFT_RIGHT, KEY_SHIFT_R, 1 << 25);
    assert_eq!(r.records.len(), 1);
    let record = &r.records[0];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_SHIFT_RIGHT);
    assert_eq!(record.logical, LOGICAL_SHIFT_RIGHT);
    assert!(!record.synthesized);
    r.next(true, CODE_ALT_LEFT, KEY_META_L, state::SHIFT | 1 << 25);
    assert_eq!(r.records.len(), 2);
    let record = &r.records[1];
    assert_eq!(record.kind, KeyEventType::Down);
    assert_eq!(record.physical, PHYSICAL_META_LEFT);
    assert_eq!(record.logical, LOGICAL_META_LEFT);
    assert!(!record.synthesized);
}
