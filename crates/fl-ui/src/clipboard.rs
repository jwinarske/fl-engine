//! The clipboard's methods on `flutter/platform` (brief M4 D4): the Linux
//! embedder's `fl_platform_channel.cc` and `fl_platform_handler.cc`, with
//! the platform's clipboard behind [`fl_input::text::Clipboard`].
//! `JSONMethodCodec`.

use std::sync::{Mutex, MutexGuard};

use fl_input::text::{Clipboard, MemoryClipboard};

use crate::json::{self, Json};

const TEXT_PLAIN: &str = "text/plain";

static CLIPBOARD: Mutex<Option<Box<dyn Clipboard>>> = Mutex::new(None);

fn clipboard() -> MutexGuard<'static, Option<Box<dyn Clipboard>>> {
    CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sets the platform's clipboard, in place of one kept in the process.
pub fn set_clipboard(platform: Box<dyn Clipboard>) {
    *clipboard() = Some(platform);
}

/// Answers a clipboard method, or `None` for a call that is not one.
pub(crate) fn handle_call(message: Option<&[u8]>) -> Option<Vec<u8>> {
    let (method, args) = message.and_then(json::method_call)?;
    if !method.starts_with("Clipboard.") {
        return None;
    }
    let mut guard = clipboard();
    let clipboard = guard.get_or_insert_with(|| Box::new(MemoryClipboard::default()));
    Some(match method.as_str() {
        "Clipboard.setData" => match &args {
            Json::Object(_) => match args.get("text").and_then(Json::as_str) {
                Some(text) => {
                    clipboard.set_text(text);
                    json::success("null")
                }
                None => json::error("Bad Arguments", "Missing clipboard text"),
            },
            _ => json::error("Bad Arguments", "Argument map missing or malformed"),
        },
        "Clipboard.getData" => match args.as_str() {
            Some(TEXT_PLAIN) => match clipboard.text() {
                Some(text) => {
                    let mut result = String::from("{\"text\":");
                    json::write_string(&mut result, &text);
                    result.push('}');
                    json::success(&result)
                }
                None => json::success("null"),
            },
            Some(_) => json::error("Unknown Clipboard Format", "The clipboard holds text only"),
            None => json::error("Bad Arguments", "Expected string"),
        },
        "Clipboard.hasStrings" => {
            let has = clipboard.text().is_some_and(|t| !t.is_empty());
            json::success(&format!("{{\"value\":{has}}}"))
        }
        // A method of the clipboard's the embedder does not have.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(text: &str) -> Option<String> {
        handle_call(Some(text.as_bytes())).map(|r| String::from_utf8_lossy(&r).into_owned())
    }

    /// One test, in order: the clipboard is the process's.
    #[test]
    fn text_is_set_asked_for_and_read() {
        set_clipboard(Box::new(MemoryClipboard::default()));
        let has = r#"{"method":"Clipboard.hasStrings","args":"text/plain"}"#;
        let get = r#"{"method":"Clipboard.getData","args":"text/plain"}"#;
        assert_eq!(call(has).as_deref(), Some(r#"[{"value":false}]"#));
        assert_eq!(call(get).as_deref(), Some("[null]"));
        assert_eq!(
            call(r#"{"method":"Clipboard.setData","args":{"text":"a \"b\"\n😄"}}"#).as_deref(),
            Some("[null]")
        );
        assert_eq!(call(has).as_deref(), Some(r#"[{"value":true}]"#));
        assert_eq!(call(get).as_deref(), Some(r#"[{"text":"a \"b\"\n😄"}]"#));
        // Empty text is text, and no strings.
        call(r#"{"method":"Clipboard.setData","args":{"text":""}}"#);
        assert_eq!(call(has).as_deref(), Some(r#"[{"value":false}]"#));
        assert_eq!(call(get).as_deref(), Some(r#"[{"text":""}]"#));

        assert_eq!(
            call(r#"{"method":"Clipboard.getData","args":"image/png"}"#).as_deref(),
            Some(r#"["Unknown Clipboard Format","The clipboard holds text only",null]"#)
        );
        assert_eq!(
            call(r#"{"method":"Clipboard.getData","args":null}"#).as_deref(),
            Some(r#"["Bad Arguments","Expected string",null]"#)
        );
        assert_eq!(
            call(r#"{"method":"Clipboard.setData","args":"text"}"#).as_deref(),
            Some(r#"["Bad Arguments","Argument map missing or malformed",null]"#)
        );
        assert_eq!(
            call(r#"{"method":"Clipboard.setData","args":{"text":7}}"#).as_deref(),
            Some(r#"["Bad Arguments","Missing clipboard text",null]"#)
        );
        // Not the clipboard's: for whoever else answers the channel.
        assert_eq!(
            call(r#"{"method":"SystemSound.play","args":"SystemSoundType.click"}"#),
            None
        );
        assert_eq!(call(r#"{"method":"Clipboard.clear"}"#), None);
        assert_eq!(call("{"), None);
        assert_eq!(handle_call(None), None);
    }
}
