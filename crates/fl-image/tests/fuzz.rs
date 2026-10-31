//! Fuzzing the decoders (brief M3 §15): every entry point the decode
//! thread and the UI thread call, given the fixtures and the gif program's
//! animations mutated: bytes flipped, runs overwritten with extremes, cut
//! short, spliced, repeated. Deterministic, a seeded generator, and pure
//! Rust: no libFuzzer, so no C++ toolchain. The sanitizer lanes run it
//! under ASan, where a decoder's memory error is caught; here, an error or
//! a decoder's own panic is an answer, and a panic in fl-image is a bug.
//!
//!     FL_FUZZ_ITERS=20000 cargo test -p fl-image --test fuzz -- --ignored
#![allow(clippy::unwrap_used, clippy::panic)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

use fl_image::{decode, from_raw, header, Frames, Limits, Raw};

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
        match rng.below(6) {
            0 => v[at] ^= 1 << rng.below(8),
            1 => v[at] = rng.next() as u8,
            // A size field's extremes: 0, 1, all ones.
            2 => {
                let b = [0x00, 0x01, 0xff][rng.below(3)];
                let n = 1 + rng.below(4);
                for x in v.iter_mut().skip(at).take(n) {
                    *x = b;
                }
            }
            3 => v.truncate(at),
            4 => {
                let other = &corpus[rng.below(corpus.len())];
                let from = rng.below(other.len());
                let n = rng.below(64).min(other.len() - from);
                v.splice(at..at, other[from..from + n].iter().copied());
            }
            _ => {
                let n = rng.below(32).min(v.len() - at);
                let run: Vec<u8> = v[at..at + n].to_vec();
                v.splice(at..at, run);
            }
        }
    }
    v
}

fn corpus() -> Vec<Vec<u8>> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata");
    let apps = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps");
    let mut c: Vec<Vec<u8>> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| std::fs::read(e.unwrap().path()).unwrap())
        .collect();
    for f in [
        "gif/assets/dispose.gif",
        "gif/assets/once.gif",
        "gif/assets/anim.webp",
        "images/assets/quads.gif",
        "images/assets/stripes.webp",
    ] {
        c.push(std::fs::read(format!("{apps}/{f}")).unwrap());
    }
    c
}

/// Where the last panic was, for telling fl-image's own from a decoder's.
static PANICKED_AT: Mutex<Option<String>> = Mutex::new(None);

/// Every entry point, on `bytes`, under a small pixel limit: a bomb is
/// refused cheaply and every frame is small.
fn run_all(bytes: &[u8]) {
    let limits = Limits {
        max_pixels: 1 << 16,
        max_frames: 64,
    };
    let _ = header(bytes);
    let _ = decode(bytes, None, &limits);
    let _ = decode(bytes, Some((7, 5)), &limits);
    if let Ok(mut f) = Frames::new(Arc::from(bytes), limits) {
        for _ in 0..f.frame_count().min(8) + 1 {
            let _ = f.next_frame();
        }
    }
    let raw = Raw {
        width: (bytes.len() % 9) as u32,
        height: (bytes.first().copied().unwrap_or(0) % 7).into(),
        row_bytes: bytes.len() % 41,
        bgra: bytes.len() % 2 == 0,
    };
    let _ = from_raw(bytes, &raw, None, &limits);
}

#[test]
#[ignore = "fuzz: FL_FUZZ_ITERS=20000 cargo test -p fl-image --test fuzz -- --ignored"]
fn decoders_answer_every_mutation() {
    let iters: usize = std::env::var("FL_FUZZ_ITERS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(2000);
    std::panic::set_hook(Box::new(|info| {
        *PANICKED_AT.lock().unwrap_or_else(|e| e.into_inner()) =
            info.location().map(|l| l.file().to_owned());
    }));
    let corpus = corpus();
    let mut rng = Rng(0x5eed_f1e1_0000_0001);
    let (mut decoder_panics, mut ours) = (0usize, Vec::new());
    for i in 0..iters {
        let seed = &corpus[i % corpus.len()];
        let input = mutate(&mut rng, seed, &corpus);
        if catch_unwind(AssertUnwindSafe(|| run_all(&input))).is_err() {
            let at = PANICKED_AT.lock().unwrap_or_else(|e| e.into_inner()).take();
            match at {
                // A path in this workspace, not a dependency's.
                Some(f) if f.contains("fl-image") || f.starts_with("crates/") => {
                    ours.push((i, f, input));
                }
                _ => decoder_panics += 1,
            }
        }
    }
    let _ = std::panic::take_hook();
    println!("{iters} inputs, {decoder_panics} decoder panics (the worker's to catch)");
    if let Some((i, f, input)) = ours.first() {
        let out = std::env::temp_dir().join(format!("fl-image-fuzz-{i}.bin"));
        let _ = std::fs::write(&out, input);
        panic!(
            "{} panics in fl-image, the first at input {i} in {f}, written to {}",
            ours.len(),
            out.display()
        );
    }
}
