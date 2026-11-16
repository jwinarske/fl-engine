//! `flutter/textinput` (brief M4 §2): the framework's calls decoded and
//! given to the text input handler ([`fl_input::text::TextInput`]), and
//! what the handler tells the framework encoded, both as the Linux
//! embedder's `fl_text_input_channel.cc` has them. `JSONMethodCodec`.
//!
//! The handler is the process's one, as the engine is: its state is the
//! UI thread's, changed by the framework's calls inside the platform
//! message native and by keys the shell offers between them.

use std::sync::{Mutex, MutexGuard};

use fl_input::text::{EditingDelta, EditingState, InputMethod, InputType, TextInput, Update};
use fl_input::xkb::XkbKey;

use crate::json::{self, Json};

/// The channel's name.
pub const TEXT_INPUT_CHANNEL: &str = "flutter/textinput";

const BAD_ARGUMENTS: &str = "Bad Arguments";

static HANDLER: Mutex<Option<TextInput>> = Mutex::new(None);

fn handler() -> MutexGuard<'static, Option<TextInput>> {
    HANDLER.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sets the platform's input method, in place of the one that types each
/// key's character and composes nothing. The handler starts over with it.
pub fn set_input_method(im: Box<dyn InputMethod>) {
    *handler() = Some(TextInput::new(im));
}

/// Starts the handler over, for a new isolate: no text field has the
/// keyboard. The input method is kept.
pub fn reset() {
    if let Some(input) = handler().as_mut() {
        input.reset();
    }
}

/// A key the framework did not handle, offered to text input. Whether it
/// was taken, and the messages to send the framework on
/// [`TEXT_INPUT_CHANNEL`], in order.
pub fn filter_key(key: &XkbKey) -> (bool, Vec<Vec<u8>>) {
    let mut updates = Vec::new();
    let taken = handler()
        .get_or_insert_with(TextInput::default)
        .filter_key(key, &mut updates);
    (taken, updates.iter().map(encode_update).collect())
}

/// Answers one of the framework's calls on [`TEXT_INPUT_CHANNEL`]: a
/// success envelope of null, an error envelope for arguments that are not
/// the method's, or `None`, no reply's worth, for a method the embedder
/// does not implement. dart:ui's platform message native calls it; it is
/// the UI thread's.
pub fn handle_call(message: Option<&[u8]>) -> Option<Vec<u8>> {
    let (method, args) = message.and_then(json::method_call)?;
    let mut guard = handler();
    let input = guard.get_or_insert_with(TextInput::default);
    let result = match method.as_str() {
        "TextInput.setClient" => set_client(input, &args),
        "TextInput.updateConfig" => update_config(input, &args),
        "TextInput.show" => {
            input.show();
            Ok(())
        }
        "TextInput.hide" => {
            input.hide();
            Ok(())
        }
        "TextInput.setEditingState" => set_editing_state(input, &args),
        "TextInput.clearClient" => {
            input.clear_client();
            Ok(())
        }
        "TextInput.setEditableSizeAndTransform" => set_editable_transform(input, &args),
        "TextInput.setMarkedTextRect" => set_marked_text_rect(input, &args),
        _ => return None,
    };
    Some(match result {
        Ok(()) => json::success("null"),
        Err(message) => json::error(BAD_ARGUMENTS, message),
    })
}

fn set_client(input: &mut TextInput, args: &Json) -> Result<(), &'static str> {
    let (Some([client, config, ..]), true) = (
        args.as_array(),
        args.as_array().is_some_and(|a| a.len() >= 2),
    ) else {
        return Err("Expected 2-element list");
    };
    let client = client.as_i64().ok_or("Expected a client id")?;
    input.set_client(client);
    update_config(input, config)
}

fn update_config(input: &mut TextInput, config: &Json) -> Result<(), &'static str> {
    let action = config.get("inputAction").and_then(Json::as_str);
    let deltas = config
        .get("enableDeltaModel")
        .and_then(Json::as_bool)
        .unwrap_or(false);
    let name = config
        .get("inputType")
        .and_then(|t| t.get("name"))
        .and_then(Json::as_str);
    let input_type = match name {
        Some("TextInputType.multiline") => InputType::Multiline,
        Some("TextInputType.none") => InputType::None,
        // The rest differ in GTK's input purpose and hints, which are an
        // on-screen keyboard's.
        _ => InputType::Text,
    };
    input.configure(action, deltas, input_type);
    Ok(())
}

fn set_editing_state(input: &mut TextInput, args: &Json) -> Result<(), &'static str> {
    let text = args
        .get("text")
        .and_then(Json::as_str)
        .ok_or("Expected a text")?;
    // The embedder reads an offset that is missing as 0.
    let offset = |key: &str| args.get(key).and_then(Json::as_i64).unwrap_or(0);
    input.set_editing_state(
        text,
        offset("selectionBase"),
        offset("selectionExtent"),
        offset("composingBase"),
        offset("composingExtent"),
    );
    Ok(())
}

fn set_editable_transform(input: &mut TextInput, args: &Json) -> Result<(), &'static str> {
    let values = args
        .get("transform")
        .and_then(Json::as_array)
        .filter(|t| t.len() == 16)
        .ok_or("Invalid transform")?;
    let mut transform = [0.0; 16];
    for (out, v) in transform.iter_mut().zip(values) {
        *out = v.as_f64().ok_or("Invalid transform")?;
    }
    input.set_editable_transform(transform);
    Ok(())
}

fn set_marked_text_rect(input: &mut TextInput, args: &Json) -> Result<(), &'static str> {
    let side = |key: &str| args.get(key).and_then(Json::as_f64).unwrap_or(0.0);
    input.set_marked_text_rect(side("x"), side("y"), side("width"), side("height"));
    Ok(())
}

/// One of the handler's messages for the framework
/// (`fl_text_input_channel_update_editing_state` and the two beside it).
fn encode_update(update: &Update) -> Vec<u8> {
    let mut out = String::from("{\"method\":");
    match update {
        Update::State { client, state } => {
            out.push_str(&format!(
                "\"TextInputClient.updateEditingState\",\"args\":[{client},{{\"text\":"
            ));
            let EditingState {
                text,
                selection_base,
                selection_extent,
                composing_base,
                composing_extent,
            } = state;
            json::write_string(&mut out, text);
            write_ranges(
                &mut out,
                [*selection_base, *selection_extent],
                [*composing_base, *composing_extent],
            );
            out.push_str("}]}");
        }
        Update::Delta { client, delta } => {
            out.push_str(&format!(
                "\"TextInputClient.updateEditingStateWithDeltas\",\"args\":[{client},\
                 {{\"deltas\":[{{\"oldText\":"
            ));
            let EditingDelta {
                old_text,
                delta_text,
                delta_start,
                delta_end,
                selection_base,
                selection_extent,
                composing_base,
                composing_extent,
            } = delta;
            json::write_string(&mut out, old_text);
            out.push_str(",\"deltaText\":");
            json::write_string(&mut out, delta_text);
            out.push_str(&format!(
                ",\"deltaStart\":{delta_start},\"deltaEnd\":{delta_end}"
            ));
            write_ranges(
                &mut out,
                [*selection_base, *selection_extent],
                [*composing_base, *composing_extent],
            );
            out.push_str("}]}]}");
        }
        Update::Action { client, action } => {
            out.push_str(&format!(
                "\"TextInputClient.performAction\",\"args\":[{client},"
            ));
            json::write_string(&mut out, action);
            out.push_str("]}");
        }
    }
    out.into_bytes()
}

/// The selection and the composing range, as both updates carry them. The
/// affinity is downstream and the selection not directional, always, as
/// the embedder's.
fn write_ranges(out: &mut String, selection: [i64; 2], composing: [i64; 2]) {
    out.push_str(&format!(
        ",\"selectionBase\":{},\"selectionExtent\":{},\
         \"selectionAffinity\":\"TextAffinity.downstream\",\"selectionIsDirectional\":false,\
         \"composingBase\":{},\"composingExtent\":{}",
        selection[0], selection[1], composing[0], composing[1]
    ));
}

#[cfg(test)]
mod tests {
    use fl_input::text::Direct;

    use super::*;

    /// The handler is the process's one; tests that use it take turns.
    static TURN: Mutex<()> = Mutex::new(());

    fn call(text: &str) -> Option<String> {
        handle_call(Some(text.as_bytes())).map(|r| String::from_utf8_lossy(&r).into_owned())
    }

    fn typed(c: char) -> XkbKey {
        XkbKey {
            pressed: true,
            keyval: c as u32,
            unicode: c as u32,
            ..XkbKey::default()
        }
    }

    fn messages(key: &XkbKey) -> Vec<String> {
        filter_key(key)
            .1
            .iter()
            .map(|m| String::from_utf8_lossy(m).into_owned())
            .collect()
    }

    const CLIENT: &str = r#"{"method":"TextInput.setClient","args":[3,{"inputType":{"name":"TextInputType.multiline","signed":null,"decimal":null},"inputAction":"TextInputAction.newline","enableDeltaModel":false}]}"#;

    /// A field is set up as the framework sets one up, typed into, and
    /// the messages are the embedder's, member for member.
    #[test]
    fn typing_tells_the_framework_the_editing_state() {
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        set_input_method(Box::new(Direct));
        assert_eq!(filter_key(&typed('a')), (false, vec![]), "no client yet");
        assert_eq!(call(CLIENT).as_deref(), Some("[null]"));
        assert_eq!(
            call(r#"{"method":"TextInput.show","args":null}"#).as_deref(),
            Some("[null]")
        );
        assert_eq!(
            call(r#"{"method":"TextInput.setEditingState","args":{"text":"a\"😄","selectionBase":4,"selectionExtent":4,"selectionAffinity":"TextAffinity.downstream","selectionIsDirectional":false,"composingBase":-1,"composingExtent":-1}}"#).as_deref(),
            Some("[null]")
        );
        assert_eq!(
            messages(&typed('é')),
            [
                r#"{"method":"TextInputClient.updateEditingState","args":[3,{"text":"a\"😄é","selectionBase":5,"selectionExtent":5,"selectionAffinity":"TextAffinity.downstream","selectionIsDirectional":false,"composingBase":-1,"composingExtent":-1}]}"#
            ]
        );
        // Return: the line added, then the action.
        let enter = XkbKey {
            pressed: true,
            keyval: 0xff0d,
            unicode: 0x0d,
            ..XkbKey::default()
        };
        assert_eq!(
            messages(&enter),
            [
                r#"{"method":"TextInputClient.updateEditingState","args":[3,{"text":"a\"😄é\n","selectionBase":6,"selectionExtent":6,"selectionAffinity":"TextAffinity.downstream","selectionIsDirectional":false,"composingBase":-1,"composingExtent":-1}]}"#,
                r#"{"method":"TextInputClient.performAction","args":[3,"TextInputAction.newline"]}"#,
            ]
        );
        assert_eq!(
            call(r#"{"method":"TextInput.clearClient","args":null}"#).as_deref(),
            Some("[null]")
        );
        assert_eq!(filter_key(&typed('a')), (false, vec![]));
    }

    #[test]
    fn a_field_that_asked_for_deltas_is_sent_them() {
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        set_input_method(Box::new(Direct));
        call(r#"{"method":"TextInput.setClient","args":[1,{"enableDeltaModel":true}]}"#);
        call(
            r#"{"method":"TextInput.setEditingState","args":{"text":"ab","selectionBase":0,"selectionExtent":2,"composingBase":-1,"composingExtent":-1}}"#,
        );
        assert_eq!(
            messages(&typed('c')),
            [
                r#"{"method":"TextInputClient.updateEditingStateWithDeltas","args":[1,{"deltas":[{"oldText":"ab","deltaText":"c","deltaStart":0,"deltaEnd":2,"selectionBase":1,"selectionExtent":1,"selectionAffinity":"TextAffinity.downstream","selectionIsDirectional":false,"composingBase":-1,"composingExtent":-1}]}]}"#
            ]
        );
        // The configuration changed: whole states from now on.
        call(r#"{"method":"TextInput.updateConfig","args":{"enableDeltaModel":false}}"#);
        assert!(messages(&typed('d'))[0].contains("TextInputClient.updateEditingState\""));
        reset();
        assert_eq!(filter_key(&typed('a')), (false, vec![]), "a new isolate");
    }

    /// Every call is answered or known to want no answer; none panics
    /// (brief M4 §15).
    #[test]
    fn malformed_calls_are_errors() {
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        set_input_method(Box::new(Direct));
        let bad = |why: &str| Some(format!("[\"Bad Arguments\",\"{why}\",null]"));
        assert_eq!(
            call(r#"{"method":"TextInput.setClient","args":[1]}"#),
            bad("Expected 2-element list")
        );
        assert_eq!(
            call(r#"{"method":"TextInput.setClient","args":{"a":1}}"#),
            bad("Expected 2-element list")
        );
        assert_eq!(
            call(r#"{"method":"TextInput.setClient","args":["1",{}]}"#),
            bad("Expected a client id")
        );
        assert_eq!(
            call(r#"{"method":"TextInput.setEditingState","args":{"selectionBase":1}}"#),
            bad("Expected a text")
        );
        assert_eq!(
            call(
                r#"{"method":"TextInput.setEditableSizeAndTransform","args":{"transform":[1,2,3]}}"#
            ),
            bad("Invalid transform")
        );
        assert_eq!(
            call(r#"{"method":"TextInput.setEditableSizeAndTransform","args":{"width":1,"height":2,"transform":[1.0,0,0,0,0,1,0,0,0,0,1,0,8.5,9,0,1]}}"#).as_deref(),
            Some("[null]")
        );
        assert_eq!(
            call(r#"{"method":"TextInput.setMarkedTextRect","args":{"width":1e400,"height":"x"}}"#)
                .as_deref(),
            Some("[null]")
        );
        // Offsets no text has, a client and a config of the wrong shapes.
        assert_eq!(
            call(r#"{"method":"TextInput.setEditingState","args":{"text":"a","selectionBase":-9,"selectionExtent":1e3,"composingBase":99,"composingExtent":5}}"#).as_deref(),
            Some("[null]")
        );
        assert_eq!(
            call(r#"{"method":"TextInput.setClient","args":[9007199254740993,[]]}"#).as_deref(),
            Some("[null]")
        );
        // Not the embedder's, or not a call at all: nothing to answer with.
        assert_eq!(call(r#"{"method":"TextInput.setStyle","args":{}}"#), None);
        assert_eq!(call(r#"{"method":7}"#), None);
        assert_eq!(call("[]"), None);
        assert_eq!(call(""), None);
        assert_eq!(handle_call(None), None);
        assert_eq!(handle_call(Some(&[0xff, 0xfe])), None);
    }
}
