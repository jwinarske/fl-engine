//! Fuzzing the script (brief M4 §15): a script is untrusted input. The
//! apps' scripts and one of every step, mutated: bytes flipped, runs
//! overwritten, cut short, spliced, repeated. Whatever parses is played,
//! its waits taken out, into a sink that checks what it is given: every
//! position and distance finite and inside the script's bounds, every
//! device one the managers have. Deterministic, a seeded generator, and
//! pure Rust: no libFuzzer, so no C++ toolchain. A panic anywhere is a
//! bug; the sanitizer lanes run it under ASan.
//!
//!     FL_FUZZ_ITERS=20000 cargo test -p fl-input-script --test fuzz -- --ignored
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use fl_input::xkb::{Keymap, XkbKey};
use fl_input::{EventSink, KeyEvent, PointerDeviceKind, PointerEvent};
use fl_input_script::{Script, Step, MAX_NOTCHES, MAX_POSITION};

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

/// `seed` changed one to eight ways.
fn mutate(rng: &mut Rng, seed: &[u8], corpus: &[Vec<u8>]) -> Vec<u8> {
    let mut v = seed.to_vec();
    for _ in 0..1 + rng.below(8) {
        if v.is_empty() {
            v.push(rng.next() as u8);
        }
        let at = rng.below(v.len());
        match rng.below(7) {
            0 => v[at] ^= 1 << rng.below(8),
            1 => v[at] = rng.next() as u8,
            // A number's extremes, where a digit was.
            2 => {
                let word: &[u8] = [
                    &b"1e308"[..],
                    b"-1e308",
                    b"nan",
                    b"inf",
                    b"-0",
                    b"4294967296",
                    b"18446744073709551616",
                    b"0.0000001",
                ][rng.below(8)];
                v.splice(at..at, word.iter().copied());
            }
            3 => v.truncate(at),
            4 => {
                let other = &corpus[rng.below(corpus.len())];
                let from = rng.below(other.len());
                let n = rng.below(64).min(other.len() - from);
                v.splice(at..at, other[from..from + n].iter().copied());
            }
            5 => v.insert(at, b"\n #\t"[rng.below(4)]),
            _ => {
                let n = rng.below(32).min(v.len() - at);
                let run: Vec<u8> = v[at..at + n].to_vec();
                v.splice(at..at, run);
            }
        }
    }
    v
}

/// One of every step, and the apps' scripts.
fn corpus() -> Vec<Vec<u8>> {
    let apps = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps");
    let mut c = vec![
        b"tap 1 2\ndown 3 4 5\nmove 6 7 5\nup 6 7 5\nmouse move 1 2\nmouse down 1 2 middle\n\
          mouse up 1 2 middle\nmouse leave 0 0\nscroll 1 2 3 -4.5\npan 1 2 0.5 0.5\npan end 1 2\n\
          zoom begin\nzoom 1.5\nzoom end\nrotate begin\nrotate -0.25\nrotate end\nidle\nwait 5\n\
          key keyA down\nkey keyA up\nkey shiftLeft\n# a comment\n"
            .to_vec(),
    ];
    for f in [
        "counter/test/taps.txt",
        "keys/test/typing.txt",
        "keys/test/french.txt",
        "text_field/test/typed.txt",
        "text_field/test/selected.txt",
        "text_field/test/pasted.txt",
        "text_field/test/emoji.txt",
        "pointer/test/pointers.txt",
        "scroll/test/scroll.txt",
    ] {
        c.push(std::fs::read(format!("{apps}/{f}")).unwrap());
    }
    c
}

/// A sink that holds every event to what the script's bounds allow.
#[derive(Default)]
struct Checking {
    pointers: AtomicU64,
    keys: AtomicU64,
}

impl EventSink for Checking {
    fn now(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn pointer_events(&self, events: &[PointerEvent]) {
        // The pan a touchpad's steps add up to, each within its bound.
        let far = MAX_NOTCHES * 53.0 * 4.0 * 1e6;
        for e in events {
            assert!(
                e.x.abs() <= MAX_POSITION && e.y.abs() <= MAX_POSITION,
                "{e:?}"
            );
            for v in [
                e.scroll_delta_x,
                e.scroll_delta_y,
                e.pan_x,
                e.pan_y,
                e.scale,
                e.rotation,
            ] {
                assert!(v.is_finite() && v.abs() <= far, "{e:?}");
            }
            match e.device_kind {
                PointerDeviceKind::Touch => assert!((e.device & 0x0fff_ffff) < 128, "{e:?}"),
                PointerDeviceKind::Mouse => assert_eq!(e.device, 0, "{e:?}"),
                PointerDeviceKind::Trackpad => assert_eq!(e.device, 1, "{e:?}"),
                kind => panic!("a script has no {kind:?}"),
            }
            assert_eq!(e.view_id, 0);
            assert!(e.timestamp >= self.now());
        }
        self.pointers
            .fetch_add(events.len() as u64, Ordering::Relaxed);
    }

    fn key_event(&self, event: KeyEvent) {
        assert!(!event.data.is_empty());
        self.keys.fetch_add(1, Ordering::Relaxed);
    }

    fn wait_idle(&self) -> bool {
        true
    }

    fn is_closed(&self) -> bool {
        false
    }
}

/// A keymap of one level: a key's keysym is its keycode.
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
#[ignore = "fuzz: FL_FUZZ_ITERS=20000 cargo test -p fl-input-script --test fuzz -- --ignored"]
fn scripts_parse_or_say_why_and_play_within_bounds() {
    let iters: usize = std::env::var("FL_FUZZ_ITERS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(2000);
    let corpus = corpus();
    let mut rng = Rng(0x5eed_f1e1_0000_0004);
    let sink = Checking::default();
    let (mut parsed, mut refused) = (0usize, 0usize);
    for i in 0..iters {
        let input = mutate(&mut rng, &corpus[i % corpus.len()], &corpus);
        let text = String::from_utf8_lossy(&input);
        match Script::parse(&text) {
            Ok(mut script) => {
                parsed += 1;
                // Played without its sleeps, which add nothing but time.
                script.steps.retain(|s| !matches!(s, Step::Wait(_)));
                let scale = [1.0, 3.0, 0.0, f64::NAN][rng.below(4)];
                script.play(&sink, Some(&mut Plain), scale);
            }
            Err(e) => {
                refused += 1;
                // An error names a line of the text, and is short.
                assert!(e.line <= text.lines().count(), "{e}");
                assert!(e.message.len() < 200, "{e}");
            }
        }
    }
    // The corpus's own scripts, unchanged, all parse.
    for seed in &corpus {
        assert!(Script::parse(&String::from_utf8_lossy(seed)).is_ok());
    }
    println!(
        "{iters} scripts: {parsed} played, {refused} refused; {} pointer events, {} key events",
        sink.pointers.load(Ordering::Relaxed),
        sink.keys.load(Ordering::Relaxed)
    );
    // Both ways are exercised.
    assert!(
        parsed > iters / 20 && refused > iters / 20,
        "{parsed} {refused}"
    );
}
