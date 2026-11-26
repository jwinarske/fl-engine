//! Fuzzing the channel decoders (brief M4 §15): `flutter/textinput` and
//! `flutter/platform`'s clipboard take what the framework sends, which an
//! app can make anything of. The framework's own calls, mutated: bytes
//! flipped, runs overwritten, cut short, spliced, repeated, brackets piled
//! up; and calls made up, JSON of any shape under the methods' names.
//! Every one is answered, with a reply that is itself JSON, or is no
//! call; none panics. Keys are typed between, so the handler works on
//! whatever state a call left. Deterministic, a seeded generator. In the
//! crate, where the decoders are; it links the VM as the crate's tests do
//! and calls none of it.
//!
//!     FL_FUZZ_ITERS=20000 cargo test -p fl-ui --features aot --lib -- --ignored fuzz

use fl_input::text::Direct;
use fl_input::xkb::XkbKey;

use crate::{clipboard, json, system, text_input, url_launcher};

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
}

/// The framework's calls on the two channels, as it encodes them.
const CORPUS: [&str; 19] = [
    r#"{"method":"TextInput.setClient","args":[1,{"inputType":{"name":"TextInputType.multiline","signed":null,"decimal":null},"readOnly":false,"obscureText":false,"autocorrect":true,"enableDeltaModel":true,"inputAction":"TextInputAction.newline","textCapitalization":"TextCapitalization.none","keyboardAppearance":"Brightness.light"}]}"#,
    r#"{"method":"TextInput.updateConfig","args":{"inputType":{"name":"TextInputType.none"},"inputAction":"TextInputAction.done","enableDeltaModel":false}}"#,
    r#"{"method":"TextInput.setEditingState","args":{"text":"caf\u00e9 \ud83d\ude04 bar","selectionBase":3,"selectionExtent":7,"selectionAffinity":"TextAffinity.downstream","selectionIsDirectional":false,"composingBase":-1,"composingExtent":-1}}"#,
    r#"{"method":"TextInput.setEditingState","args":{"text":"","selectionBase":-1,"selectionExtent":-1,"composingBase":0,"composingExtent":0}}"#,
    r#"{"method":"TextInput.setEditableSizeAndTransform","args":{"width":320.0,"height":48.0,"transform":[1.0,0.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,0.0,1.0,0.0,40.0,40.0,0.0,1.0]}}"#,
    r#"{"method":"TextInput.setMarkedTextRect","args":{"width":2.0,"height":38.0,"x":120.5,"y":0.0}}"#,
    r#"{"method":"TextInput.setCaretRect","args":{"width":2.0,"height":38.0,"x":120.5,"y":0.0}}"#,
    r#"{"method":"TextInput.setStyle","args":{"fontFamily":"Roboto","fontSize":32.0,"fontWeightIndex":3,"textAlignIndex":4,"textDirectionIndex":1}}"#,
    r#"{"method":"TextInput.show","args":null}"#,
    r#"{"method":"TextInput.hide","args":null}"#,
    r#"{"method":"TextInput.clearClient","args":null}"#,
    r#"{"method":"Clipboard.setData","args":{"text":"caf\u00e9 \"bar\"\n"}}"#,
    r#"{"method":"Clipboard.getData","args":"text/plain"}"#,
    r#"{"method":"Clipboard.hasStrings","args":"text/plain"}"#,
    r#"{"method":"SystemSound.play","args":"SystemSoundType.click"}"#,
    r#"{"method":"System.initializationComplete","args":null}"#,
    r#"{"method":"System.exitApplication","args":{"type":"cancelable","exitCode":0}}"#,
    r#"{"method":"SystemNavigator.pop","args":null}"#,
    r#"[1e400, -0.0, 1E-9, "\u0000\ud800", {"a": [[[[[]]]]], "b": true}, null]"#,
];

/// `seed` changed one to eight ways.
fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut v = seed.to_vec();
    for _ in 0..1 + rng.below(8) {
        if v.is_empty() {
            v.push(rng.next() as u8);
        }
        let at = rng.below(v.len());
        match rng.below(8) {
            0 => v[at] ^= 1 << rng.below(8),
            1 => v[at] = rng.next() as u8,
            // A number's extremes, an offset no text has.
            2 => {
                let word: &[u8] = [
                    &b"-1"[..],
                    b"9223372036854775807",
                    b"-9223372036854775808",
                    b"1e999",
                    b"0.5",
                    b"null",
                    b"\"\\ud83d\"",
                    b"[]",
                ][rng.below(8)];
                v.splice(at..at, word.iter().copied());
            }
            3 => v.truncate(at),
            4 => {
                let other = CORPUS[rng.below(CORPUS.len())].as_bytes();
                let from = rng.below(other.len());
                let n = rng.below(96).min(other.len() - from);
                v.splice(at..at, other[from..from + n].iter().copied());
            }
            // Depth: brackets piled up past what is parsed.
            5 => {
                let n = 1 + rng.below(80);
                let open = [b'[', b'{'][rng.below(2)];
                v.splice(at..at, std::iter::repeat_n(open, n));
            }
            6 => v.insert(at, b"\"\\,:}]"[rng.below(6)]),
            _ => {
                let n = rng.below(32).min(v.len() - at);
                let run: Vec<u8> = v[at..at + n].to_vec();
                v.splice(at..at, run);
            }
        }
    }
    v
}

const METHODS: [&str; 18] = [
    "TextInput.setClient",
    "TextInput.updateConfig",
    "TextInput.setEditingState",
    "TextInput.setEditableSizeAndTransform",
    "TextInput.setMarkedTextRect",
    "TextInput.show",
    "TextInput.hide",
    "TextInput.clearClient",
    "TextInput.setStyle",
    "Clipboard.setData",
    "Clipboard.getData",
    "Clipboard.hasStrings",
    "Clipboard.clear",
    "SystemSound.play",
    "System.initializationComplete",
    "System.exitApplication",
    "SystemNavigator.pop",
    "",
];

/// The members the decoders look for, and one they do not.
const KEYS: [&str; 17] = [
    "text",
    "selectionBase",
    "selectionExtent",
    "composingBase",
    "composingExtent",
    "inputType",
    "name",
    "inputAction",
    "enableDeltaModel",
    "transform",
    "x",
    "y",
    "width",
    "height",
    "method",
    "type",
    "other",
];

/// A JSON value of any shape, written: what a call's arguments are made
/// of when they are JSON and not the method's.
fn value(rng: &mut Rng, depth: usize, out: &mut String) {
    match rng.below(if depth >= 4 { 8 } else { 11 }) {
        0 => out.push_str("null"),
        1 => out.push_str(["true", "false"][rng.below(2)]),
        2 => out.push_str(&(rng.below(12) as i64 - 2).to_string()),
        3 => out.push_str(
            [
                "9223372036854775807",
                "-9223372036854775808",
                "1e999",
                "-1e999",
                "0.5",
                "1e-320",
                "18446744073709551616",
                "-0.0",
            ][rng.below(8)],
        ),
        4 => json::write_string(
            out,
            [
                "",
                "caf\u{e9}",
                "a\u{1f604}\"\\\n",
                "text/plain",
                "image/png",
                "TextInputType.multiline",
                "TextInputType.none",
                "TextInputAction.newline",
            ][rng.below(8)],
        ),
        5 => out.push_str("\"\\ud83d\""),
        6 => out.push_str("\"\\ud83d\\ude04\\u0000\""),
        7 => out.push_str(&format!("{}.0", rng.below(400))),
        8 | 9 => {
            out.push('{');
            for i in 0..rng.below(8) {
                if i > 0 {
                    out.push(',');
                }
                json::write_string(out, KEYS[rng.below(KEYS.len())]);
                out.push(':');
                value(rng, depth + 1, out);
            }
            out.push('}');
        }
        _ => {
            out.push('[');
            // Sixteen now and then: a transform's length.
            let n = if rng.below(4) == 0 { 16 } else { rng.below(5) };
            for i in 0..n {
                if i > 0 {
                    out.push(',');
                }
                value(rng, depth + 1, out);
            }
            out.push(']');
        }
    }
}

/// A method call that is JSON, of a method the channels have or have
/// not, with arguments of any shape.
fn call(rng: &mut Rng) -> Vec<u8> {
    let mut out = String::from("{\"method\":");
    json::write_string(&mut out, METHODS[rng.below(METHODS.len())]);
    if rng.below(8) != 0 {
        out.push_str(",\"args\":");
        value(rng, 0, &mut out);
    }
    out.push('}');
    out.into_bytes()
}

/// A reply is a `JSONMethodCodec` envelope: an array of one, or of three
/// with a code and a message.
fn is_envelope(reply: &[u8]) -> bool {
    match json::parse(reply) {
        Some(json::Json::Array(a)) => {
            a.len() == 1 || (a.len() == 3 && a[0].as_str().is_some() && a[1].as_str().is_some())
        }
        _ => false,
    }
}

#[test]
#[ignore = "fuzz: FL_FUZZ_ITERS=20000 cargo test -p fl-ui --features aot --lib -- --ignored fuzz"]
#[allow(clippy::panic)]
fn channels_answer_every_mutation() {
    let iters: usize = std::env::var("FL_FUZZ_ITERS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(2000);
    let mut rng = Rng(0x5eed_f1e1_0000_0007);
    text_input::set_input_method(Box::new(Direct));
    let (mut answered, mut ignored, mut typed) = (0usize, 0usize, 0usize);
    for i in 0..iters {
        // Half the framework's calls with their bytes changed, most of
        // which are no JSON by then; half JSON of the wrong shape.
        let input = if i % 2 == 0 {
            mutate(&mut rng, CORPUS[i % CORPUS.len()].as_bytes())
        } else {
            call(&mut rng)
        };
        // The parser alone, on anything.
        let _ = json::parse(&input);
        // url_launcher's decoder, with no launcher set: whatever it is given
        // is answered, and nothing is opened.
        for channel in [
            "dev.flutter.pigeon.url_launcher_linux.UrlLauncherApi.canLaunchUrl",
            "dev.flutter.pigeon.url_launcher_linux.UrlLauncherApi.launchUrl",
        ] {
            // A launch is not made: with no launcher it answers at once,
            // and the decoder is what is under test.
            assert!(url_launcher::handle(channel, Some(&input)).is_some());
        }
        // The method channel before 3.2.0: answered, or no handler for a
        // method it does not have, never a panic.
        let _ = url_launcher::handle("plugins.flutter.io/url_launcher_linux", Some(&input));
        // An exit put to the framework has no reply yet: the empty one
        // stands for it, and the next is not refused as in progress.
        let system = system::handle_call(Some(&input)).map(|a| match a {
            system::Answer::Reply(reply) => reply,
            system::Answer::AskFramework => {
                system::reset();
                b"[null]".to_vec()
            }
        });
        for reply in [
            text_input::handle_call(Some(&input)),
            clipboard::handle_call(Some(&input)),
            system,
        ] {
            match reply {
                Some(reply) => {
                    answered += 1;
                    assert!(
                        is_envelope(&reply),
                        "{:?} answered {:?}",
                        String::from_utf8_lossy(&input),
                        String::from_utf8_lossy(&reply)
                    );
                }
                None => ignored += 1,
            }
        }
        // A key, on whatever the call left the field as.
        let c = [0x61, 0xe9, 0x1f604, 0x0d, 0][rng.below(5)];
        let key = XkbKey {
            pressed: true,
            keyval: [c, 0xff0d, 0xff50, 0xff57][rng.below(4)],
            unicode: c,
            state: [0, 1, 4][rng.below(3)],
            ..XkbKey::default()
        };
        for message in text_input::filter_key(&key).1 {
            typed += 1;
            // What the framework is sent is JSON it can read.
            assert!(json::method_call(&message).is_some());
        }
    }
    // The corpus's own calls, unchanged: each is answered by its channel.
    for call in &CORPUS[..14] {
        let bytes = call.as_bytes();
        let known = !call.contains("setCaretRect") && !call.contains("setStyle");
        let reply =
            text_input::handle_call(Some(bytes)).or_else(|| clipboard::handle_call(Some(bytes)));
        assert_eq!(reply.is_some(), known, "{call}");
    }
    text_input::reset();
    system::reset();
    println!("{iters} calls: {answered} answered, {ignored} not calls; {typed} messages typed");
    assert!(answered > iters / 20 && ignored > iters / 20 && typed > iters / 50);
}
