//! `flutter/platform`'s system methods, as the Linux embedder answers them
//! (`fl_platform_channel.cc`, `fl_platform_handler.cc`): sounds, the app's
//! word that it is ready, and leaving. `JSONMethodCodec`. A method the
//! Linux embedder does not have (haptics, orientations, the status bar)
//! is answered with an empty reply, which `OptionalMethodChannel` reads as
//! null.
//!
//! Leaving goes through the shell ([`set_exit_handler`]). An exit the app
//! may cancel is first put to the framework, `System.requestAppExit` on the
//! same channel ([`set_framework_sender`]), once the app has said it can
//! answer one.

use std::sync::{Mutex, MutexGuard};

use crate::json::{self, Json};
use crate::platform::{self, Persistent, ReplyArg};

const CHANNEL: &str = "flutter/platform";

/// What the framework is asked when an exit can be canceled: the Linux
/// embedder's `System.requestAppExit` call.
const REQUEST_APP_EXIT: &str = r#"{"method":"System.requestAppExit","args":{"type":"cancelable"}}"#;

/// The app's state as this side knows it, per isolate ([`reset`]).
#[derive(Default)]
struct State {
    /// `System.initializationComplete` came: the framework answers
    /// `System.requestAppExit` now.
    initialized: bool,
    /// An exit is being put to the framework.
    exit_pending: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    initialized: false,
    exit_pending: false,
});

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

type ExitHandler = Box<dyn Fn() + Send>;

static EXIT_HANDLER: Mutex<Option<ExitHandler>> = Mutex::new(None);

/// Sets what ends the app: the shell's loop, told to return. Run on the UI
/// thread, inside a native or the framework's response to one; it must not
/// block, nor call this again (the slot is locked).
pub fn set_exit_handler(exit: impl Fn() + Send + 'static) {
    *EXIT_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(exit));
}

/// What a sender calls with the framework's response, `None` for a null
/// one or none.
pub type Respond = Box<dyn FnOnce(Option<Vec<u8>>) + Send>;

type FrameworkSender = Box<dyn Fn(&str, Vec<u8>, Respond) + Send>;

static FRAMEWORK_SENDER: Mutex<Option<FrameworkSender>> = Mutex::new(None);

/// Sets how a message goes to the framework, the channel, its bytes and
/// what takes the response: the shell's `DispatchPlatformMessage`. Run on
/// the UI thread inside a native; it must queue, not dispatch, and must
/// not call this again.
pub fn set_framework_sender(send: impl Fn(&str, Vec<u8>, Respond) + Send + 'static) {
    *FRAMEWORK_SENDER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(send));
}

/// Forgets the app's state, for a new isolate: not ready, no exit pending.
pub fn reset() {
    *state() = State::default();
}

fn exit() {
    if let Some(exit) = EXIT_HANDLER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        exit();
    }
}

/// `System.exitApplication`'s result.
fn exit_result(exit: bool) -> Vec<u8> {
    json::success(if exit {
        r#"{"response":"exit"}"#
    } else {
        r#"{"response":"cancel"}"#
    })
}

/// How a call is answered.
#[derive(Debug, PartialEq)]
pub(crate) enum Answer {
    /// With these bytes, now.
    Reply(Vec<u8>),
    /// Once the framework has answered `System.requestAppExit`
    /// ([`ask_framework`]).
    AskFramework,
}

/// Answers a system method, or `None` for a call that is not one the Linux
/// embedder has.
pub(crate) fn handle_call(message: Option<&[u8]>) -> Option<Answer> {
    let (method, args) = message.and_then(json::method_call)?;
    Some(Answer::Reply(match method.as_str() {
        // An alert is the display's beep on Linux; a click and a tick
        // are not made on a desktop. There is no display's beep here.
        "SystemSound.play" => match args {
            Json::Str(_) => json::success("null"),
            _ => json::error("Bad Arguments", "Expected string"),
        },
        "System.initializationComplete" => {
            state().initialized = true;
            json::success("null")
        }
        "SystemNavigator.pop" => {
            exit();
            json::success("null")
        }
        "System.exitApplication" => {
            let cancelable = match &args {
                Json::Object(_) => match args.get("type").and_then(Json::as_str) {
                    Some("cancelable") => true,
                    Some("required") => false,
                    Some(_) => {
                        return Some(Answer::Reply(json::error(
                            "Bad Arguments",
                            "Invalid exit type",
                        )))
                    }
                    None => {
                        return Some(Answer::Reply(json::error(
                            "Bad Arguments",
                            "Missing type argument",
                        )))
                    }
                },
                _ => {
                    return Some(Answer::Reply(json::error(
                        "Bad Arguments",
                        "Argument map missing or malformed",
                    )))
                }
            };
            let mut s = state();
            if s.exit_pending {
                json::error("In Progress", "Request already in progress")
            } else if cancelable && s.initialized {
                s.exit_pending = true;
                return Some(Answer::AskFramework);
            } else {
                // Not yet able to ask, or not the app's to refuse.
                drop(s);
                exit();
                exit_result(true)
            }
        }
        _ => return None,
    }))
}

/// Whether the framework's answer to `System.requestAppExit` lets the app
/// go. Anything but a successful `cancel` does, as on Linux: an error, a
/// malformed result, or none.
fn lets_go(response: Option<&[u8]>) -> bool {
    let Some(Json::Array(envelope)) = response.and_then(json::parse) else {
        return true;
    };
    let [result] = envelope.as_slice() else {
        return true;
    };
    result.get("response").and_then(Json::as_str) != Some("cancel")
}

/// Puts a cancelable exit to the framework, and answers `callback`, the
/// `System.exitApplication` call's, with what it says: leaving if it lets
/// the app go. With no way to the framework set, the app goes now.
pub(crate) fn ask_framework(callback: Option<Persistent>) {
    let generation = platform::generation();
    let finish = move |response: Option<Vec<u8>>| {
        let go = lets_go(response.as_deref());
        state().exit_pending = false;
        if go {
            exit();
        }
        if let Some(callback) = callback {
            platform::queue_persistent_in(generation, callback, ReplyArg::Bytes(exit_result(go)));
        }
    };
    let sender = FRAMEWORK_SENDER.lock().unwrap_or_else(|e| e.into_inner());
    match sender.as_ref() {
        Some(send) => send(
            CHANNEL,
            REQUEST_APP_EXIT.as_bytes().to_vec(),
            Box::new(finish),
        ),
        None => {
            drop(sender);
            finish(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn call(text: &str) -> Option<Answer> {
        handle_call(Some(text.as_bytes()))
    }

    fn reply(text: &str) -> String {
        match call(text) {
            Some(Answer::Reply(r)) => String::from_utf8(r).unwrap(),
            other => panic!("{text}: {other:?}"),
        }
    }

    /// One test, in order: the state and the handlers are the process's.
    #[test]
    fn system_methods_are_answered_as_the_linux_embedder_answers_them() {
        let exits = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&exits);
        set_exit_handler(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        });
        reset();

        assert_eq!(
            reply(r#"{"method":"SystemSound.play","args":"SystemSoundType.alert"}"#),
            "[null]"
        );
        assert_eq!(
            reply(r#"{"method":"SystemSound.play","args":1}"#),
            r#"["Bad Arguments","Expected string",null]"#
        );
        // Not the Linux embedder's: no answer here, an empty reply there.
        assert_eq!(call(r#"{"method":"HapticFeedback.vibrate"}"#), None);
        assert_eq!(
            call(r#"{"method":"SystemChrome.setPreferredOrientations","args":[]}"#),
            None
        );
        assert_eq!(call("not json"), None);

        // Before the app is ready, even a cancelable exit is not put to it.
        let cancelable = r#"{"method":"System.exitApplication","args":{"type":"cancelable"}}"#;
        assert_eq!(reply(cancelable), r#"[{"response":"exit"}]"#);
        assert_eq!(exits.load(Ordering::Relaxed), 1);

        assert_eq!(
            reply(r#"{"method":"System.initializationComplete"}"#),
            "[null]"
        );
        assert_eq!(
            reply(r#"{"method":"System.exitApplication","args":{"type":"required"}}"#),
            r#"[{"response":"exit"}]"#
        );
        assert_eq!(exits.load(Ordering::Relaxed), 2);
        assert_eq!(
            reply(r#"{"method":"System.exitApplication","args":{"type":"later"}}"#),
            r#"["Bad Arguments","Invalid exit type",null]"#
        );
        assert_eq!(
            reply(r#"{"method":"System.exitApplication","args":{}}"#),
            r#"["Bad Arguments","Missing type argument",null]"#
        );

        // Ready: a cancelable exit is the framework's to answer, and a
        // second waits for none.
        assert_eq!(call(cancelable), Some(Answer::AskFramework));
        assert_eq!(
            reply(cancelable),
            r#"["In Progress","Request already in progress",null]"#
        );
        // The framework says cancel: the app stays.
        let sent = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&sent);
        set_framework_sender(move |channel, data, respond| {
            seen.lock().unwrap().push((channel.to_owned(), data));
            respond(Some(br#"[{"response":"cancel"}]"#.to_vec()));
        });
        ask_framework(None);
        assert_eq!(exits.load(Ordering::Relaxed), 2);
        assert_eq!(
            sent.lock().unwrap().as_slice(),
            [(CHANNEL.to_owned(), REQUEST_APP_EXIT.as_bytes().to_vec())]
        );
        // No longer pending; asked again, the framework does not answer.
        assert_eq!(call(cancelable), Some(Answer::AskFramework));
        set_framework_sender(|_, _, respond| respond(None));
        ask_framework(None);
        assert_eq!(exits.load(Ordering::Relaxed), 3);

        assert_eq!(reply(r#"{"method":"SystemNavigator.pop"}"#), "[null]");
        assert_eq!(exits.load(Ordering::Relaxed), 4);

        // A new isolate has said nothing yet.
        reset();
        assert_eq!(reply(cancelable), r#"[{"response":"exit"}]"#);
    }

    #[test]
    fn only_a_successful_cancel_keeps_the_app() {
        assert!(!lets_go(Some(br#"[{"response":"cancel"}]"#)));
        assert!(lets_go(Some(br#"[{"response":"exit"}]"#)));
        assert!(lets_go(Some(br#"["error","message",null]"#)));
        assert!(lets_go(Some(b"[]")));
        assert!(lets_go(Some(b"{")));
        assert!(lets_go(None));
    }
}
