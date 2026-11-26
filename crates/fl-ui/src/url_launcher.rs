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
//!
//! Before 3.2.0 the plugin made the same two calls on a method channel,
//! `plugins.flutter.io/url_launcher_linux`: `canLaunch` and `launch`, each
//! with the URL as its argument, in `StandardMethodCodec`'s envelopes. They
//! are answered by the same rules, a failed launch as the plugin's C side
//! answered one, a `Launch Error` whose message says why; a method it did
//! not have, by no handler.

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

/// The method channel of `url_launcher_linux` before 3.2.0.
const METHOD_CHANNEL: &str = "plugins.flutter.io/url_launcher_linux";

/// How a call came, and so how it is answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Protocol {
    /// Pigeon's, 3.2.0 on: a list holding the answer.
    Pigeon,
    /// The method channel's, before 3.2.0: a success or error envelope.
    MethodChannel,
}

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

/// A launch's reply. Pigeon's: `[null]` for opened, `[message]` for why
/// not, which the plugin raises. The method channel's: `true` in a success
/// envelope, or a `Launch Error` envelope.
fn launch_reply(protocol: Protocol, result: Result<(), String>) -> Vec<u8> {
    match (protocol, result) {
        (Protocol::Pigeon, Ok(())) => vec![12, 1, 0],
        (Protocol::Pigeon, Err(why)) => {
            let mut out = vec![12, 1];
            write_string(&mut out, &why);
            out
        }
        (Protocol::MethodChannel, Ok(())) => vec![0, 1],
        (Protocol::MethodChannel, Err(why)) => {
            method_error("Launch Error", &format!("Failed to launch URL: {why}"))
        }
    }
}

/// `StandardMethodCodec`'s error envelope: `1`, then code, message and null
/// details.
fn method_error(code: &str, message: &str) -> Vec<u8> {
    let mut out = vec![1];
    write_string(&mut out, code);
    write_string(&mut out, message);
    out.push(0);
    out
}

/// A method call, `StandardMethodCodec`'s: the method's name, a string, then
/// its argument, here the URL as a string and nothing after it. The name,
/// and the URL if the argument is one.
fn method_call(message: &[u8]) -> Option<(&str, Option<&str>)> {
    let mut at = 0;
    if *message.get(at)? != 7 {
        return None;
    }
    at += 1;
    let len = read_size(message, &mut at)?;
    let name = std::str::from_utf8(message.get(at..at + len)?).ok()?;
    at += len;
    let url = (|| {
        if *message.get(at)? != 7 {
            return None;
        }
        let mut at = at + 1;
        let len = read_size(message, &mut at)?;
        if len > MAX_URL || at + len != message.len() {
            return None;
        }
        std::str::from_utf8(&message[at..]).ok()
    })();
    Some((name, url))
}

/// Whether the platform's launcher would say yes to `url`: one of the six
/// schemes, with a launcher that has an opener.
fn can_launch(url: &str) -> bool {
    scheme_of(url).is_some_and(|s| SCHEMES.iter().any(|known| s.eq_ignore_ascii_case(known)))
        && launcher().as_ref().is_some_and(|l| l.available())
}

/// A launch of `url` if it names a scheme, else the reply that says it
/// does not: nothing without one reaches a launcher, so no URL reaches an
/// opener's command line looking like an option.
fn launch_or_refuse(protocol: Protocol, url: &str) -> Answer {
    if has_scheme(url) {
        Answer::Launch(url.to_owned(), protocol)
    } else {
        Answer::Reply(launch_reply(
            protocol,
            Err(format!("{url:?} names no scheme")),
        ))
    }
}

/// Answers a call on the method channel: `None` for a method it does not
/// have, which the caller answers as no handler.
fn handle_method_call(message: Option<&[u8]>) -> Option<Answer> {
    let Some((name, url)) = message.and_then(method_call) else {
        return Some(Answer::Reply(method_error(
            "Bad Arguments",
            "expected a method call",
        )));
    };
    if name != "canLaunch" && name != "launch" {
        return None;
    }
    let Some(url) = url else {
        return Some(Answer::Reply(method_error(
            "Bad Arguments",
            "expected a URL as a string",
        )));
    };
    Some(if name == "canLaunch" {
        Answer::Reply(vec![0, if can_launch(url) { 1 } else { 2 }])
    } else {
        launch_or_refuse(Protocol::MethodChannel, url)
    })
}

/// How a call on one of the plugin's channels is answered.
pub(crate) enum Answer {
    /// With these bytes, now.
    Reply(Vec<u8>),
    /// Once [`launch`] has opened this URL, in the reply of the protocol the
    /// call came in.
    Launch(String, Protocol),
}

/// Answers a call on one of the plugin's channels, or `None` for a channel
/// that is not one.
pub(crate) fn handle(channel: &str, message: Option<&[u8]>) -> Option<Answer> {
    if channel == METHOD_CHANNEL {
        return handle_method_call(message);
    }
    let call = call_of(channel)?;
    let Some(url) = message.and_then(url_of) else {
        return Some(Answer::Reply(error(
            "Bad Arguments",
            "expected [url], a URL as a string",
        )));
    };
    if call == CAN_LAUNCH {
        // `[true]` or `[false]`.
        return Some(Answer::Reply(vec![
            12,
            1,
            if can_launch(url) { 1 } else { 2 },
        ]));
    }
    Some(launch_or_refuse(Protocol::Pigeon, url))
}

/// Opens `url` with the platform's launcher, and gives `reply` the plugin's
/// answer when the launch is over: at once with no launcher, else whenever
/// and on whichever thread the launcher says.
pub(crate) fn launch(url: &str, protocol: Protocol, reply: impl FnOnce(Vec<u8>) + Send + 'static) {
    let guard = launcher();
    match guard.as_ref() {
        None => {
            drop(guard);
            reply(launch_reply(
                protocol,
                Err("no application here opens URLs".to_owned()),
            ));
        }
        Some(l) => l.launch(
            url,
            Box::new(move |result| reply(launch_reply(protocol, result))),
        ),
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
            Some(Answer::Launch(url, protocol)) => {
                let got = Arc::new(Mutex::new(None));
                let into = Arc::clone(&got);
                launch(&url, protocol, move |bytes| {
                    *into.lock().unwrap() = Some(bytes)
                });
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

    /// A method call as `StandardMethodCodec` writes one: the name, then
    /// the URL.
    fn method(name: &str, url: &str) -> Vec<u8> {
        let mut out = Vec::new();
        write_string(&mut out, name);
        write_string(&mut out, url);
        out
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
        method_channel_calls();
        set_url_launcher(None);
        // With no launcher, the method channel's launch says so as the
        // plugin's C side said a failure.
        match handle(METHOD_CHANNEL, Some(&method("launch", site))) {
            Some(Answer::Launch(url, protocol)) => {
                let got = Arc::new(Mutex::new(None));
                let into = Arc::clone(&got);
                launch(&url, protocol, move |bytes| {
                    *into.lock().unwrap() = Some(bytes)
                });
                let answered = got.lock().unwrap().take();
                assert_eq!(
                    answered.unwrap(),
                    method_error(
                        "Launch Error",
                        "Failed to launch URL: no application here opens URLs"
                    )
                );
            }
            _ => panic!("not a launch"),
        }
    }

    /// The method channel's calls, before 3.2.0, by the same rules: run
    /// within the pigeon test's lock on the process's launcher.
    fn method_channel_calls() {
        let site = "https://www.wonderous.info/";
        let ask = |name: &str, url: &str| match handle(METHOD_CHANNEL, Some(&method(name, url))) {
            Some(Answer::Reply(bytes)) => bytes,
            Some(Answer::Launch(url, protocol)) => {
                assert_eq!(protocol, Protocol::MethodChannel);
                let got = Arc::new(Mutex::new(None));
                let into = Arc::clone(&got);
                launch(&url, protocol, move |bytes| {
                    *into.lock().unwrap() = Some(bytes)
                });
                let answered = got.lock().unwrap().take();
                answered.expect("answered")
            }
            None => panic!("{name}: no handler"),
        };
        assert_eq!(ask("canLaunch", site), vec![0, 1]);
        assert_eq!(ask("canLaunch", "myapp://open"), vec![0, 2]);
        assert_eq!(ask("launch", site), vec![0, 1]);
        assert_eq!(
            ask("launch", "https://fail.example/"),
            method_error("Launch Error", "Failed to launch URL: exit status 3")
        );
        assert_eq!(
            ask("launch", "--help"),
            method_error(
                "Launch Error",
                "Failed to launch URL: \"--help\" names no scheme"
            )
        );
        // A method the plugin did not have: no handler.
        assert!(handle(METHOD_CHANNEL, Some(&method("closeWebView", site))).is_none());
        // Malformed: no method call, or no URL.
        for bad in [&[][..], &[12, 0][..], &[7, 9, b'c'][..]] {
            match handle(METHOD_CHANNEL, Some(bad)) {
                Some(Answer::Reply(bytes)) => {
                    assert_eq!(
                        bytes,
                        method_error("Bad Arguments", "expected a method call")
                    )
                }
                _ => panic!("{bad:?}: not refused"),
            }
        }
        let mut no_url = Vec::new();
        write_string(&mut no_url, "launch");
        no_url.push(0);
        match handle(METHOD_CHANNEL, Some(&no_url)) {
            Some(Answer::Reply(bytes)) => {
                assert_eq!(
                    bytes,
                    method_error("Bad Arguments", "expected a URL as a string")
                )
            }
            _ => panic!("no URL: not refused"),
        }
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
