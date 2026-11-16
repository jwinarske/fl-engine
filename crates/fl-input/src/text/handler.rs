//! What becomes of a key the framework did not handle, and of what an
//! input method makes of it (brief M4 §2, D6): the Linux embedder's
//! `fl_text_input_handler.cc`, ported, with GTK's `GtkIMContext` behind
//! [`InputMethod`].
//!
//! The framework tells the handler which text field has the keyboard and
//! what is in it ([`TextInput::set_client`], [`TextInput::
//! set_editing_state`]); the handler tells the framework what typing did
//! to it ([`Update`]). Which messages those are is fl-ui's.

use super::model::{TextInputModel, TextRange};
use crate::xkb::{state, XkbKey};

/// `kClientIdUnset`: no text field has the keyboard.
pub const CLIENT_UNSET: i64 = -1;

const NEWLINE_ACTION: &str = "TextInputAction.newline";

/// Keysyms the handler acts on itself.
mod keysym {
    pub const ISO_ENTER: u32 = 0xfe34;
    pub const RETURN: u32 = 0xff0d;
    pub const HOME: u32 = 0xff50;
    pub const END: u32 = 0xff57;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const KP_HOME: u32 = 0xff95;
    pub const KP_END: u32 = 0xff9c;
}

/// `FlTextInputType`: what the handler does differently by a field's
/// `TextInputType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputType {
    #[default]
    Text,
    /// Return adds a line, if the field's action is `newline`.
    Multiline,
    /// The field brings its own keyboard: the input method is not shown.
    None,
}

/// What an input method makes of a key: GTK's `GtkIMContext` signals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImEvent {
    /// Composing began (`preedit-start`).
    PreeditStart,
    /// The text being composed is now this, the caret `cursor` UTF-16
    /// code units into it (`preedit-changed`).
    PreeditChanged { text: String, cursor: usize },
    /// Composing ended (`preedit-end`).
    PreeditEnd,
    /// Text to put at the caret (`commit`).
    Commit(String),
    /// `count` characters to delete, starting `offset` characters from
    /// the caret (`delete-surrounding`).
    DeleteSurrounding { offset: i32, count: i32 },
}

/// The platform's input method: what GTK's `GtkIMContext` is to the Linux
/// embedder. It is offered each key the framework left unhandled while a
/// text field has the keyboard, before the handler acts on the key itself.
pub trait InputMethod: Send {
    /// One key. `true` if the input method took it; what it made of it is
    /// pushed on `events`, in order.
    fn filter_key(&mut self, key: &XkbKey, events: &mut Vec<ImEvent>) -> bool;

    /// A text field wants input (`gtk_im_context_focus_in`).
    fn focus_in(&mut self) {}

    /// No text field wants input (`gtk_im_context_focus_out`).
    fn focus_out(&mut self) {}

    /// Whatever is being composed is abandoned (`gtk_im_context_reset`).
    fn reset(&mut self) {}

    /// Where the composing text ends in the view, in logical pixels, for
    /// an input method that shows a window of its own beside it
    /// (`gtk_im_context_set_cursor_location`).
    fn set_cursor_location(&mut self, _x: i32, _y: i32) {}
}

/// The character a key types with no input method between: what GTK's
/// `GtkIMContextSimple` commits for a key outside any sequence. None for a
/// release, with Control or Alt held (its `NO_TEXT_INPUT` modifiers), or
/// for a keysym with no character or a control character.
pub fn typed_character(key: &XkbKey) -> Option<char> {
    if !key.pressed || key.state & (state::CONTROL | state::MOD1) != 0 {
        return None;
    }
    char::from_u32(key.unicode).filter(|c| *c != '\0' && !c.is_control())
}

/// An input method that composes nothing: each key types its character.
#[derive(Debug, Clone, Copy, Default)]
pub struct Direct;

impl InputMethod for Direct {
    fn filter_key(&mut self, key: &XkbKey, events: &mut Vec<ImEvent>) -> bool {
        match typed_character(key) {
            Some(c) => {
                events.push(ImEvent::Commit(c.to_string()));
                true
            }
            None => false,
        }
    }
}

/// A text field's text, selection and composing range as the framework is
/// told them: UTF-16 offsets, -1 and -1 for no composing range.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditingState {
    pub text: String,
    pub selection_base: i64,
    pub selection_extent: i64,
    pub composing_base: i64,
    pub composing_extent: i64,
}

/// One change to a text field's text, for a field that asked for deltas
/// (`TextInputConfiguration.enableDeltaModel`): `delta_text` in place of
/// `delta_start..delta_end` of `old_text`, or -1 and -1 for a change of
/// selection or composing range alone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditingDelta {
    pub old_text: String,
    pub delta_text: String,
    pub delta_start: i64,
    pub delta_end: i64,
    pub selection_base: i64,
    pub selection_extent: i64,
    pub composing_base: i64,
    pub composing_extent: i64,
}

/// What the handler tells the framework.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// `TextInputClient.updateEditingState`.
    State { client: i64, state: EditingState },
    /// `TextInputClient.updateEditingStateWithDeltas`, one delta.
    Delta { client: i64, delta: EditingDelta },
    /// `TextInputClient.performAction`.
    Action { client: i64, action: String },
}

/// The engine's `TextEditingDelta` before the selection is added.
struct Change {
    old_text: String,
    delta_text: String,
    start: i64,
    end: i64,
}

impl Change {
    /// `text` in place of `range` of `old_text`.
    fn replace(old_text: String, range: TextRange, text: &str) -> Change {
        Change {
            old_text,
            delta_text: text.to_owned(),
            start: offset(range.start()),
            end: offset(range.start() + range.length()),
        }
    }

    /// No text changed.
    fn none(text: String) -> Change {
        Change {
            old_text: text,
            delta_text: String::new(),
            start: -1,
            end: -1,
        }
    }
}

fn offset(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// An offset the framework sent, as the embedder reads one into a
/// `size_t`: a negative one is past any text.
fn index(n: i64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// The text input handler: one for the engine.
pub struct TextInput {
    client: i64,
    input_action: Option<String>,
    input_type: InputType,
    enable_delta_model: bool,
    im: Box<dyn InputMethod>,
    model: TextInputModel,
    /// `EditableText`'s local coordinates to the view's, column-major.
    transform: [f64; 16],
    /// The composing text's rect, or the caret's, in `EditableText`'s
    /// coordinates: x, y, width, height.
    composing_rect: [i32; 4],
}

impl Default for TextInput {
    fn default() -> TextInput {
        TextInput::new(Box::new(Direct))
    }
}

impl TextInput {
    pub fn new(im: Box<dyn InputMethod>) -> TextInput {
        let mut im = im;
        // As the embedder starts: not wanting input.
        im.focus_out();
        TextInput {
            client: CLIENT_UNSET,
            input_action: None,
            input_type: InputType::Text,
            enable_delta_model: false,
            im,
            model: TextInputModel::new(),
            transform: [0.0; 16],
            composing_rect: [0; 4],
        }
    }

    /// Back to how it started, for a new isolate: no client, no text,
    /// nothing being composed. The input method is kept.
    pub fn reset(&mut self) {
        self.im.reset();
        self.im.focus_out();
        self.client = CLIENT_UNSET;
        self.input_action = None;
        self.input_type = InputType::Text;
        self.enable_delta_model = false;
        self.model = TextInputModel::new();
        self.transform = [0.0; 16];
        self.composing_rect = [0; 4];
    }

    /// The text field that has the keyboard, or [`CLIENT_UNSET`].
    pub fn client(&self) -> i64 {
        self.client
    }

    /// The text as the handler has it.
    pub fn model(&self) -> &TextInputModel {
        &self.model
    }

    /// `TextInput.setClient`'s first half: the field's id.
    pub fn set_client(&mut self, client: i64) {
        self.client = client;
    }

    /// `TextInput.setClient`'s second half, and `TextInput.updateConfig`.
    pub fn configure(
        &mut self,
        input_action: Option<&str>,
        enable_delta_model: bool,
        input_type: InputType,
    ) {
        self.input_action = input_action.map(str::to_owned);
        self.enable_delta_model = enable_delta_model;
        self.input_type = input_type;
    }

    /// `TextInput.show`.
    pub fn show(&mut self) {
        if self.input_type == InputType::None {
            self.hide();
        } else {
            self.im.focus_in();
        }
    }

    /// `TextInput.hide`.
    pub fn hide(&mut self) {
        self.im.focus_out();
    }

    /// `TextInput.setEditingState`. Offsets are UTF-16 code units; -1 and
    /// -1 is no selection, or no composing range. A selection that is not
    /// inside the text is refused, and the caret is at its start.
    pub fn set_editing_state(
        &mut self,
        text: &str,
        selection_base: i64,
        selection_extent: i64,
        composing_base: i64,
        composing_extent: i64,
    ) {
        self.model.set_text(text);
        // Flutter's -1/-1 is the model's 0/0.
        let (base, extent) = if selection_base == -1 && selection_extent == -1 {
            (0, 0)
        } else {
            (selection_base, selection_extent)
        };
        self.model
            .set_selection(TextRange::new(index(base), index(extent)));
        if composing_base == -1 && composing_extent == -1 {
            self.model.end_composing();
        } else {
            let range = TextRange::new(index(composing_base), index(composing_extent));
            let cursor_offset = index(base).wrapping_sub(range.start());
            self.model.set_composing_range(range, cursor_offset);
        }
    }

    /// `TextInput.clearClient`.
    pub fn clear_client(&mut self) {
        self.client = CLIENT_UNSET;
    }

    /// `TextInput.setEditableSizeAndTransform`: `EditableText`'s
    /// coordinates to the view's.
    pub fn set_editable_transform(&mut self, transform: [f64; 16]) {
        self.transform = transform;
        self.update_im_cursor_position();
    }

    /// `TextInput.setMarkedTextRect`: where the composing text is, or the
    /// caret, in `EditableText`'s coordinates.
    pub fn set_marked_text_rect(&mut self, x: f64, y: f64, width: f64, height: f64) {
        // A `GdkRectangle`'s ints; a cast from a double saturates.
        self.composing_rect = [x as i32, y as i32, width as i32, height as i32];
        self.update_im_cursor_position();
    }

    /// Tells the input method where the composing text ends, in the view
    /// (`update_im_cursor_position`).
    fn update_im_cursor_position(&mut self) {
        // Not composing: there is no such place.
        if !self.model.composing() {
            return;
        }
        let [x, y, width, height] = self.composing_rect.map(f64::from);
        let t = &self.transform;
        let vx = x * t[0] + y * t[4] + t[12] + width;
        let vy = x * t[1] + y * t[5] + t[13] + height;
        self.im.set_cursor_location(vx as i32, vy as i32);
    }

    /// A key the framework did not handle (`fl_text_input_handler_
    /// filter_keypress`). `true` if text input took it; what the framework
    /// is to be told is pushed on `updates`.
    pub fn filter_key(&mut self, key: &XkbKey, updates: &mut Vec<Update>) -> bool {
        if self.client == CLIENT_UNSET {
            return false;
        }

        let mut events = Vec::new();
        let filtered = self.im.filter_key(key, &mut events);
        for event in &events {
            self.im_event(event, updates);
        }
        if filtered {
            return true;
        }

        let text_before = self.model.text();
        let selection_before = self.model.selection();
        let mut text = text_before.clone();
        let mut do_action = false;
        let mut changed = false;
        if key.pressed {
            let shift = key.state & state::SHIFT != 0;
            match key.keyval {
                keysym::END | keysym::KP_END => {
                    changed = if shift {
                        self.model.select_to_end()
                    } else {
                        self.model.move_cursor_to_end()
                    };
                }
                keysym::RETURN | keysym::KP_ENTER | keysym::ISO_ENTER => {
                    if self.input_type == InputType::Multiline
                        && self.input_action.as_deref() == Some(NEWLINE_ACTION)
                    {
                        self.model.add_code_point(u32::from('\n'));
                        text = "\n".to_owned();
                        changed = true;
                    }
                    do_action = true;
                }
                keysym::HOME | keysym::KP_HOME => {
                    changed = if shift {
                        self.model.select_to_beginning()
                    } else {
                        self.model.move_cursor_to_beginning()
                    };
                }
                // Backspace, Delete and the arrows are the framework's.
                _ => {}
            }
        }

        if changed {
            // For Home and End the embedder's delta is the whole text put
            // at the old selection, which is no description of a caret
            // moving; its own test holds it to that, and so it is here.
            self.update(
                Change::replace(text_before, selection_before, &text),
                updates,
            );
        }
        if do_action {
            if let Some(action) = &self.input_action {
                updates.push(Update::Action {
                    client: self.client,
                    action: action.clone(),
                });
            }
        }
        changed
    }

    /// One of the input method's events, outside a key: an input method
    /// that composes on its own time delivers them here.
    pub fn im_event(&mut self, event: &ImEvent, updates: &mut Vec<Update>) {
        match event {
            ImEvent::PreeditStart => self.model.begin_composing(),
            ImEvent::PreeditChanged { text, cursor } => {
                let text_before = self.model.text();
                let composing_before = self.model.composing_range();
                let origin = if self.model.composing() {
                    self.model.composing_range().start()
                } else {
                    self.model.selection().start()
                };
                self.model.update_composing_text(text);
                self.model
                    .set_selection(TextRange::at(origin.saturating_add(*cursor)));
                self.update(
                    Change::replace(text_before, composing_before, text),
                    updates,
                );
            }
            ImEvent::Commit(text) => {
                let text_before = self.model.text();
                let composing_before = self.model.composing_range();
                let selection_before = self.model.selection();
                let was_composing = self.model.composing();
                self.model.add_text(text);
                if self.model.composing() {
                    self.model.commit_composing();
                }
                let replaced = if was_composing {
                    composing_before
                } else {
                    selection_before
                };
                self.update(Change::replace(text_before, replaced, text), updates);
            }
            ImEvent::PreeditEnd => {
                self.model.end_composing();
                self.update(Change::none(self.model.text()), updates);
            }
            ImEvent::DeleteSurrounding { offset, count } => {
                let text_before = self.model.text();
                if self.model.delete_surrounding(*offset, *count) {
                    let text = self.model.text();
                    self.update(
                        Change::replace(text_before, self.model.composing_range(), &text),
                        updates,
                    );
                }
            }
        }
    }

    /// The text before the caret's end in bytes, and the text: what an
    /// input method asking for its surroundings is given
    /// (`retrieve-surrounding`).
    pub fn surrounding(&self) -> (String, usize) {
        (self.model.text(), self.model.cursor_offset())
    }

    /// Tells the framework the text as it now is: whole, or as `change`
    /// if the field asked for deltas.
    fn update(&self, change: Change, updates: &mut Vec<Update>) {
        let selection = self.model.selection();
        let composing = self.model.composing_range();
        let (composing_base, composing_extent) = if composing.collapsed() {
            (-1, -1)
        } else {
            (offset(composing.base()), offset(composing.extent()))
        };
        let (selection_base, selection_extent) =
            (offset(selection.base()), offset(selection.extent()));
        updates.push(if self.enable_delta_model {
            Update::Delta {
                client: self.client,
                delta: EditingDelta {
                    old_text: change.old_text,
                    delta_text: change.delta_text,
                    delta_start: change.start,
                    delta_end: change.end,
                    selection_base,
                    selection_extent,
                    composing_base,
                    composing_extent,
                },
            }
        } else {
            Update::State {
                client: self.client,
                state: EditingState {
                    text: self.model.text(),
                    selection_base,
                    selection_extent,
                    composing_base,
                    composing_extent,
                },
            }
        });
    }
}

#[cfg(test)]
#[path = "handler_tests.rs"]
mod tests;
