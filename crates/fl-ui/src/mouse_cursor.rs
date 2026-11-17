//! `flutter/mousecursor` (brief M4 §2): the Linux embedder's
//! `fl_mouse_cursor_channel.cc` and `fl_mouse_cursor_handler.cc`, with the
//! platform's cursor behind [`fl_input::MouseCursor`].
//! `StandardMethodCodec`.

use std::sync::Mutex;

use fl_input::{cursor_name, MouseCursor};

pub(crate) const MOUSE_CURSOR_CHANNEL: &str = "flutter/mousecursor";

static CURSOR: Mutex<Option<Box<dyn MouseCursor>>> = Mutex::new(None);

/// Sets the platform's mouse cursor. With none, the framework's requests
/// are answered and nothing is shown.
pub fn set_mouse_cursor(platform: Option<Box<dyn MouseCursor>>) {
    *CURSOR.lock().unwrap_or_else(|e| e.into_inner()) = platform;
}

/// `StandardMessageCodec`'s types, the few a call here has.
const NULL: u8 = 0;
const INT32: u8 = 3;
const INT64: u8 = 4;
const STRING: u8 = 7;
const MAP: u8 = 13;

/// A string: its type, a one-byte length below 254, UTF-8.
fn string<'a>(bytes: &mut &'a [u8]) -> Option<&'a str> {
    let (&[STRING, len], rest) = bytes.split_first_chunk::<2>()? else {
        return None;
    };
    if len >= 254 {
        return None;
    }
    let (text, rest) = rest.split_at_checked(usize::from(len))?;
    *bytes = rest;
    std::str::from_utf8(text).ok()
}

/// The `kind` of an `activateSystemCursor` call's arguments, a map of
/// strings to an integer (`device`) and a string (`kind`). `None` if the
/// arguments are no such map or have no kind.
fn kind(mut args: &[u8]) -> Option<&str> {
    let (&[MAP, entries], rest) = args.split_first_chunk::<2>()? else {
        return None;
    };
    if entries >= 254 {
        return None;
    }
    args = rest;
    let mut kind = None;
    for _ in 0..entries {
        let key = string(&mut args)?;
        match *args.first()? {
            STRING => {
                let value = string(&mut args)?;
                if key == "kind" {
                    kind = Some(value);
                }
            }
            INT32 => args = args.get(5..)?,
            INT64 => args = args.get(9..)?,
            NULL => args = args.get(1..)?,
            _ => return None,
        }
    }
    kind
}

/// An error envelope: 1, then the code, the message and null details.
fn error(code: &str, message: &str) -> Vec<u8> {
    let mut out = vec![1];
    for text in [code, message] {
        out.push(STRING);
        // Both are this file's, and short.
        out.push(u8::try_from(text.len()).unwrap_or(0));
        out.extend_from_slice(&text.as_bytes()[..text.len().min(253)]);
    }
    out.push(NULL);
    out
}

/// Answers a call on the channel; `None` is the framework's "no such
/// method", for any method but the embedder's one.
pub(crate) fn handle_call(message: Option<&[u8]>) -> Option<Vec<u8>> {
    let mut rest = message?;
    if string(&mut rest)? != "activateSystemCursor" {
        return None;
    }
    let Some(kind) = kind(rest) else {
        return Some(error("Bad Arguments", "Missing cursor kind"));
    };
    if let Some(cursor) = CURSOR.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        cursor.activate(cursor_name(kind));
    }
    // A success envelope of null.
    Some(vec![0, NULL])
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex as StdMutex};

    use super::*;

    struct Kept(Arc<StdMutex<Vec<String>>>);

    impl MouseCursor for Kept {
        fn activate(&mut self, name: &str) {
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(name.to_owned());
        }
    }

    fn put(out: &mut Vec<u8>, text: &str) {
        out.push(STRING);
        out.push(text.len() as u8);
        out.extend_from_slice(text.as_bytes());
    }

    /// `activateSystemCursor({device: 1, kind: kind})` as the framework's
    /// codec writes it.
    fn call(kind: &str) -> Vec<u8> {
        let mut out = Vec::new();
        put(&mut out, "activateSystemCursor");
        out.extend_from_slice(&[MAP, 2]);
        put(&mut out, "device");
        out.extend_from_slice(&[INT32, 1, 0, 0, 0]);
        put(&mut out, "kind");
        put(&mut out, kind);
        out
    }

    #[test]
    fn a_kind_is_shown_by_its_theme_name_and_every_call_is_answered() {
        let shown = Arc::new(StdMutex::new(Vec::new()));
        set_mouse_cursor(Some(Box::new(Kept(Arc::clone(&shown)))));
        assert_eq!(handle_call(Some(&call("click"))), Some(vec![0, 0]));
        assert_eq!(handle_call(Some(&call("none"))), Some(vec![0, 0]));
        assert_eq!(handle_call(Some(&call("not a kind"))), Some(vec![0, 0]));
        assert_eq!(
            *shown.lock().unwrap_or_else(|e| e.into_inner()),
            ["pointer", "none", "default"]
        );
        // No kind: the embedder's error, and nothing shown.
        let mut none = Vec::new();
        put(&mut none, "activateSystemCursor");
        none.extend_from_slice(&[MAP, 0]);
        let reply = handle_call(Some(&none)).unwrap_or_default();
        assert_eq!(reply.first(), Some(&1));
        // Another method is no method here.
        let mut other = Vec::new();
        put(&mut other, "setCustomCursor");
        other.push(NULL);
        assert_eq!(handle_call(Some(&other)), None);
        assert_eq!(handle_call(None), None);
        // Cut short anywhere, or with any byte changed, a call is answered
        // or is none, and never more than that.
        let whole = call("text");
        for end in 0..whole.len() {
            let _ = handle_call(Some(&whole[..end]));
        }
        for at in 0..whole.len() {
            for byte in [0u8, 3, 4, 7, 12, 13, 253, 254, 255] {
                let mut changed = whole.clone();
                changed[at] = byte;
                let _ = handle_call(Some(&changed));
            }
        }
        set_mouse_cursor(None);
        assert_eq!(handle_call(Some(&call("click"))), Some(vec![0, 0]));
    }
}
