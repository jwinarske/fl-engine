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
//! idle              wait until the app has nothing to do: every event
//!                   delivered has been dispatched, every frame asked for
//!                   drawn (an animation run to its end)
//! wait 250          wait 250 ms
//! ```
//!
//! A script starts as if with `idle`, so its first event finds the app's
//! first frame laid out. A touch is delivered as the Linux embedder's
//! `fl_touch_manager` delivers one: added as it goes down, removed as it
//! lifts.
//!
//! A script is untrusted input (brief M4 §15): what does not parse is an
//! error naming its line, never a panic, and its size, its waits and its
//! positions are bounded.

use std::fmt;
use std::time::Duration;

use fl_input::{EventSink, PointerDeviceKind, PointerEvent, PointerPhase};

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
    Tap { x: f64, y: f64 },
    Down { x: f64, y: f64 },
    Move { x: f64, y: f64 },
    Up { x: f64, y: f64 },
    Idle,
    Wait(Duration),
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

    /// Plays the script into `sink`, on this thread: waits for the app to
    /// go idle, then each step in turn. Returns when the last has been
    /// delivered, or early if the loop ends first.
    pub fn play(&self, sink: &dyn EventSink) {
        if !sink.wait_idle() {
            return;
        }
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
    use std::sync::Mutex;

    #[test]
    fn steps_parse_a_line_each() {
        let s = Script::parse(
            "# taps\n\n  tap 756 556.5  # the button\nidle\ndown 1 2\nmove -3 4e1\nup 3 40\nwait 250\n",
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
                    assert_eq!(e.device_kind, PointerDeviceKind::Touch);
                    assert_eq!((e.device, e.view_id, e.timestamp), (0, 0, self.now()));
                    format!("{:?}@{},{}", e.phase, e.x, e.y)
                })
                .collect();
            if let Ok(mut log) = self.log.lock() {
                log.push(line.join(" "));
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

    #[test]
    fn a_touch_is_added_as_it_goes_down_and_removed_as_it_lifts() {
        let script = Script::parse("tap 5 6\nidle\ndown 1 2\nmove 3 4\nup 3 4\nwait 1\n");
        let sink = Recorder::new(usize::MAX);
        script.unwrap_or_default().play(&sink);
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

    #[test]
    fn a_loop_that_ends_ends_the_script() {
        let script = Script::parse("tap 1 1\nidle\ntap 2 2\nwait 600000\ntap 3 3\n");
        // The first idle is the script's own; the second never comes.
        let sink = Recorder::new(1);
        script.clone().unwrap_or_default().play(&sink);
        assert_eq!(sink.log(), ["idle", "Add@1,1 Down@1,1 Up@1,1 Remove@1,1"]);
        // Not even the first.
        let sink = Recorder::new(0);
        script.unwrap_or_default().play(&sink);
        assert!(sink.log().is_empty());
    }
}
