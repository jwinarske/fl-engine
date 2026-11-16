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
//! down 300 300 1    another finger, numbered: any step of a touch takes
//!                   a finger's number after its position, 0 if none
//! mouse move 10 20  the mouse moved: hovering, or dragging with a
//!                   button down
//! mouse down 10 20  the mouse's primary button pressed, there
//! mouse up 90 20    the button released
//! mouse down 10 20 secondary
//!                   another button: `primary`, `secondary`, `middle`,
//!                   `back`, `forward`
//! mouse leave 0 20  the mouse left the view
//! scroll 10 20 0 1  the wheel turned with the mouse there: notches
//!                   across and down, a fraction for a smooth wheel
//! pan 10 20 0 -1    two fingers moved on a touchpad, by a wheel's
//!                   measure: the content follows them
//! pan end 10 20     the fingers lifted
//! zoom begin        a pinch on a touchpad began, where the mouse is
//! zoom 1.5          the fingers are 1.5 times as far apart
//! zoom end
//! rotate begin      a rotation on a touchpad began
//! rotate 0.5        the fingers have turned half a radian
//! rotate end
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
//! first frame laid out. Pointers are delivered as the Linux embedder's
//! managers deliver them ([`fl_input::pointer`]): a touch added as it goes
//! down and removed as it lifts, a mouse added by its first event and
//! hovering with no button down, a touchpad's gestures as pan-zoom.
//!
//! An event's time is the script's own: when the script began, plus its
//! `wait`s. So a gesture's speed, which the framework reads from its
//! events' times, is what the script says and not what the sleeps came
//! to. `idle` brings the script's time up to the clock's.
//!
//! A key is a place on the keyboard, as a device reports one. What it
//! means is the keymap's, which whoever plays the script brings
//! ([`Keymap`]), the one a board's keys go through: the layout's keysym,
//! its character, the modifiers held. From there it is the Linux
//! embedder's key path ([`fl_input::xkb`]).
//!
//! A script is untrusted input (brief M4 §15): what does not parse is an
//! error naming its line, never a panic, and its size, its waits, its
//! positions and its distances are bounded.

use std::fmt;
use std::time::Duration;

use fl_input::pointer::{button, At, Mouse, Scroll, Scrolling, TouchAction, Touches, MAX_TOUCHES};
use fl_input::xkb::{keycode_named, Keymap, Keys};
use fl_input::{EventSink, PointerEvent};

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
    /// A finger down and up at a point.
    Tap {
        x: f64,
        y: f64,
        finger: u32,
    },
    Down {
        x: f64,
        y: f64,
        finger: u32,
    },
    Move {
        x: f64,
        y: f64,
        finger: u32,
    },
    Up {
        x: f64,
        y: f64,
        finger: u32,
    },
    /// The mouse: moved, a button pressed or released, or gone.
    Mouse {
        action: MouseAction,
        x: f64,
        y: f64,
    },
    /// The wheel turned, in notches, with the mouse at a position.
    Scroll {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
    },
    /// Two fingers moved on a touchpad, by a wheel's measure.
    Pan {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
    },
    /// They lifted.
    PanEnd {
        x: f64,
        y: f64,
    },
    /// A pinch on a touchpad; its update is the scale.
    Zoom(Gesture),
    /// A rotation on a touchpad; its update is the angle, in radians.
    Rotate(Gesture),
    Idle,
    Wait(Duration),
    /// A key by its XKB keycode: down, up, or with `None` down then up.
    Key {
        keycode: u16,
        pressed: Option<bool>,
    },
}

/// What the mouse did, in a [`Step::Mouse`]. A button is GDK's number
/// ([`fl_input::pointer::button`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Move,
    Down(u32),
    Up(u32),
    Leave,
}

/// A step of a touchpad's pinch or rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    Begin,
    Update(f64),
    End,
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
    /// `scale` is the view's physical pixels to a logical one, which a
    /// wheel's notch is measured in.
    pub fn play(&self, sink: &dyn EventSink, mut keymap: Option<&mut dyn Keymap>, scale: f64) {
        if !sink.wait_idle() {
            return;
        }
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        // The devices' paths, the script's own as a seat's are.
        let mut keys = Keys::new();
        let mut mouse = Mouse::new(VIEW);
        let mut scrolling = Scrolling::new(VIEW);
        let mut touches = Touches::new(VIEW);
        // The script's time: the clock's when it began, and its waits.
        let mut at = sink.now();
        let mut events: Vec<PointerEvent> = Vec::new();
        for step in &self.steps {
            match *step {
                Step::Tap { x, y, finger } => {
                    touches.touch(at, TouchAction::Begin, finger, x, y, &mut events);
                    touches.touch(at, TouchAction::End, finger, x, y, &mut events);
                }
                Step::Down { x, y, finger } => {
                    touches.touch(at, TouchAction::Begin, finger, x, y, &mut events);
                }
                Step::Move { x, y, finger } => {
                    touches.touch(at, TouchAction::Update, finger, x, y, &mut events);
                }
                Step::Up { x, y, finger } => {
                    touches.touch(at, TouchAction::End, finger, x, y, &mut events);
                }
                Step::Mouse { action, x, y } => {
                    let place = At::mouse(at, x, y);
                    match action {
                        MouseAction::Move => {
                            mouse.motion(&place, &mut events);
                            scrolling.set_last_mouse_position(x, y);
                        }
                        MouseAction::Down(b) => {
                            mouse.button_press(&place, b, &mut events);
                        }
                        MouseAction::Up(b) => {
                            mouse.button_release(&place, b, &mut events);
                        }
                        MouseAction::Leave => mouse.leave(&place, &mut events),
                    }
                }
                Step::Scroll { x, y, dx, dy } => {
                    scrolling.scroll(&scroll(at, x, y, dx, dy, false, false), scale, &mut events);
                }
                Step::Pan { x, y, dx, dy } => {
                    scrolling.scroll(&scroll(at, x, y, dx, dy, true, false), scale, &mut events);
                }
                Step::PanEnd { x, y } => {
                    scrolling.scroll(&scroll(at, x, y, 0.0, 0.0, true, true), scale, &mut events);
                }
                Step::Zoom(Gesture::Begin) => scrolling.zoom_begin(at, &mut events),
                Step::Zoom(Gesture::Update(v)) => scrolling.zoom_update(at, v, &mut events),
                Step::Zoom(Gesture::End) => scrolling.zoom_end(at, &mut events),
                Step::Rotate(Gesture::Begin) => scrolling.rotation_begin(at, &mut events),
                Step::Rotate(Gesture::Update(v)) => scrolling.rotation_update(at, v, &mut events),
                Step::Rotate(Gesture::End) => scrolling.rotation_end(at, &mut events),
                Step::Idle => {
                    if !sink.wait_idle() {
                        return;
                    }
                    at = at.max(sink.now());
                }
                Step::Wait(d) => {
                    if !wait(sink, d) {
                        return;
                    }
                    at += d;
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
                        let key = keymap.key(at, keycode, *pressed);
                        let event = keys.handle(&key, |c, g, l| keymap.lookup(c, g, l));
                        sink.key_event(event);
                    }
                }
            }
            // Each event is delivered on its own, as the embedder sends
            // each: the framework settles one before it is told the next.
            // A step that changed nothing is no event: a button told down
            // twice, a leave with no mouse.
            for event in events.drain(..) {
                sink.pointer_events(std::slice::from_ref(&event));
            }
        }
    }
}

/// The one view.
const VIEW: i64 = 0;

fn scroll(at: Duration, x: f64, y: f64, dx: f64, dy: f64, touchpad: bool, stop: bool) -> Scroll {
    Scroll {
        time: at,
        x,
        y,
        delta_x: dx,
        delta_y: dy,
        touchpad,
        stop,
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

/// The longest a wheel or a pan goes in one step, in notches: well past
/// any content, and a distance a double still adds exactly.
pub const MAX_NOTCHES: f64 = 1e6;

fn parse_step(command: &str, args: &[&str]) -> Result<Step, String> {
    // A position, and after it a finger's number or nothing.
    let touch = || -> Result<(f64, f64, u32), String> {
        let (x, y, finger) = match args {
            [x, y] => (x, y, 0),
            [x, y, finger] => (x, y, finger_number(finger)?),
            _ => {
                return Err(format!(
                    "`{command}` takes a position, x and y, and a finger's number or none"
                ))
            }
        };
        Ok((position(x)?, position(y)?, finger))
    };
    match command {
        "tap" => touch().map(|(x, y, finger)| Step::Tap { x, y, finger }),
        "down" => touch().map(|(x, y, finger)| Step::Down { x, y, finger }),
        "move" => touch().map(|(x, y, finger)| Step::Move { x, y, finger }),
        "up" => touch().map(|(x, y, finger)| Step::Up { x, y, finger }),
        "mouse" => {
            let usage = || {
                "`mouse` takes `move`, `down`, `up` or `leave`, then a position, x and y, \
                 and for `down` and `up` a button or none"
                    .to_owned()
            };
            let (action, x, y) = match args {
                ["move", x, y] => (MouseAction::Move, x, y),
                ["leave", x, y] => (MouseAction::Leave, x, y),
                ["down", x, y] => (MouseAction::Down(button::PRIMARY), x, y),
                ["up", x, y] => (MouseAction::Up(button::PRIMARY), x, y),
                ["down", x, y, b] => (MouseAction::Down(button_named(b)?), x, y),
                ["up", x, y, b] => (MouseAction::Up(button_named(b)?), x, y),
                _ => return Err(usage()),
            };
            Ok(Step::Mouse {
                action,
                x: position(x)?,
                y: position(y)?,
            })
        }
        "scroll" => {
            let [x, y, dx, dy] = args else {
                return Err("`scroll` takes a position, x and y, and notches across and down".to_owned());
            };
            Ok(Step::Scroll {
                x: position(x)?,
                y: position(y)?,
                dx: notches(dx)?,
                dy: notches(dy)?,
            })
        }
        "pan" => match args {
            ["end", x, y] => Ok(Step::PanEnd {
                x: position(x)?,
                y: position(y)?,
            }),
            [x, y, dx, dy] => Ok(Step::Pan {
                x: position(x)?,
                y: position(y)?,
                dx: notches(dx)?,
                dy: notches(dy)?,
            }),
            _ => Err(
                "`pan` takes a position, x and y, and notches across and down; or `end` and a position"
                    .to_owned(),
            ),
        },
        "zoom" => gesture(command, args, "a scale").map(Step::Zoom),
        "rotate" => gesture(command, args, "an angle in radians").map(Step::Rotate),
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

/// `begin`, `end`, or a gesture's value.
fn gesture(command: &str, args: &[&str], what: &str) -> Result<Gesture, String> {
    match args {
        ["begin"] => Ok(Gesture::Begin),
        ["end"] => Ok(Gesture::End),
        [value] => match value.parse::<f64>() {
            Ok(v) if v.is_finite() && v.abs() <= MAX_NOTCHES => Ok(Gesture::Update(v)),
            Ok(_) => Err(format!("`{}` is out of range", clip(value))),
            Err(_) => Err(format!("`{}` is not `begin`, `end` or {what}", clip(value))),
        },
        _ => Err(format!("`{command}` takes `begin`, `end` or {what}")),
    }
}

fn finger_number(word: &str) -> Result<u32, String> {
    match word.parse::<u32>() {
        Ok(n) if n < MAX_TOUCHES => Ok(n),
        Ok(_) => Err(format!(
            "finger {}; the limit is {}",
            clip(word),
            MAX_TOUCHES - 1
        )),
        Err(_) => Err(format!("`{}` is not a finger's number", clip(word))),
    }
}

fn button_named(word: &str) -> Result<u32, String> {
    match word {
        "primary" => Ok(button::PRIMARY),
        "secondary" => Ok(button::SECONDARY),
        "middle" => Ok(button::MIDDLE),
        "back" => Ok(button::BACK),
        "forward" => Ok(button::FORWARD),
        other => Err(format!("`{}` is not a mouse button", clip(other))),
    }
}

fn notches(word: &str) -> Result<f64, String> {
    match word.parse::<f64>() {
        Ok(v) if v.is_finite() && v.abs() <= MAX_NOTCHES => Ok(v),
        Ok(_) => Err(format!("`{}` is out of range", clip(word))),
        Err(_) => Err(format!("`{}` is not a number", clip(word))),
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
    use fl_input::{KeyEvent, PointerDeviceKind, PointerSignalKind};
    use std::sync::Mutex;

    #[test]
    fn steps_parse_a_line_each() {
        let s = Script::parse(
            "# taps\n\n  tap 756 556.5  # the button\nidle\ndown 1 2\nmove -3 4e1\nup 3 40\nwait 250\n\
             key keyA down\nkey keyA up\nkey enter\nmouse down 4 5\nmouse up 4 5 middle\n\
             mouse leave 0 5\ndown 1 2 3\nscroll 1 2 0 -1.5\npan 1 2 3 4\npan end 1 2\n\
             zoom begin\nzoom 1.5\nrotate -0.5\nrotate end\n",
        );
        assert_eq!(
            s.map(|s| s.steps),
            Ok(vec![
                Step::Tap {
                    x: 756.0,
                    y: 556.5,
                    finger: 0
                },
                Step::Idle,
                Step::Down {
                    x: 1.0,
                    y: 2.0,
                    finger: 0
                },
                Step::Move {
                    x: -3.0,
                    y: 40.0,
                    finger: 0
                },
                Step::Up {
                    x: 3.0,
                    y: 40.0,
                    finger: 0
                },
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
                    action: MouseAction::Down(button::PRIMARY),
                    x: 4.0,
                    y: 5.0
                },
                Step::Mouse {
                    action: MouseAction::Up(button::MIDDLE),
                    x: 4.0,
                    y: 5.0
                },
                Step::Mouse {
                    action: MouseAction::Leave,
                    x: 0.0,
                    y: 5.0
                },
                Step::Down {
                    x: 1.0,
                    y: 2.0,
                    finger: 3
                },
                Step::Scroll {
                    x: 1.0,
                    y: 2.0,
                    dx: 0.0,
                    dy: -1.5
                },
                Step::Pan {
                    x: 1.0,
                    y: 2.0,
                    dx: 3.0,
                    dy: 4.0
                },
                Step::PanEnd { x: 1.0, y: 2.0 },
                Step::Zoom(Gesture::Begin),
                Step::Zoom(Gesture::Update(1.5)),
                Step::Rotate(Gesture::Update(-0.5)),
                Step::Rotate(Gesture::End),
            ])
        );
        assert_eq!(Script::parse("").map(|s| s.steps.len()), Ok(0));
    }

    #[test]
    fn what_does_not_parse_names_its_line() {
        let cases = [
            ("tap 1", 1, "`tap` takes a position, x and y, and a finger's number or none"),
            ("idle\ntap 1 2 3 4", 2, "`tap` takes a position, x and y, and a finger's number or none"),
            ("tap 1 2 x", 1, "`x` is not a finger's number"),
            ("down 1 2 128", 1, "finger 128; the limit is 127"),
            ("up 1 2 -1", 1, "`-1` is not a finger's number"),
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
            ("mouse 1 2", 1, "`mouse` takes `move`, `down`, `up` or `leave`, then a position, x and y, and for `down` and `up` a button or none"),
            ("mouse click 1 2", 1, "`mouse` takes `move`, `down`, `up` or `leave`, then a position, x and y, and for `down` and `up` a button or none"),
            ("mouse move 1 2 primary", 1, "`mouse` takes `move`, `down`, `up` or `leave`, then a position, x and y, and for `down` and `up` a button or none"),
            ("mouse down 1 2 left", 1, "`left` is not a mouse button"),
            (
                "scroll 1 2 3",
                1,
                "`scroll` takes a position, x and y, and notches across and down",
            ),
            ("scroll 1 2 0 1e7", 1, "`1e7` is out of range"),
            ("scroll 1 2 0 up", 1, "`up` is not a number"),
            (
                "pan end",
                1,
                "`pan` takes a position, x and y, and notches across and down; or `end` and a position",
            ),
            ("pan 1 2 nan 0", 1, "`nan` is out of range"),
            ("zoom", 1, "`zoom` takes `begin`, `end` or a scale"),
            ("zoom in", 1, "`in` is not `begin`, `end` or a scale"),
            ("zoom inf", 1, "`inf` is out of range"),
            (
                "rotate 1 2",
                1,
                "`rotate` takes `begin`, `end` or an angle in radians",
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

        /// One pointer event: a touch as its phase and place, and its
        /// finger if not the first; a mouse with its buttons, and what it
        /// scrolled; a touchpad's gesture with its pan, scale and
        /// rotation. Then how long after the script began, if at all.
        fn describe(&self, e: &PointerEvent) -> String {
            assert_eq!(e.view_id, 0);
            let mut line = match e.device_kind {
                PointerDeviceKind::Touch => {
                    assert_eq!(e.device >> 28, 2);
                    let finger = match e.device & 0xff {
                        0 => String::new(),
                        n => format!("#{n}"),
                    };
                    format!("{:?}{finger}@{},{}", e.phase, e.x, e.y)
                }
                PointerDeviceKind::Trackpad => {
                    assert_eq!(e.device, 1);
                    format!(
                        "pad:{:?}@{},{} pan={},{} scale={} rotation={}",
                        e.phase, e.x, e.y, e.pan_x, e.pan_y, e.scale, e.rotation
                    )
                }
                kind => {
                    assert_eq!((kind, e.device), (PointerDeviceKind::Mouse, 0));
                    let scrolled = match e.signal_kind {
                        PointerSignalKind::Scroll => {
                            format!(" scroll={},{}", e.scroll_delta_x, e.scroll_delta_y)
                        }
                        _ => String::new(),
                    };
                    format!(
                        "mouse:{:?}@{},{} buttons={}{scrolled}",
                        e.phase, e.x, e.y, e.buttons
                    )
                }
            };
            let after = e.timestamp.saturating_sub(self.now());
            if !after.is_zero() {
                line.push_str(&format!(" +{}ms", after.as_millis()));
            }
            line
        }
    }

    impl EventSink for Recorder {
        fn now(&self) -> Duration {
            Duration::from_millis(7)
        }

        fn pointer_events(&self, events: &[PointerEvent]) {
            let line: Vec<String> = events.iter().map(|e| self.describe(e)).collect();
            if let Ok(mut log) = self.log.lock() {
                log.push(line.join(" "));
            }
        }

        fn key_event(&self, event: KeyEvent) {
            let line: Vec<String> = event
                .data
                .iter()
                .map(|d| {
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
        script.play(&sink, Some(&mut Plain), 1.0);
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
        script.play(&sink, None, 1.0);
        assert_eq!(
            sink.log(),
            ["idle", "Add@1,1", "Down@1,1", "Up@1,1", "Remove@1,1"]
        );
        assert!(!Script::parse("tap 1 1").unwrap_or_default().has_keys());
    }

    #[test]
    fn a_touch_is_added_as_it_goes_down_and_removed_as_it_lifts() {
        let script = Script::parse("tap 5 6\nidle\ndown 1 2\nmove 3 4\nup 3 4\nwait 1\n");
        let sink = Recorder::new(usize::MAX);
        script.unwrap_or_default().play(&sink, None, 1.0);
        assert_eq!(
            sink.log(),
            [
                // Before the first step, unasked.
                "idle",
                "Add@5,6",
                "Down@5,6",
                "Up@5,6",
                "Remove@5,6",
                "idle",
                "Add@1,2",
                "Down@1,2",
                "Move@3,4",
                "Up@3,4",
                "Remove@3,4",
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
        script.unwrap_or_default().play(&sink, None, 1.0);
        assert_eq!(
            sink.log(),
            [
                "idle",
                "mouse:Add@1,2 buttons=0",
                "mouse:Hover@1,2 buttons=0",
                "mouse:Down@1,2 buttons=1",
                "mouse:Move@3,4 buttons=1",
                "mouse:Up@3,4 buttons=0",
                "mouse:Hover@5,6 buttons=0",
                "Add@7,8",
                "Down@7,8",
                "Up@7,8",
                "Remove@7,8",
            ]
        );
        // A script that starts with the button: added there.
        let script = Script::parse("mouse down 9 9\n");
        let sink = Recorder::new(usize::MAX);
        script.unwrap_or_default().play(&sink, None, 1.0);
        assert_eq!(
            sink.log(),
            [
                "idle",
                "mouse:Add@9,9 buttons=0",
                "mouse:Down@9,9 buttons=1"
            ]
        );
    }

    /// Fingers are pointers of their own, and a tap is a finger too.
    #[test]
    fn fingers_are_told_apart() {
        let script =
            Script::parse("down 1 1\ndown 2 2 5\nmove 3 3 5\ntap 9 9 7\nup 1 1\nup 3 3 5\n");
        let sink = Recorder::new(usize::MAX);
        script.unwrap_or_default().play(&sink, None, 1.0);
        assert_eq!(
            sink.log(),
            [
                "idle",
                "Add@1,1",
                "Down@1,1",
                "Add#1@2,2",
                "Down#1@2,2",
                "Move#1@3,3",
                "Add#2@9,9",
                "Down#2@9,9",
                "Up#2@9,9",
                "Remove#2@9,9",
                "Up@1,1",
                "Remove@1,1",
                "Up#1@3,3",
                "Remove#1@3,3",
            ]
        );
    }

    /// The wheel and the touchpad as `fl_scrolling_manager` sends them: a
    /// notch is 53 logical pixels, so twice that at a scale of 2; a pan
    /// goes the other way and adds up; a pinch and a rotation are one
    /// gesture, where the mouse was.
    #[test]
    fn a_wheel_scrolls_and_a_touchpad_pans_zooms_and_rotates() {
        let script = Script::parse(
            "scroll 10 20 0 1\nscroll 10 20 -0.5 0\npan 4 5 0 -1\npan 4 5 1 -1\npan end 4 5\n\
             mouse move 30 40\nzoom begin\nrotate begin\nzoom 2\nrotate 0.5\nzoom end\nrotate end\n",
        );
        let sink = Recorder::new(usize::MAX);
        script.unwrap_or_default().play(&sink, None, 2.0);
        assert_eq!(
            sink.log(),
            [
                "idle",
                "mouse:Move@10,20 buttons=0 scroll=0,106",
                "mouse:Move@10,20 buttons=0 scroll=-53,0",
                "pad:PanZoomStart@4,5 pan=0,0 scale=0 rotation=0",
                "pad:PanZoomUpdate@4,5 pan=0,106 scale=1 rotation=0",
                "pad:PanZoomUpdate@4,5 pan=-106,212 scale=1 rotation=0",
                "pad:PanZoomEnd@4,5 pan=-106,212 scale=0 rotation=0",
                "mouse:Add@30,40 buttons=0",
                "mouse:Hover@30,40 buttons=0",
                "pad:PanZoomStart@30,40 pan=0,0 scale=0 rotation=0",
                "pad:PanZoomUpdate@30,40 pan=0,0 scale=2 rotation=0",
                "pad:PanZoomUpdate@30,40 pan=0,0 scale=2 rotation=0.5",
                "pad:PanZoomEnd@30,40 pan=0,0 scale=0 rotation=0",
            ]
        );
        // A scale that is no scale is 1.
        let script = Script::parse("scroll 1 1 0 1\n").unwrap_or_default();
        for scale in [0.0, -3.0, f64::NAN, f64::INFINITY] {
            let sink = Recorder::new(usize::MAX);
            script.play(&sink, None, scale);
            assert_eq!(sink.log()[1], "mouse:Move@1,1 buttons=0 scroll=0,53");
        }
    }

    /// Every button, and a mouse that leaves: gone with no button down,
    /// kept with one.
    #[test]
    fn a_mouse_has_buttons_and_leaves() {
        let script = Script::parse(
            "mouse down 1 1 secondary\nmouse down 1 1 back\nmouse up 1 1 secondary\n\
             mouse leave 0 1\nmouse up 0 1 back\nmouse leave 0 1\nmouse leave 0 1\nmouse move 2 2\n",
        );
        let sink = Recorder::new(usize::MAX);
        script.unwrap_or_default().play(&sink, None, 1.0);
        assert_eq!(
            sink.log(),
            [
                "idle",
                "mouse:Add@1,1 buttons=0",
                "mouse:Down@1,1 buttons=2",
                "mouse:Move@1,1 buttons=10",
                "mouse:Move@1,1 buttons=8",
                "mouse:Up@0,1 buttons=0",
                "mouse:Remove@0,1 buttons=0",
                "mouse:Add@2,2 buttons=0",
                "mouse:Hover@2,2 buttons=0",
            ]
        );
    }

    /// An event's time is the script's: its waits, added up, whatever
    /// the sleeps came to; so a gesture's speed is what the script says.
    #[test]
    fn events_are_timed_by_the_script_s_waits() {
        let script =
            Script::parse("down 0 0\nwait 8\nmove 0 10\nwait 8\nmove 0 20\nup 0 20\nkey keyA\n");
        let sink = Recorder::new(usize::MAX);
        script
            .unwrap_or_default()
            .play(&sink, Some(&mut Plain), 1.0);
        let log = sink.log();
        assert_eq!(
            log[..7],
            [
                "idle",
                "Add@0,0",
                "Down@0,0",
                "Move@0,10 +8ms",
                "Move@0,20 +16ms",
                "Up@0,20 +16ms",
                "Remove@0,20 +16ms",
            ]
        );
        assert_eq!(log.len(), 9);
    }

    #[test]
    fn a_loop_that_ends_ends_the_script() {
        let script = Script::parse("tap 1 1\nidle\ntap 2 2\nwait 600000\ntap 3 3\n");
        // The first idle is the script's own; the second never comes.
        let sink = Recorder::new(1);
        script.clone().unwrap_or_default().play(&sink, None, 1.0);
        assert_eq!(
            sink.log(),
            ["idle", "Add@1,1", "Down@1,1", "Up@1,1", "Remove@1,1"]
        );
        // Not even the first.
        let sink = Recorder::new(0);
        script.unwrap_or_default().play(&sink, None, 1.0);
        assert!(sink.log().is_empty());
    }
}
