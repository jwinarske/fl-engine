//! The scripted input source (brief M4 §4): events from a file, one step a
//! line, delivered as a board's devices deliver theirs. Everything after
//! the [`EventSink`] is the board's path.
//!
//! ```text
//! # A comment. Positions are physical pixels in the view.
//! tap 756 556       a touch down and up at a point
//! down 100 300      a touch down, and where it is until it moves
//! move 100 200      the touch moved, down
//! up 100 200        the touch lifted
//! mouse move 10 20  the mouse moved: hovering, or dragging with its
//!                   button down
//! mouse down 10 20  the mouse's primary button pressed, there
//! mouse up 90 20    the button released
//! idle              wait until the app has nothing to do: every event
//!                   delivered has been dispatched, every frame asked for
//!                   drawn (an animation run to its end)
//! wait 250          wait 250 ms
//! key keyA down     a key going down, named as Flutter names its place
//!                   on the keyboard (`PhysicalKeyboardKey`: `keyA`,
//!                   `shiftLeft`, `numpad1`); down again is a repeat
//! key keyA up       the key coming up
//! key enter         the key down, then up
//! ```
//!
//! A script starts as if with `idle`, so its first event finds the app's
//! first frame laid out. A touch is delivered as the Linux embedder's
//! `fl_touch_manager` delivers one: added as it goes down, removed as it
//! lifts. The mouse is its `fl_pointer_manager`'s: added by its first
//! event, hovering with no button down, and never removed.
//!
//! A key is a place on the keyboard, as a device reports one. What it
//! means is the keymap's, which whoever plays the script brings
//! ([`Keymap`]), the one a board's keys go through: the layout's keysym,
//! its character, the modifiers held. From there it is the Linux
//! embedder's key path ([`fl_input::xkb`]).
//!
//! A script is untrusted input (brief M4 §15): what does not parse is an
//! error naming its line, never a panic, and its size, its waits and its
//! positions are bounded.

use std::fmt;
use std::time::Duration;

use fl_input::xkb::{keycode_named, Keymap, Keys};
use fl_input::{mouse_button, EventSink, PointerDeviceKind, PointerEvent, PointerPhase};

/// A script's size limit: far more than any test's, and small enough to
/// read whole.
pub const MAX_BYTES: usize = 1 << 20;

/// The longest one `wait`, ten minutes: a soak's worth.
pub const MAX_WAIT_MS: u64 = 600_000;

/// Positions are within this of the origin: beyond any view, and well
/// inside what a double holds exactly.
pub const MAX_POSITION: f64 = 1e6;

/// One step of a script.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    Tap {
        x: f64,
        y: f64,
    },
    Down {
        x: f64,
        y: f64,
    },
    Move {
        x: f64,
        y: f64,
    },
    Up {
        x: f64,
        y: f64,
    },
    /// The mouse: moved, or its primary button pressed or released.
    Mouse {
        action: MouseAction,
        x: f64,
        y: f64,
    },
    Idle,
    Wait(Duration),
    /// A key by its XKB keycode: down, up, or with `None` down then up.
    Key {
        keycode: u16,
        pressed: Option<bool>,
    },
}

/// What the mouse did, in a [`Step::Mouse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Move,
    Down,
    Up,
}

/// A parsed script.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Script {
    pub steps: Vec<Step>,
}

/// Why a script did not parse: the line, from 1, and what is wrong with it.
/// Line 0 is the script as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

impl Script {
    /// Parses a script. Blank lines and what follows a `#` are skipped.
    pub fn parse(text: &str) -> Result<Script, ParseError> {
        if text.len() > MAX_BYTES {
            return Err(ParseError {
                line: 0,
                message: format!("{} bytes; the limit is {MAX_BYTES}", text.len()),
            });
        }
        let mut steps = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or_default();
            let mut words = line.split_whitespace();
            let Some(command) = words.next() else {
                continue;
            };
            let args: Vec<&str> = words.collect();
            let step = parse_step(command, &args).map_err(|message| ParseError {
                line: i + 1,
                message,
            })?;
            steps.push(step);
        }
        Ok(Script { steps })
    }

    /// Whether the script has a key step, and so needs a keymap to play.
    pub fn has_keys(&self) -> bool {
        self.steps.iter().any(|s| matches!(s, Step::Key { .. }))
    }

    /// Plays the script into `sink`, on this thread: waits for the app to
    /// go idle, then each step in turn. Returns when the last has been
    /// delivered, or early if the loop ends first.
    ///
    /// Keys are resolved against `keymap`. With none, key steps are
    /// skipped: a caller with no keymap asks [`Script::has_keys`] first.
    pub fn play(&self, sink: &dyn EventSink, mut keymap: Option<&mut dyn Keymap>) {
        if !sink.wait_idle() {
            return;
        }
        // The keyboard's key path, the script's own as a seat's is.
        let mut keys = Keys::new();
        // The mouse: whether the framework knows of it, and its button.
        let (mut mouse_added, mut mouse_down) = (false, false);
        for step in &self.steps {
            match *step {
                Step::Tap { x, y } => {
                    let at = sink.now();
                    sink.pointer_events(&[
                        touch(PointerPhase::Add, at, x, y),
                        touch(PointerPhase::Down, at, x, y),
                        touch(PointerPhase::Up, at, x, y),
                        touch(PointerPhase::Remove, at, x, y),
                    ]);
                }
                Step::Down { x, y } => {
                    let at = sink.now();
                    sink.pointer_events(&[
                        touch(PointerPhase::Add, at, x, y),
                        touch(PointerPhase::Down, at, x, y),
                    ]);
                }
                Step::Move { x, y } => {
                    sink.pointer_events(&[touch(PointerPhase::Move, sink.now(), x, y)]);
                }
                Step::Up { x, y } => {
                    let at = sink.now();
                    sink.pointer_events(&[
                        touch(PointerPhase::Up, at, x, y),
                        touch(PointerPhase::Remove, at, x, y),
                    ]);
                }
                Step::Mouse { action, x, y } => {
                    let at = sink.now();
                    let mut events = Vec::with_capacity(2);
                    if !std::mem::replace(&mut mouse_added, true) {
                        events.push(mouse(PointerPhase::Add, at, x, y, false));
                    }
                    match (action, mouse_down) {
                        (MouseAction::Move, true) => {
                            events.push(mouse(PointerPhase::Move, at, x, y, true));
                        }
                        (MouseAction::Move, false) => {
                            events.push(mouse(PointerPhase::Hover, at, x, y, false));
                        }
                        (MouseAction::Down, false) => {
                            mouse_down = true;
                            events.push(mouse(PointerPhase::Down, at, x, y, true));
                        }
                        (MouseAction::Up, true) => {
                            mouse_down = false;
                            events.push(mouse(PointerPhase::Up, at, x, y, false));
                        }
                        // Down while down, up while up: a button's state
                        // told twice is no event.
                        (MouseAction::Down, true) | (MouseAction::Up, false) => {}
                    }
                    if !events.is_empty() {
                        sink.pointer_events(&events);
                    }
                }
                Step::Idle => {
                    if !sink.wait_idle() {
                        return;
                    }
                }
                Step::Wait(d) => {
                    if !wait(sink, d) {
                        return;
                    }
                }
                Step::Key { keycode, pressed } => {
                    let Some(keymap) = keymap.as_deref_mut() else {
                        continue;
                    };
                    let directions: &[bool] = match pressed {
                        Some(true) => &[true],
                        Some(false) => &[false],
                        None => &[true, false],
                    };
                    for pressed in directions {
                        let key = keymap.key(sink.now(), keycode, *pressed);
                        let event = keys.handle(&key, |c, g, l| keymap.lookup(c, g, l));
                        sink.key_event(event);
                    }
                }
            }
        }
    }
}

/// Sleeps `d`, a little at a time so a loop that ends is noticed within
/// [`WAIT_SLICE`]. `false` if it did end.
fn wait(sink: &dyn EventSink, d: Duration) -> bool {
    let mut left = d;
    while !left.is_zero() {
        if sink.is_closed() {
            return false;
        }
        let slice = left.min(WAIT_SLICE);
        std::thread::sleep(slice);
        left -= slice;
    }
    !sink.is_closed()
}

const WAIT_SLICE: Duration = Duration::from_millis(20);

/// The script's one finger.
fn touch(phase: PointerPhase, at: Duration, x: f64, y: f64) -> PointerEvent {
    PointerEvent::new(phase, PointerDeviceKind::Touch, at, x, y)
}

/// The mouse's device: not the touch's, which is 0, since the framework's
/// pointers are told apart by device.
const MOUSE_DEVICE: i32 = 1;

/// The script's mouse, its primary button down or not.
fn mouse(phase: PointerPhase, at: Duration, x: f64, y: f64, down: bool) -> PointerEvent {
    PointerEvent {
        device: MOUSE_DEVICE,
        buttons: if down { mouse_button::PRIMARY } else { 0 },
        ..PointerEvent::new(phase, PointerDeviceKind::Mouse, at, x, y)
    }
}

fn parse_step(command: &str, args: &[&str]) -> Result<Step, String> {
    let point = || -> Result<(f64, f64), String> {
        let [x, y] = args else {
            return Err(format!("`{command}` takes a position, x and y"));
        };
        Ok((position(x)?, position(y)?))
    };
    match command {
        "tap" => point().map(|(x, y)| Step::Tap { x, y }),
        "down" => point().map(|(x, y)| Step::Down { x, y }),
        "move" => point().map(|(x, y)| Step::Move { x, y }),
        "up" => point().map(|(x, y)| Step::Up { x, y }),
        "mouse" => {
            let (action, x, y) = match args {
                ["move", x, y] => (MouseAction::Move, x, y),
                ["down", x, y] => (MouseAction::Down, x, y),
                ["up", x, y] => (MouseAction::Up, x, y),
                _ => {
                    return Err(
                        "`mouse` takes `move`, `down` or `up`, then a position, x and y".to_owned(),
                    )
                }
            };
            Ok(Step::Mouse {
                action,
                x: position(x)?,
                y: position(y)?,
            })
        }
        "idle" if args.is_empty() => Ok(Step::Idle),
        "idle" => Err("`idle` takes nothing".to_owned()),
        "wait" => {
            let [ms] = args else {
                return Err("`wait` takes milliseconds".to_owned());
            };
            let ms: u64 = ms
                .parse()
                .map_err(|_| format!("`{}` is not a count of milliseconds", clip(ms)))?;
            if ms > MAX_WAIT_MS {
                return Err(format!("a wait of {ms} ms; the limit is {MAX_WAIT_MS}"));
            }
            Ok(Step::Wait(Duration::from_millis(ms)))
        }
        "key" => {
            let (name, pressed) = match args {
                [name] => (name, None),
                [name, "down"] => (name, Some(true)),
                [name, "up"] => (name, Some(false)),
                _ => {
                    return Err(
                        "`key` takes a key's name, then `down` or `up` or neither".to_owned()
                    )
                }
            };
            let keycode =
                keycode_named(name).ok_or_else(|| format!("`{}` is not a key", clip(name)))?;
            Ok(Step::Key { keycode, pressed })
        }
        other => Err(format!("`{}` is not a step", clip(other))),
    }
}

fn position(word: &str) -> Result<f64, String> {
    match word.parse::<f64>() {
        Ok(v) if v.is_finite() && v.abs() <= MAX_POSITION => Ok(v),
        Ok(_) => Err(format!("`{}` is out of range", clip(word))),
        Err(_) => Err(format!("`{}` is not a number", clip(word))),
    }
}

/// A word for an error message, cut short: a script's line can be a
/// megabyte long.
fn clip(word: &str) -> String {
    const MAX: usize = 32;
    if word.chars().count() <= MAX {
        return word.to_owned();
    }
    let mut cut: String = word.chars().take(MAX).collect();
    cut.push_str("...");
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use fl_input::xkb::XkbKey;
    use fl_input::KeyEvent;
    use std::sync::Mutex;

    #[test]
    fn steps_parse_a_line_each() {
        let s = Script::parse(
            "# taps\n\n  tap 756 556.5  # the button\nidle\ndown 1 2\nmove -3 4e1\nup 3 40\nwait 250\n\
             key keyA down\nkey keyA up\nkey enter\nmouse down 4 5\n",
        );
        assert_eq!(
            s.map(|s| s.steps),
            Ok(vec![
                Step::Tap { x: 756.0, y: 556.5 },
                Step::Idle,
                Step::Down { x: 1.0, y: 2.0 },
                Step::Move { x: -3.0, y: 40.0 },
                Step::Up { x: 3.0, y: 40.0 },
                Step::Wait(Duration::from_millis(250)),
                Step::Key {
                    keycode: 0x26,
                    pressed: Some(true)
                },
                Step::Key {
                    keycode: 0x26,
                    pressed: Some(false)
                },
                Step::Key {
                    keycode: 0x24,
                    pressed: None
                },
                Step::Mouse {
                    action: MouseAction::Down,
                    x: 4.0,
                    y: 5.0
                },
            ])
        );
        assert_eq!(Script::parse("").map(|s| s.steps.len()), Ok(0));
    }

    #[test]
    fn what_does_not_parse_names_its_line() {
        let cases = [
            ("tap 1", 1, "`tap` takes a position, x and y"),
            ("idle\ntap 1 2 3", 2, "`tap` takes a position, x and y"),
            ("tap x 2", 1, "`x` is not a number"),
            ("tap 1 nan", 1, "`nan` is out of range"),
            ("tap 1 inf", 1, "`inf` is out of range"),
            ("move 1e7 0", 1, "`1e7` is out of range"),
            ("idle now", 1, "`idle` takes nothing"),
            ("wait", 1, "`wait` takes milliseconds"),
            ("wait -1", 1, "`-1` is not a count of milliseconds"),
            ("wait 600001", 1, "a wait of 600001 ms; the limit is 600000"),
            ("\n\nclick 1 2", 3, "`click` is not a step"),
            (
                "key",
                1,
                "`key` takes a key's name, then `down` or `up` or neither",
            ),
            (
                "key keyA sideways",
                1,
                "`key` takes a key's name, then `down` or `up` or neither",
            ),
            ("key a", 1, "`a` is not a key"),
            (
                "mouse 1 2",
                1,
                "`mouse` takes `move`, `down` or `up`, then a position, x and y",
            ),
            (
                "mouse click 1 2",
                1,
                "`mouse` takes `move`, `down` or `up`, then a position, x and y",
            ),
            ("mouse down 1 x", 1, "`x` is not a number"),
            ("key KeyA down", 1, "`KeyA` is not a key"),
        ];
        for (text, line, message) in cases {
            let e = Script::parse(text).err();
            assert_eq!(
                e,
                Some(ParseError {
                    line,
                    message: message.to_owned()
                }),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_script_is_bounded_in_size_and_its_errors_in_length() {
        let big = "idle\n".repeat(MAX_BYTES / 5 + 1);
        assert_eq!(Script::parse(&big).err().map(|e| e.line), Some(0));
        let long = "x".repeat(10_000);
        let e = Script::parse(&long)
            .err()
            .map(|e| e.message)
            .unwrap_or_default();
        assert!(e.len() < 80, "{e}");
        // Cut on a character, not in one.
        let wide = "\u{1f600}".repeat(100);
        assert!(Script::parse(&wide).is_err());
    }

    /// A sink that records what it is given, and is closed after a number
    /// of waits for idle.
    struct Recorder {
        log: Mutex<Vec<String>>,
        idles_left: Mutex<usize>,
    }

    impl Recorder {
        fn new(idles: usize) -> Recorder {
            Recorder {
                log: Mutex::new(Vec::new()),
                idles_left: Mutex::new(idles),
            }
        }

        fn log(&self) -> Vec<String> {
            self.log.lock().map(|l| l.clone()).unwrap_or_default()
        }
    }

    impl EventSink for Recorder {
        fn now(&self) -> Duration {
            Duration::from_millis(7)
        }

        fn pointer_events(&self, events: &[PointerEvent]) {
            let line: Vec<String> = events
                .iter()
                .map(|e| {
                    assert_eq!((e.view_id, e.timestamp), (0, self.now()));
                    match e.device_kind {
                        PointerDeviceKind::Touch => {
                            assert_eq!((e.device, e.buttons), (0, 0));
                            format!("{:?}@{},{}", e.phase, e.x, e.y)
                        }
                        kind => {
                            assert_eq!((kind, e.device), (PointerDeviceKind::Mouse, 1));
                            format!("mouse:{:?}@{},{} buttons={}", e.phase, e.x, e.y, e.buttons)
                        }
                    }
                })
                .collect();
            if let Ok(mut log) = self.log.lock() {
                log.push(line.join(" "));
            }
        }

        fn key_event(&self, event: KeyEvent) {
            let line: Vec<String> = event
                .data
                .iter()
                .map(|d| {
                    assert_eq!(d.timestamp, self.now());
                    let character = d.character.as_deref().unwrap_or("");
                    format!(
                        "{:?}:{:#x}:{:#x}:{character}",
                        d.kind, d.physical, d.logical
                    )
                })
                .collect();
            let raw =
                String::from_utf8_lossy(event.raw.as_deref().unwrap_or_default()).into_owned();
            if let Ok(mut log) = self.log.lock() {
                log.push(format!("{} {raw}", line.join(" ")));
            }
        }

        fn wait_idle(&self) -> bool {
            let Ok(mut left) = self.idles_left.lock() else {
                return false;
            };
            if *left == 0 {
                return false;
            }
            *left -= 1;
            if let Ok(mut log) = self.log.lock() {
                log.push("idle".to_owned());
            }
            true
        }

        fn is_closed(&self) -> bool {
            self.idles_left.lock().map(|l| *l == 0).unwrap_or(true)
        }
    }

    /// A keymap of one key level: every key gives its keycode as its
    /// keysym, with no state.
    struct Plain;

    impl Keymap for Plain {
        fn key(&mut self, time: Duration, keycode: u16, pressed: bool) -> XkbKey {
            XkbKey {
                time,
                pressed,
                keycode,
                keyval: u32::from(keycode),
                unicode: u32::from(keycode),
                ..XkbKey::default()
            }
        }

        fn lookup(&self, _keycode: u16, _group: u8, _level: u8) -> u32 {
            0
        }
    }

    #[test]
    fn a_key_goes_through_the_keymap_and_the_embedder_s_key_path() {
        let script = Script::parse("key keyA down\nkey keyA down\nkey keyA up\nkey escape\n")
            .unwrap_or_default();
        assert!(script.has_keys());
        let sink = Recorder::new(usize::MAX);
        script.play(&sink, Some(&mut Plain));
        let raw = |kind: &str, code: u32, logical: &str| {
            format!(
                "{{\"type\":\"{kind}\",\"keymap\":\"linux\",\"scanCode\":{code},\"toolkit\":\"gtk\",\
                 \"keyCode\":{code},\"modifiers\":0,\"unicodeScalarValues\":{code}{logical}}}"
            )
        };
        // Key A's keycode, 0x26, is `&` as a keysym; the layout says `a`.
        let a = ",\"specifiedLogicalKey\":97";
        assert_eq!(
            sink.log(),
            [
                "idle".to_owned(),
                format!("Down:0x70004:0x61:& {}", raw("keydown", 0x26, a)),
                format!("Repeat:0x70004:0x61:& {}", raw("keydown", 0x26, a)),
                format!("Up:0x70004:0x61: {}", raw("keyup", 0x26, a)),
                // Escape is no goal of the layout: its keysym decides.
                format!("Down:0x70029:0x9:\t {}", raw("keydown", 9, "")),
                format!("Up:0x70029:0x9: {}", raw("keyup", 9, "")),
            ]
        );
    }

    #[test]
    fn with_no_keymap_keys_are_skipped() {
        let script = Script::parse("key keyA\ntap 1 1\n").unwrap_or_default();
        let sink = Recorder::new(usize::MAX);
        script.play(&sink, None);
        assert_eq!(sink.log(), ["idle", "Add@1,1 Down@1,1 Up@1,1 Remove@1,1"]);
        assert!(!Script::parse("tap 1 1").unwrap_or_default().has_keys());
    }

    #[test]
    fn a_touch_is_added_as_it_goes_down_and_removed_as_it_lifts() {
        let script = Script::parse("tap 5 6\nidle\ndown 1 2\nmove 3 4\nup 3 4\nwait 1\n");
        let sink = Recorder::new(usize::MAX);
        script.unwrap_or_default().play(&sink, None);
        assert_eq!(
            sink.log(),
            [
                // Before the first step, unasked.
                "idle",
                "Add@5,6 Down@5,6 Up@5,6 Remove@5,6",
                "idle",
                "Add@1,2 Down@1,2",
                "Move@3,4",
                "Up@3,4 Remove@3,4",
            ]
        );
    }

    /// The mouse as `fl_pointer_manager` sends it: added once, hovering
    /// until its button is down, moving while it is, and beside the
    /// touch, not in its place.
    #[test]
    fn a_mouse_hovers_and_drags() {
        let script = Script::parse(
            "mouse move 1 2\nmouse down 1 2\nmouse down 1 2\nmouse move 3 4\nmouse up 3 4\n\
             mouse up 3 4\nmouse move 5 6\ntap 7 8\n",
        );
        let sink = Recorder::new(usize::MAX);
        script.unwrap_or_default().play(&sink, None);
        assert_eq!(
            sink.log(),
            [
                "idle",
                "mouse:Add@1,2 buttons=0 mouse:Hover@1,2 buttons=0",
                "mouse:Down@1,2 buttons=1",
                "mouse:Move@3,4 buttons=1",
                "mouse:Up@3,4 buttons=0",
                "mouse:Hover@5,6 buttons=0",
                "Add@7,8 Down@7,8 Up@7,8 Remove@7,8",
            ]
        );
        // A script that starts with the button: added there.
        let script = Script::parse("mouse down 9 9\n");
        let sink = Recorder::new(usize::MAX);
        script.unwrap_or_default().play(&sink, None);
        assert_eq!(
            sink.log(),
            ["idle", "mouse:Add@9,9 buttons=0 mouse:Down@9,9 buttons=1"]
        );
    }

    #[test]
    fn a_loop_that_ends_ends_the_script() {
        let script = Script::parse("tap 1 1\nidle\ntap 2 2\nwait 600000\ntap 3 3\n");
        // The first idle is the script's own; the second never comes.
        let sink = Recorder::new(1);
        script.clone().unwrap_or_default().play(&sink, None);
        assert_eq!(sink.log(), ["idle", "Add@1,1 Down@1,1 Up@1,1 Remove@1,1"]);
        // Not even the first.
        let sink = Recorder::new(0);
        script.unwrap_or_default().play(&sink, None);
        assert!(sink.log().is_empty());
    }
}
