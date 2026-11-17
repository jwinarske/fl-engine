//! Fuzzing the input boundary's state (brief M4 §15): the text input
//! handler given editing states, keys and an input method's signals no
//! framework or keymap would send, and the key and pointer paths given
//! keys and positions no device would. Nothing the framework, a keymap or
//! a device reports is trusted to be in range. After every step the text's
//! selection and composing range are inside the text, and what the
//! handler would tell the framework says the same. Deterministic, a seeded
//! generator, and pure Rust. A panic is a bug; the sanitizer lanes run it
//! under ASan.
//!
//!     FL_FUZZ_ITERS=20000 cargo test -p fl-input --test fuzz -- --ignored
#![allow(clippy::unwrap_used, clippy::panic)]

use std::time::Duration;

use fl_input::pointer::{At, Mouse, Scroll, Scrolling, TouchAction, Touches};
use fl_input::text::{Direct, ImEvent, InputType, TextInput, Update};
use fl_input::xkb::{Keys, XkbKey};
use fl_input::PointerDeviceKind;

/// xorshift64*: the same inputs on every run and every host.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    /// An offset: small, at an extreme, or anything.
    fn offset(&mut self) -> i64 {
        match self.below(6) {
            0 => -1,
            1 => [i64::MIN, i64::MAX, -2, 0][self.below(4)],
            2 => self.next() as i64,
            _ => self.below(12) as i64,
        }
    }

    /// A text of characters one, two and four bytes wide, and of one and
    /// two UTF-16 units.
    fn text(&mut self) -> String {
        const PIECES: [&str; 8] = ["a", "é", "😄", "\n", " ", "ぴ", "🧐", "\u{301}"];
        (0..self.below(7)).map(|_| PIECES[self.below(8)]).collect()
    }

    fn number(&mut self) -> f64 {
        match self.below(8) {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => f64::NEG_INFINITY,
            3 => f64::MAX,
            4 => -0.0,
            _ => (self.next() % 4000) as f64 - 2000.0,
        }
    }

    fn key(&mut self) -> XkbKey {
        const KEYVALS: [u32; 12] = [
            0x61, 0x41, 0xff0d, 0xff8d, 0xfe34, 0xff50, 0xff57, 0xff95, 0xff9c, 0xffe1, 0xfe51, 0,
        ];
        let keyval = match self.below(3) {
            0 => self.next() as u32,
            _ => KEYVALS[self.below(12)],
        };
        XkbKey {
            time: Duration::from_micros(self.next() >> 20),
            pressed: self.below(4) != 0,
            keycode: self.next() as u16,
            keyval,
            unicode: match self.below(4) {
                0 => self.next() as u32,
                1 => 0xd800 + (self.next() % 0x800) as u32,
                _ => keyval,
            },
            state: self.next() as u32,
            group: self.next() as u8,
        }
    }
}

/// The text's ranges are inside the text, whatever was done to it.
fn check(input: &TextInput) {
    let model = input.model();
    let units = model.text().encode_utf16().count();
    // A lone surrogate reads back as one replacement character, one unit.
    assert_eq!(model.text_range().end(), units, "{model:?}");
    assert!(model.text_range().contains(&model.selection()), "{model:?}");
    assert!(
        model.text_range().contains(&model.composing_range()),
        "{model:?}"
    );
    assert!(model.cursor_offset() <= model.text().len());
}

/// What the framework is told agrees with the text: offsets inside it.
fn check_updates(updates: &[Update]) {
    for u in updates {
        let (text, ranges) = match u {
            Update::State { state, .. } => (
                state.text.clone(),
                [
                    state.selection_base,
                    state.selection_extent,
                    state.composing_base,
                    state.composing_extent,
                ],
            ),
            Update::Delta { delta, .. } => {
                assert!(delta.delta_start >= -1 && delta.delta_end >= delta.delta_start);
                continue;
            }
            // The field's action, whatever the framework named it.
            Update::Action { .. } => continue,
        };
        let units = text.encode_utf16().count() as i64;
        assert!(
            (0..=units).contains(&ranges[0]) && (0..=units).contains(&ranges[1]),
            "{u:?}"
        );
        assert!(
            (-1..=units).contains(&ranges[2]) && (-1..=units).contains(&ranges[3]),
            "{u:?}"
        );
    }
}

#[test]
#[ignore = "fuzz: FL_FUZZ_ITERS=20000 cargo test -p fl-input --test fuzz -- --ignored"]
fn text_input_keeps_its_ranges_inside_its_text() {
    let iters: usize = std::env::var("FL_FUZZ_ITERS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(2000);
    let mut rng = Rng(0x5eed_f1e1_0000_0005);
    let mut told = 0usize;
    for _ in 0..iters {
        let mut input = TextInput::new(Box::new(Direct));
        for _ in 0..1 + rng.below(40) {
            let mut updates = Vec::new();
            match rng.below(14) {
                0 => input.set_client(rng.offset()),
                1 => {
                    let action = [
                        None,
                        Some("TextInputAction.newline"),
                        Some("TextInputAction.done"),
                        Some(""),
                    ][rng.below(4)];
                    let kind =
                        [InputType::Text, InputType::Multiline, InputType::None][rng.below(3)];
                    input.configure(action, rng.below(2) == 0, kind);
                }
                2 | 3 => {
                    let text = rng.text();
                    input.set_editing_state(
                        &text,
                        rng.offset(),
                        rng.offset(),
                        rng.offset(),
                        rng.offset(),
                    );
                }
                4 => input.clear_client(),
                5 => input.im_event(&ImEvent::PreeditStart, &mut updates),
                6 => {
                    let event = ImEvent::PreeditChanged {
                        text: rng.text(),
                        cursor: rng.offset().unsigned_abs() as usize,
                    };
                    input.im_event(&event, &mut updates);
                }
                7 => input.im_event(&ImEvent::PreeditEnd, &mut updates),
                8 => input.im_event(&ImEvent::Commit(rng.text()), &mut updates),
                9 => {
                    let event = ImEvent::DeleteSurrounding {
                        offset: rng.offset() as i32,
                        count: rng.offset() as i32,
                    };
                    input.im_event(&event, &mut updates);
                }
                10 => {
                    let mut t = [0.0; 16];
                    t.iter_mut().for_each(|v| *v = rng.number());
                    input.set_editable_transform(t);
                    input.set_marked_text_rect(
                        rng.number(),
                        rng.number(),
                        rng.number(),
                        rng.number(),
                    );
                }
                11 => input.reset(),
                _ => {
                    input.filter_key(&rng.key(), &mut updates);
                }
            }
            check(&input);
            check_updates(&updates);
            told += updates.len();
        }
    }
    println!("{iters} fields edited; the framework told {told} times");
    assert!(told > iters, "{told}");
}

/// Keys no keymap gives and pointers no device reports: the key path and
/// the three managers answer each, and what they remember stays bounded.
#[test]
#[ignore = "fuzz: FL_FUZZ_ITERS=20000 cargo test -p fl-input --test fuzz -- --ignored"]
fn keys_and_pointers_answer_anything() {
    let iters: usize = std::env::var("FL_FUZZ_ITERS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(2000);
    let mut rng = Rng(0x5eed_f1e1_0000_0006);
    let mut keys = Keys::new();
    let (mut mouse, mut scrolling, mut touches) =
        (Mouse::new(0), Scrolling::new(0), Touches::new(0));
    let mut out = Vec::new();
    let mut events = 0usize;
    for _ in 0..iters * 20 {
        let time = Duration::from_micros(rng.next() >> 16);
        let (x, y) = (rng.number(), rng.number());
        match rng.below(9) {
            0 | 1 => {
                let key = rng.key();
                let seed = rng.next();
                let event = keys.handle(&key, |code, group, level| {
                    (seed ^ u64::from(code) ^ u64::from(group) ^ u64::from(level)) as u32
                });
                assert!(!event.data.is_empty());
                events += event.data.len();
                if rng.below(50) == 0 {
                    keys.sync_modifiers(rng.next() as u32, time);
                }
                if rng.below(200) == 0 {
                    keys.keymap_changed();
                }
            }
            2..=4 => {
                let kinds = [
                    PointerDeviceKind::Mouse,
                    PointerDeviceKind::Stylus,
                    PointerDeviceKind::InvertedStylus,
                    PointerDeviceKind::Trackpad,
                    PointerDeviceKind::Touch,
                ];
                let at = At {
                    time,
                    kind: kinds[rng.below(5)],
                    x,
                    y,
                    rotation: rng.number(),
                    pressure: rng.number(),
                };
                let button = rng.next() as u32 % 12;
                match rng.below(5) {
                    0 => drop(mouse.button_press(&at, button, &mut out)),
                    1 => drop(mouse.button_release(&at, button, &mut out)),
                    2 => mouse.motion(&at, &mut out),
                    3 => mouse.enter(&at, &mut out),
                    _ => mouse.leave(&at, &mut out),
                }
            }
            5 | 6 => {
                let scroll = Scroll {
                    time,
                    x,
                    y,
                    delta_x: rng.number(),
                    delta_y: rng.number(),
                    touchpad: rng.below(2) == 0,
                    stop: rng.below(4) == 0,
                };
                match rng.below(8) {
                    0 => scrolling.zoom_begin(time, &mut out),
                    1 => scrolling.zoom_update(time, rng.number(), &mut out),
                    2 => scrolling.zoom_end(time, &mut out),
                    3 => scrolling.rotation_begin(time, &mut out),
                    4 => scrolling.rotation_update(time, rng.number(), &mut out),
                    5 => scrolling.rotation_end(time, &mut out),
                    6 => scrolling.set_last_mouse_position(x, y),
                    _ => scrolling.scroll(&scroll, rng.number(), &mut out),
                }
            }
            _ => {
                let action = [
                    TouchAction::Begin,
                    TouchAction::Update,
                    TouchAction::End,
                    TouchAction::Cancel,
                ][rng.below(4)];
                // More fingers than there are ids, and numbers past any.
                let number = match rng.below(4) {
                    0 => rng.next() as u32,
                    _ => rng.below(300) as u32,
                };
                touches.touch(time, action, number, x, y, &mut out);
            }
        }
        // The keys held are those of a keyboard, not of every code seen.
        assert!(keys.pressed().count() <= 1 << 16);
        for e in out.drain(..) {
            assert!(e.device >= 0, "{e:?}");
            if e.device_kind == PointerDeviceKind::Touch && e.device >> 28 == 2 {
                assert!((e.device & 0x0fff_ffff) < 128, "{e:?}");
            }
            events += 1;
        }
    }
    println!("{} steps, {events} events", iters * 20);
    assert!(events > iters);
}
