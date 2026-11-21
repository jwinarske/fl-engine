//! `url_launcher_linux`'s two calls, answered here: the plugin's Linux side is
//! C over GTK (`url_launcher_plugin.cc`), which fl-engine cannot load, and
//! its Dart side sends pigeon messages on two channels whoever hosts the app
//! is to answer. `StandardMessageCodec`, a list holding the URL; a list
//! holding the answer back, or pigeon's error list of three.
//!
//! What opens a URL is the platform's ([`set_url_launcher`]). With none set
//! no URL can be launched, which is what a kiosk with no browser can say:
//! `canLaunchUrl` answers false and `launchUrl` an error, which the plugin
//! raises as a `PlatformException`, as the stock embedder's does when GIO
//! finds no handler.
//!
//! `canLaunchUrl` is ivi-homescreen-plugins' rule: a URL of one of six
//! schemes, which costs no lookup and offers nothing a handler for a custom
//! scheme would open. `launchUrl` opens any URL with a scheme, and its
//! answer is the launcher's own, when it has one: an opener's exit status,
//! say, which the launcher waits for off the UI thread, the reply going
//! back when it comes. An error is the plugin's own reply, a string, not
//! pigeon's error list, so the plugin raises it as it documents.

use std::sync::{Mutex, MutexGuard};

/// Said when a launch is over: opened, or why not.
pub type Launched = Box<dyn FnOnce(Result<(), String>) + Send>;

/// What opens a URL for an app, on the platform's side.
pub trait UrlLauncher: Send {
    /// Whether this launcher can open anything at all: its opener is there.
    /// Which URLs it is asked about is fl-ui's to decide.
    fn available(&self) -> bool;
    /// Opens `url` and calls `done` with how it went, from any thread, once.
    /// Called on the UI thread inside a native, so it must not wait on what
    /// it starts: `done` is how the answer comes back.
    fn launch(&self, url: &str, done: Launched);
}

static LAUNCHER: Mutex<Option<Box<dyn UrlLauncher>>> = Mutex::new(None);

fn launcher() -> MutexGuard<'static, Option<Box<dyn UrlLauncher>>> {
    LAUNCHER.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sets what opens URLs, or none: then none can be.
pub fn set_url_launcher(platform: Option<Box<dyn UrlLauncher>>) {
    *launcher() = platform;
}

const CAN_LAUNCH: &str = "dev.flutter.pigeon.url_launcher_linux.UrlLauncherApi.canLaunchUrl";
const LAUNCH: &str = "dev.flutter.pigeon.url_launcher_linux.UrlLauncherApi.launchUrl";

/// Which of the two calls `channel` is, with pigeon's optional instance
/// suffix (`.name`) after it.
fn call_of(channel: &str) -> Option<&'static str> {
    [CAN_LAUNCH, LAUNCH].into_iter().find(|name| {
        channel
            .strip_prefix(name)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    })
}

/// The longest URL taken: a browser's own limits are of this order, and a
/// message past it is no URL an app means to open.
const MAX_URL: usize = 64 * 1024;

/// `StandardMessageCodec.readSize` at `at`, moving past it.
fn read_size(bytes: &[u8], at: &mut usize) -> Option<usize> {
    let first = *bytes.get(*at)?;
    *at += 1;
    match first {
        254 => {
            let raw = bytes.get(*at..*at + 2)?;
            *at += 2;
            Some(usize::from(u16::from_ne_bytes([raw[0], raw[1]])))
        }
        255 => {
            let raw = bytes.get(*at..*at + 4)?;
            *at += 4;
            usize::try_from(u32::from_ne_bytes([raw[0], raw[1], raw[2], raw[3]])).ok()
        }
        n => Some(usize::from(n)),
    }
}

/// The URL of a call's arguments, `[url]`: a list (12) of one string (7),
/// and nothing after it.
fn url_of(message: &[u8]) -> Option<&str> {
    let mut at = 0;
    if *message.get(at)? != 12 {
        return None;
    }
    at += 1;
    if read_size(message, &mut at)? != 1 || *message.get(at)? != 7 {
        return None;
    }
    at += 1;
    let len = read_size(message, &mut at)?;
    if len > MAX_URL || at + len != message.len() {
        return None;
    }
    std::str::from_utf8(&message[at..]).ok()
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

fn write_string(out: &mut Vec<u8>, s: &str) {
    out.push(7);
    write_size(out, s.len());
    out.extend_from_slice(s.as_bytes());
}

/// Pigeon's error reply: `[code, message, details]`.
fn error(code: &str, message: &str) -> Vec<u8> {
    let mut out = vec![12, 3];
    write_string(&mut out, code);
    write_string(&mut out, message);
    out.push(0);
    out
}

/// The schemes `canLaunchUrl` says can be opened, ivi-homescreen-plugins'
/// six (`url_launcher_plugin.cc`).
const SCHEMES: [&str; 6] = ["https", "http", "ftp", "file", "mailto", "tel"];

/// Whether `url` names a scheme as GIO's `g_uri_parse_scheme` reads one: a
/// letter, then letters, digits, `+`, `-` and `.`, then a colon. Nothing
/// without one is opened, and so no URL reaches a launcher's command line
/// looking like an option.
fn has_scheme(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// The scheme of a URL that has one ([`has_scheme`]), as written.
fn scheme_of(url: &str) -> Option<&str> {
    has_scheme(url).then(|| url.split_once(':').map_or("", |(scheme, _)| scheme))
}

/// `launchUrl`'s reply: `[null]` for opened, `[message]` for why not, which
/// the plugin raises.
fn launch_reply(result: Result<(), String>) -> Vec<u8> {
    match result {
        Ok(()) => vec![12, 1, 0],
        Err(why) => {
            let mut out = vec![12, 1];
            write_string(&mut out, &why);
            out
        }
    }
}

/// How a call on one of the plugin's channels is answered.
pub(crate) enum Answer {
    /// With these bytes, now.
    Reply(Vec<u8>),
    /// Once [`launch`] has opened this URL.
    Launch(String),
}

/// Answers a call on one of the plugin's channels, or `None` for a channel
/// that is not one.
pub(crate) fn handle(channel: &str, message: Option<&[u8]>) -> Option<Answer> {
    let call = call_of(channel)?;
    let Some(url) = message.and_then(url_of) else {
        return Some(Answer::Reply(error(
            "Bad Arguments",
            "expected [url], a URL as a string",
        )));
    };
    if call == CAN_LAUNCH {
        let can = scheme_of(url)
            .is_some_and(|s| SCHEMES.iter().any(|known| s.eq_ignore_ascii_case(known)))
            && launcher().as_ref().is_some_and(|l| l.available());
        // `[true]` or `[false]`.
        return Some(Answer::Reply(vec![12, 1, if can { 1 } else { 2 }]));
    }
    Some(if has_scheme(url) {
        Answer::Launch(url.to_owned())
    } else {
        // Nothing without a scheme reaches a launcher, so no URL reaches an
        // opener's command line looking like an option.
        Answer::Reply(launch_reply(Err(format!("{url:?} names no scheme"))))
    })
}

/// Opens `url` with the platform's launcher, and gives `reply` the plugin's
/// answer when the launch is over: at once with no launcher, else whenever
/// and on whichever thread the launcher says.
pub(crate) fn launch(url: &str, reply: impl FnOnce(Vec<u8>) + Send + 'static) {
    let guard = launcher();
    match guard.as_ref() {
        None => {
            drop(guard);
            reply(launch_reply(Err(
                "no application here opens URLs".to_owned()
            )));
        }
        Some(l) => l.launch(url, Box::new(move |result| reply(launch_reply(result)))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn call(url: &str) -> Vec<u8> {
        let mut out = vec![12, 1];
        write_string(&mut out, url);
        out
    }

    fn reply(channel: &str, url: &str) -> Vec<u8> {
        match handle(channel, Some(&call(url))) {
            Some(Answer::Reply(bytes)) => bytes,
            Some(Answer::Launch(url)) => {
                let got = Arc::new(Mutex::new(None));
                let into = Arc::clone(&got);
                launch(&url, move |bytes| *into.lock().unwrap() = Some(bytes));
                // The test's launcher answers before it returns.
                let answered = got.lock().unwrap().take();
                answered.expect("answered")
            }
            None => panic!("{channel}: not the plugin's"),
        }
    }

    fn said(message: &str) -> Vec<u8> {
        let mut out = vec![12, 1];
        write_string(&mut out, message);
        out
    }

    /// Records what it is asked to open and fails a URL naming `fail`, as an
    /// opener's exit status would.
    struct Recorder(Arc<Mutex<Vec<String>>>);

    impl UrlLauncher for Recorder {
        fn available(&self) -> bool {
            true
        }
        fn launch(&self, url: &str, done: Launched) {
            self.0.lock().unwrap().push(url.to_owned());
            done(if url.contains("fail") {
                Err("exit status 3".into())
            } else {
                Ok(())
            });
        }
    }

    /// One test, in order: the launcher is the process's.
    #[test]
    fn the_plugins_calls_are_answered_as_pigeon_reads_them() {
        let site = "https://www.wonderous.info/";
        set_url_launcher(None);
        assert_eq!(reply(CAN_LAUNCH, site), vec![12, 1, 2]);
        assert_eq!(reply(LAUNCH, site), said("no application here opens URLs"));

        let launched = Arc::new(Mutex::new(Vec::new()));
        set_url_launcher(Some(Box::new(Recorder(Arc::clone(&launched)))));
        assert_eq!(reply(CAN_LAUNCH, site), vec![12, 1, 1]);
        assert_eq!(reply(LAUNCH, site), vec![12, 1, 0]);
        assert_eq!(
            reply(LAUNCH, "https://fail.example/"),
            said("exit status 3")
        );
        // The six schemes can be launched, in any case; another cannot,
        // though a launch of it is still made.
        for url in [
            "tel:123",
            "MAILTO:a@b.c",
            "file:///x",
            "ftp://h/",
            "http://h/",
        ] {
            assert_eq!(reply(CAN_LAUNCH, url), vec![12, 1, 1], "{url}");
        }
        assert_eq!(reply(CAN_LAUNCH, "myapp://open"), vec![12, 1, 2]);
        assert_eq!(reply(LAUNCH, "myapp://open"), vec![12, 1, 0]);
        // An instance suffix is the same call.
        assert_eq!(reply(&format!("{CAN_LAUNCH}.second"), site), vec![12, 1, 1]);
        // Nothing without a scheme reaches the launcher.
        assert_eq!(reply(CAN_LAUNCH, "-x"), vec![12, 1, 2]);
        assert_eq!(reply(LAUNCH, "--help"), said("\"--help\" names no scheme"));
        assert_eq!(
            launched.lock().unwrap().as_slice(),
            [site, "https://fail.example/", "myapp://open"]
        );
        set_url_launcher(None);
    }

    #[test]
    fn other_channels_and_malformed_calls() {
        assert!(handle("flutter/platform", Some(&call("x:y"))).is_none());
        assert!(handle(&format!("{CAN_LAUNCH}Extra"), Some(&call("x:y"))).is_none());
        let bad = error("Bad Arguments", "expected [url], a URL as a string");
        for message in [
            None,
            Some(&[][..]),
            Some(&[12, 0][..]),
            Some(&[12, 1, 3, 0, 0, 0, 0][..]),
            Some(&[12, 1, 7, 5, b'a'][..]),
            Some(&[12, 2, 7, 0, 7, 0][..]),
            Some(&[12, 1, 7, 255, 255, 255, 255, 255][..]),
        ] {
            match handle(CAN_LAUNCH, message) {
                Some(Answer::Reply(bytes)) => assert_eq!(bytes, bad, "{message:?}"),
                _ => panic!("{message:?}: not refused"),
            }
        }
        // A long URL is sized in two bytes, and read so.
        let long = format!("https://example.com/{}", "a".repeat(300));
        assert_eq!(url_of(&call(&long)), Some(long.as_str()));
        assert!(has_scheme("mailto:x@example.com") && !has_scheme("1http:x") && !has_scheme("x"));
    }
}
