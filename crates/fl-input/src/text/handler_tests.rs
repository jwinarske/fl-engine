//! `fl_text_input_handler_test.cc`'s cases: what the handler tells the
//! framework for a key or an input method's signal. The cases that are
//! GTK's alone (input purposes and hints, the widget's coordinates) have
//! nothing here to hold.

use std::sync::{Arc, Mutex};

use super::*;

const GDK_SHIFT_MASK: u32 = state::SHIFT;

fn press(keyval: u32, state: u32) -> XkbKey {
    XkbKey {
        pressed: true,
        keyval,
        state,
        ..XkbKey::default()
    }
}

/// An input method that takes no key, as the tests' mock GTK's.
struct Deaf;

impl InputMethod for Deaf {
    fn filter_key(&mut self, _key: &XkbKey, _events: &mut Vec<ImEvent>) -> bool {
        false
    }
}

fn handler() -> TextInput {
    TextInput::new(Box::new(Deaf))
}

fn client(input: &mut TextInput, input_type: InputType, action: &str, deltas: bool) {
    input.set_client(1);
    input.configure(Some(action), deltas, input_type);
}

fn state(client: i64, text: &str, base: i64, extent: i64, composing: (i64, i64)) -> Update {
    Update::State {
        client,
        state: EditingState {
            text: text.to_owned(),
            selection_base: base,
            selection_extent: extent,
            composing_base: composing.0,
            composing_extent: composing.1,
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn delta(old: &str, text: &str, start: i64, end: i64, sel: i64, composing: (i64, i64)) -> Update {
    Update::Delta {
        client: 1,
        delta: EditingDelta {
            old_text: old.to_owned(),
            delta_text: text.to_owned(),
            delta_start: start,
            delta_end: end,
            selection_base: sel,
            selection_extent: sel,
            composing_base: composing.0,
            composing_extent: composing.1,
        },
    }
}

fn send(input: &mut TextInput, keyval: u32, state: u32) -> Vec<Update> {
    let mut updates = Vec::new();
    input.filter_key(&press(keyval, state), &mut updates);
    updates
}

fn signal(input: &mut TextInput, events: &[ImEvent]) -> Vec<Update> {
    let mut updates = Vec::new();
    for e in events {
        input.im_event(e, &mut updates);
    }
    updates
}

fn preedit(text: &str, cursor: usize) -> ImEvent {
    ImEvent::PreeditChanged {
        text: text.to_owned(),
        cursor,
    }
}

fn commit(text: &str) -> ImEvent {
    ImEvent::Commit(text.to_owned())
}

/// `SetClient`, `ClearClient`: with no client a key is not text input's.
#[test]
fn a_key_is_filtered_only_for_a_client() {
    let mut input = TextInput::new(Box::new(Direct));
    let a = XkbKey {
        pressed: true,
        keyval: 0x61,
        unicode: 0x61,
        ..XkbKey::default()
    };
    let mut updates = Vec::new();
    assert!(!input.filter_key(&a, &mut updates));
    assert!(updates.is_empty());
    input.set_client(7);
    assert!(input.filter_key(&a, &mut updates));
    assert_eq!(updates, [state(7, "a", 1, 1, (-1, -1))]);
    input.clear_client();
    assert_eq!(input.client(), CLIENT_UNSET);
    assert!(!input.filter_key(&a, &mut updates));
}

/// `Show`, `Hide`, `TextInputTypeNone`: the input method is told a field
/// wants input, unless the field brings its own keyboard.
#[test]
fn show_and_hide_reach_the_input_method() {
    #[derive(Clone, Default)]
    struct Focus(Arc<Mutex<Vec<&'static str>>>);
    impl InputMethod for Focus {
        fn filter_key(&mut self, _: &XkbKey, _: &mut Vec<ImEvent>) -> bool {
            false
        }
        fn focus_in(&mut self) {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).push("in");
        }
        fn focus_out(&mut self) {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).push("out");
        }
    }
    let focus = Focus::default();
    let seen = Arc::clone(&focus.0);
    let take = move || std::mem::take(&mut *seen.lock().unwrap_or_else(|e| e.into_inner()));
    let mut input = TextInput::new(Box::new(focus));
    assert_eq!(take(), ["out"], "as the embedder starts");
    input.show();
    assert_eq!(take(), ["in"]);
    input.hide();
    assert_eq!(take(), ["out"]);
    input.set_client(1);
    input.configure(Some("TextInputAction.none"), false, InputType::None);
    input.show();
    assert_eq!(take(), ["out"]);
}

/// `PerformAction`: Return in a multiline field whose action is newline
/// adds the line, then performs the action.
#[test]
fn perform_action() {
    let mut input = handler();
    client(
        &mut input,
        InputType::Multiline,
        "TextInputAction.newline",
        false,
    );
    input.set_editing_state("Flutter", 7, 7, -1, -1);
    assert_eq!(
        send(&mut input, keysym::RETURN, 0),
        [
            state(1, "Flutter\n", 8, 8, (-1, -1)),
            Update::Action {
                client: 1,
                action: "TextInputAction.newline".to_owned()
            },
        ]
    );
}

/// `MultilineWithSendAction` (flutter/flutter#125879): with another
/// action, Return performs it and adds nothing.
#[test]
fn multiline_with_send_action() {
    let mut input = handler();
    client(
        &mut input,
        InputType::Multiline,
        "TextInputAction.send",
        false,
    );
    input.set_editing_state("Flutter", 7, 7, -1, -1);
    assert_eq!(
        send(&mut input, keysym::RETURN, 0),
        [Update::Action {
            client: 1,
            action: "TextInputAction.send".to_owned()
        }]
    );
    assert_eq!(input.model().text(), "Flutter");
}

/// `MoveCursor`.
#[test]
fn move_cursor() {
    let mut input = handler();
    client(&mut input, InputType::Text, "TextInputAction.none", false);
    input.set_editing_state("Flutter", 4, 4, -1, -1);
    assert_eq!(
        send(&mut input, keysym::HOME, 0),
        [state(1, "Flutter", 0, 0, (-1, -1))]
    );
    assert_eq!(
        send(&mut input, keysym::END, 0),
        [state(1, "Flutter", 7, 7, (-1, -1))]
    );
    // There already: nothing to say.
    assert_eq!(send(&mut input, keysym::KP_END, 0), []);
}

/// `Select`.
#[test]
fn select() {
    let mut input = handler();
    client(&mut input, InputType::Text, "TextInputAction.none", false);
    input.set_editing_state("Flutter", 4, 4, -1, -1);
    assert_eq!(
        send(&mut input, keysym::END, GDK_SHIFT_MASK),
        [state(1, "Flutter", 4, 7, (-1, -1))]
    );
    assert_eq!(
        send(&mut input, keysym::HOME, GDK_SHIFT_MASK),
        [state(1, "Flutter", 4, 0, (-1, -1))]
    );
}

/// `Composing`: an input method's signals with no client still tell the
/// framework, as the embedder's do.
#[test]
fn composing() {
    let mut input = handler();
    assert_eq!(
        signal(
            &mut input,
            &[
                ImEvent::PreeditStart,
                preedit("Flutter", 0),
                commit("engine"),
                ImEvent::PreeditEnd,
            ]
        ),
        [
            state(-1, "Flutter", 0, 0, (0, 7)),
            state(-1, "engine", 6, 6, (-1, -1)),
            state(-1, "engine", 6, 6, (-1, -1)),
        ]
    );
}

/// `SurroundingText`.
#[test]
fn surrounding_text() {
    let mut input = handler();
    client(&mut input, InputType::Text, "TextInputAction.none", false);
    input.set_editing_state("Flutter", 3, 3, -1, -1);
    assert_eq!(input.surrounding(), ("Flutter".to_owned(), 3));
    assert_eq!(
        signal(
            &mut input,
            &[ImEvent::DeleteSurrounding {
                offset: 1,
                count: 2
            }]
        ),
        [state(1, "Flutr", 3, 3, (-1, -1))]
    );
}

/// `SetMarkedTextRect`: the composing text's far corner, through
/// `EditableText`'s transform, is where the input method is told the
/// caret is; and only while composing.
#[test]
fn set_marked_text_rect() {
    #[derive(Clone, Default)]
    struct Where(Arc<Mutex<Vec<(i32, i32)>>>);
    impl InputMethod for Where {
        fn filter_key(&mut self, _: &XkbKey, _: &mut Vec<ImEvent>) -> bool {
            false
        }
        fn set_cursor_location(&mut self, x: i32, y: i32) {
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((x, y));
        }
    }
    let place = Where::default();
    let seen = Arc::clone(&place.0);
    let take = move || std::mem::take(&mut *seen.lock().unwrap_or_else(|e| e.into_inner()));
    let mut input = TextInput::new(Box::new(place));
    let transform = [
        1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
    ];
    input.set_editable_transform(transform);
    input.set_marked_text_rect(1.0, 2.0, 3.0, 4.0);
    assert_eq!(take(), [], "not composing");
    signal(&mut input, &[ImEvent::PreeditStart]);
    input.set_editable_transform(transform);
    // x: 1*1 + 2*5 + 13 + 3; y: 1*2 + 2*6 + 14 + 4.
    assert_eq!(take(), [(27, 32)]);
    input.set_marked_text_rect(1.0, 2.0, 3.0, f64::INFINITY);
    assert_eq!(take(), [(27, i32::MAX)], "a rect no int holds saturates");
}

/// `TextEditingDelta`: the delta the embedder sends for Home.
#[test]
fn text_editing_delta() {
    let mut input = handler();
    client(&mut input, InputType::Text, "TextInputAction.none", true);
    input.set_editing_state("Flutter", 7, 7, -1, -1);
    assert_eq!(
        send(&mut input, keysym::HOME, 0),
        [delta("Flutter", "Flutter", 7, 7, 0, (-1, -1))]
    );
}

/// `ComposingDelta`.
#[test]
fn composing_delta() {
    let mut input = handler();
    client(&mut input, InputType::Text, "TextInputAction.none", true);
    assert_eq!(
        signal(
            &mut input,
            &[
                ImEvent::PreeditStart,
                preedit("Flutter ", 8),
                commit("Flutter engine"),
                ImEvent::PreeditEnd,
            ]
        ),
        [
            delta("", "Flutter ", 0, 0, 8, (0, 8)),
            delta("Flutter ", "Flutter engine", 0, 8, 14, (-1, -1)),
            delta("Flutter engine", "", -1, -1, 14, (-1, -1)),
        ]
    );
}

/// `NonComposingDelta`.
#[test]
fn non_composing_delta() {
    let mut input = handler();
    client(&mut input, InputType::Text, "TextInputAction.none", true);
    let mut old = String::new();
    for (i, c) in "Flutter".chars().enumerate() {
        let i = i as i64;
        assert_eq!(
            signal(&mut input, &[commit(&c.to_string())]),
            [delta(&old, &c.to_string(), i, i, i + 1, (-1, -1))]
        );
        old.push(c);
    }
}

/// `UpdateConfig`: a field's configuration changed takes effect.
#[test]
fn update_config() {
    let mut input = handler();
    client(&mut input, InputType::Text, "TextInputAction.none", false);
    input.configure(Some("TextInputAction.done"), false, InputType::Text);
    assert_eq!(
        send(&mut input, keysym::KP_ENTER, 0),
        [Update::Action {
            client: 1,
            action: "TextInputAction.done".to_owned()
        }]
    );
    // No action named: none to perform.
    input.configure(None, false, InputType::Text);
    assert_eq!(send(&mut input, keysym::ISO_ENTER, 0), []);
}

/// What the framework sends is not trusted (brief M4 §15): offsets that
/// are not inside the text are refused, and nothing indexes with one.
#[test]
fn offsets_outside_the_text_are_refused() {
    let mut input = TextInput::new(Box::new(Direct));
    client(&mut input, InputType::Text, "TextInputAction.none", false);
    for (base, extent, composing) in [
        (9, 9, (-1, -1)),
        (-5, 2, (-1, -1)),
        (i64::MAX, i64::MIN, (i64::MIN, i64::MAX)),
        (1, 1, (0, 99)),
        (2, 2, (-7, 1)),
    ] {
        input.set_editing_state("a😄", base, extent, composing.0, composing.1);
        let model = input.model();
        assert!(
            model.text_range().contains(&model.selection()),
            "{base} {extent}"
        );
        assert!(!model.composing());
        // Typing after it works from wherever the caret was left.
        let x = XkbKey {
            pressed: true,
            keyval: 0x78,
            unicode: 0x78,
            ..XkbKey::default()
        };
        let mut updates = Vec::new();
        assert!(input.filter_key(&x, &mut updates));
        assert_eq!(updates.len(), 1);
    }
    // A selection inside the text is taken, in UTF-16 units: after the
    // emoji's two.
    input.set_editing_state("a😄", 3, 3, -1, -1);
    assert_eq!(input.model().selection(), TextRange::at(3));
}

/// A key types its character unless it is a release, a shortcut's key, or
/// no character at all (`GtkIMContextSimple`).
#[test]
fn a_key_types_its_character() {
    let key = |unicode, state, pressed| XkbKey {
        pressed,
        keyval: unicode,
        unicode,
        state,
        ..XkbKey::default()
    };
    assert_eq!(typed_character(&key(0x61, 0, true)), Some('a'));
    assert_eq!(
        typed_character(&key(0x41, state::SHIFT | state::LOCK, true)),
        Some('A')
    );
    assert_eq!(
        typed_character(&key(0x1f604, state::MOD2, true)),
        Some('😄')
    );
    assert_eq!(typed_character(&key(0x61, 0, false)), None, "a release");
    assert_eq!(typed_character(&key(0x76, state::CONTROL, true)), None);
    assert_eq!(typed_character(&key(0x76, state::MOD1, true)), None);
    assert_eq!(typed_character(&key(0, 0, true)), None, "no character");
    assert_eq!(typed_character(&key(0x0d, 0, true)), None, "Return");
    assert_eq!(typed_character(&key(0x7f, 0, true)), None, "Delete");
    assert_eq!(typed_character(&key(0xd800, 0, true)), None, "a surrogate");
}
