#![allow(clippy::unwrap_used)]

use super::*;

/// The fixtures' pixels, straight alpha: `rgba.png` and `rgba.webp`,
/// 4x2.
const RGBA: [[u8; 4]; 8] = [
    [255, 0, 0, 255],
    [0, 255, 0, 255],
    [0, 0, 255, 255],
    [255, 255, 255, 255],
    [255, 0, 0, 128],
    [0, 0, 0, 0],
    [10, 20, 30, 255],
    [200, 100, 50, 64],
];

fn premultiplied(px: &[[u8; 4]]) -> Vec<u8> {
    let mut v: Vec<u8> = px.iter().flatten().copied().collect();
    premultiply(&mut v);
    v
}

fn pixel(f: &Frame, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * f.width + x) * 4) as usize;
    f.pixels[i..i + 4].try_into().unwrap()
}

#[test]
fn formats_are_told_by_their_first_bytes() {
    assert_eq!(
        format_of(include_bytes!("../testdata/rgba.png")),
        Some(Format::Png)
    );
    assert_eq!(
        format_of(include_bytes!("../testdata/halves.jpg")),
        Some(Format::Jpeg)
    );
    assert_eq!(
        format_of(include_bytes!("../testdata/still.gif")),
        Some(Format::Gif)
    );
    assert_eq!(
        format_of(include_bytes!("../testdata/rgba.webp")),
        Some(Format::Webp)
    );
    assert_eq!(format_of(b"BM......"), None);
    assert_eq!(header(b""), Err(ImageError::Unknown));
}

#[test]
fn headers_are_read_without_decoding() {
    let h = header(include_bytes!("../testdata/rgba.png")).unwrap();
    assert_eq!((h.format, h.width, h.height), (Format::Png, 4, 2));
    let h = header(include_bytes!("../testdata/rgba.webp")).unwrap();
    assert_eq!((h.width, h.height), (4, 2));
    let h = header(include_bytes!("../testdata/still.gif")).unwrap();
    assert_eq!((h.width, h.height), (2, 2));
    let h = header(include_bytes!("../testdata/halves.jpg")).unwrap();
    assert_eq!((h.width, h.height), (16, 8));
    // As shown: a quarter turn swaps the axes.
    let h = header(include_bytes!("../testdata/halves-orient6.jpg")).unwrap();
    assert_eq!((h.width, h.height), (8, 16));
}

#[test]
fn png_and_webp_decode_exactly_premultiplied() {
    for bytes in [
        &include_bytes!("../testdata/rgba.png")[..],
        &include_bytes!("../testdata/rgba.webp")[..],
    ] {
        let f = decode(bytes, None, &Limits::default()).unwrap();
        assert_eq!((f.width, f.height), (4, 2));
        assert_eq!(f.pixels, premultiplied(&RGBA));
    }
    // Premultiplied as Skia rounds: 255 at alpha 128 is 128; 200 at 64 is 50.
    let f = decode(
        include_bytes!("../testdata/rgba.png"),
        None,
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(pixel(&f, 0, 1), [128, 0, 0, 128]);
    assert_eq!(pixel(&f, 3, 1), [50, 25, 13, 64]);
}

#[test]
fn gray_and_palette_pngs_expand_to_rgba() {
    let f = decode(
        include_bytes!("../testdata/gray.png"),
        None,
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(f.pixels, [0, 0, 0, 255, 200, 200, 200, 255]);
    // Palette entry 1 is transparent (tRNS).
    let f = decode(
        include_bytes!("../testdata/palette.png"),
        None,
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(f.pixels, [255, 0, 0, 255, 0, 0, 0, 0]);
}

#[test]
fn a_gif_is_its_first_frame_with_its_delay() {
    let f = decode(
        include_bytes!("../testdata/still.gif"),
        None,
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(
        f.pixels,
        [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 0, 0]
    );
    assert_eq!(f.duration_ms, 50);
}

/// A frame smaller than the screen, offset into it: the rest transparent.
#[test]
fn a_gif_frame_sits_at_its_offset_on_the_screen() {
    let mut bytes = Vec::new();
    {
        let mut e = gif::Encoder::new(&mut bytes, 3, 2, &[255, 0, 0, 0, 0, 255]).unwrap();
        let mut f = gif::Frame::from_indexed_pixels(1, 1, vec![1], None);
        (f.left, f.top) = (2, 1);
        e.write_frame(&f).unwrap();
    }
    let f = decode(&bytes, None, &Limits::default()).unwrap();
    assert_eq!((f.width, f.height), (3, 2));
    assert_eq!(pixel(&f, 2, 1), [0, 0, 255, 255]);
    assert!(f.pixels[..20].iter().all(|b| *b == 0));
}

#[test]
fn jpegs_decode_and_turn_as_exif_says() {
    let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 8);
    let f = decode(
        include_bytes!("../testdata/halves.jpg"),
        None,
        &Limits::default(),
    )
    .unwrap();
    assert_eq!((f.width, f.height), (16, 8));
    assert!(
        near(pixel(&f, 1, 4), [255, 0, 0, 255]),
        "{:?}",
        pixel(&f, 1, 4)
    );
    assert!(
        near(pixel(&f, 14, 4), [0, 0, 255, 255]),
        "{:?}",
        pixel(&f, 14, 4)
    );
    // Orientation 6, a quarter turn clockwise: the left half is on top.
    let f = decode(
        include_bytes!("../testdata/halves-orient6.jpg"),
        None,
        &Limits::default(),
    )
    .unwrap();
    assert_eq!((f.width, f.height), (8, 16));
    assert!(
        near(pixel(&f, 4, 1), [255, 0, 0, 255]),
        "{:?}",
        pixel(&f, 4, 1)
    );
    assert!(
        near(pixel(&f, 4, 14), [0, 0, 255, 255]),
        "{:?}",
        pixel(&f, 4, 14)
    );
}

#[test]
fn every_orientation_moves_the_corner_where_exif_says() {
    // 2x1: the stored left pixel marked. Where it is shown, per tag.
    let stored = vec![255, 255, 255, 255, 0, 0, 0, 255];
    let want = [
        (1, (2, 1), 0),
        (2, (2, 1), 1),
        (3, (2, 1), 1),
        (4, (2, 1), 0),
        (5, (1, 2), 0),
        (6, (1, 2), 0),
        (7, (1, 2), 1),
        (8, (1, 2), 1),
    ];
    for (o, size, at) in want {
        let (p, w, h) = orient(stored.clone(), 2, 1, Orientation(o)).unwrap();
        assert_eq!((w, h), size, "orientation {o}");
        assert_eq!(p[at * 4], 255, "orientation {o}");
    }
}

#[test]
fn exif_orientation_is_read_in_either_byte_order() {
    // One IFD entry: tag 0x0112, SHORT, count 1, value 6.
    let le = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0";
    let be = b"Exif\0\0MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06\0\0";
    assert_eq!(orientation(Some(le)), Orientation(6));
    assert_eq!(orientation(Some(be)), Orientation(6));
    assert_eq!(orientation(Some(b"Exif\0\0junk")), Orientation(1));
    assert_eq!(orientation(None), Orientation(1));
}

/// §15: a header over the limit is refused before a buffer exists. This
/// GIF is a few dozen bytes, one 1x1 frame, and claims a 65535x65535
/// screen, 4 Gpx.
#[test]
fn a_header_over_the_pixel_limit_is_refused() {
    let mut bomb = Vec::new();
    {
        let mut e = gif::Encoder::new(&mut bomb, 65535, 65535, &[0, 0, 0, 255, 255, 255]).unwrap();
        e.write_frame(&gif::Frame::from_indexed_pixels(1, 1, vec![0], None))
            .unwrap();
    }
    assert!(bomb.len() < 64);
    assert_eq!(
        decode(&bomb, None, &Limits::default()),
        Err(ImageError::TooLarge {
            width: 65535,
            height: 65535,
            limit: DEFAULT_MAX_PIXELS
        })
    );
    // At the limit is allowed; one pixel over is not.
    let png = include_bytes!("../testdata/rgba.png");
    assert!(decode(
        png,
        None,
        &Limits {
            max_pixels: 8,
            ..Limits::default()
        }
    )
    .is_ok());
    assert!(matches!(
        decode(
            png,
            None,
            &Limits {
                max_pixels: 7,
                ..Limits::default()
            }
        ),
        Err(ImageError::TooLarge { .. })
    ));
    // A target size over the limit too.
    assert!(matches!(
        decode(png, Some((100_000, 100_000)), &Limits::default()),
        Err(ImageError::TooLarge { .. })
    ));
    assert!(Limits::default().check(u32::MAX, u32::MAX).is_err());
}

#[test]
fn truncated_data_is_an_error_not_a_panic() {
    for bytes in [
        &include_bytes!("../testdata/rgba.png")[..],
        &include_bytes!("../testdata/halves.jpg")[..],
        &include_bytes!("../testdata/still.gif")[..],
        &include_bytes!("../testdata/rgba.webp")[..],
    ] {
        for n in [4, 12, bytes.len() / 2, bytes.len() - 1] {
            // Some decoders fill a short tail rather than fail; either is
            // fine, so long as it returns.
            let _ = decode(&bytes[..n], None, &Limits::default());
        }
    }
}

#[test]
fn resizing_averages_down_and_interpolates_up() {
    // 2x1 black and white, down to 1x1: the mean.
    let px = [0, 0, 0, 255, 255, 255, 255, 255];
    assert_eq!(resize(&px, 2, 1, 1, 1).unwrap(), [128, 128, 128, 255]);
    // Up to 4x1: the outer pixels hold, the inner blend.
    let up = resize(&px, 2, 1, 4, 1).unwrap();
    assert_eq!(&up[..4], [0, 0, 0, 255]);
    assert_eq!(&up[12..], [255, 255, 255, 255]);
    assert_eq!(up[4], 64);
    // Through decode, a target size is the frame's.
    let f = decode(
        include_bytes!("../testdata/rgba.png"),
        Some((2, 1)),
        &Limits::default(),
    )
    .unwrap();
    assert_eq!((f.width, f.height, f.pixels.len()), (2, 1, 8));
}

#[test]
fn the_worker_answers_every_job_and_survives_a_bad_one() {
    let (tx, rx) = std::sync::mpsc::channel();
    let w = Worker::spawn(move |d| {
        // Decoded on the worker, whose thread calls the sink.
        let on = std::thread::current().name().map(str::to_owned);
        let _ = tx.send((d, on));
    })
    .unwrap();
    let job = |id, bytes: &[u8]| Job {
        id,
        tag: 1,
        bytes: bytes.into(),
        source: Source::Encoded,
        target: None,
        limits: Limits::default(),
    };
    assert!(w.submit(job(1, b"not an image")));
    assert!(w.submit(job(2, include_bytes!("../testdata/rgba.png"))));
    let (a, on) = rx.recv().unwrap();
    assert_eq!(on.as_deref(), Some("fl-image"));
    let (b, _) = rx.recv().unwrap();
    assert_eq!((a.id, a.result), (1, Err(ImageError::Unknown)));
    assert_eq!(b.id, 2);
    assert_eq!(b.result.unwrap().width, 4);
}

#[test]
fn retired_jobs_are_dropped_unstarted() {
    let (tx, rx) = std::sync::mpsc::channel();
    let (gate_tx, gate_rx) = std::sync::mpsc::channel::<()>();
    let (busy_tx, busy_rx) = std::sync::mpsc::channel::<()>();
    let gate = std::sync::Mutex::new((gate_rx, busy_tx));
    let w = Worker::spawn(move |d| {
        // Hold the worker on its first result until the test retires.
        if d.id == 0 {
            let g = gate.lock().unwrap();
            let _ = g.1.send(());
            let _ = g.0.recv();
        }
        let _ = tx.send(d.id);
    })
    .unwrap();
    let job = |id, tag| Job {
        id,
        tag,
        bytes: include_bytes!("../testdata/gray.png")[..].into(),
        source: Source::Encoded,
        target: None,
        limits: Limits::default(),
    };
    w.submit(job(0, 1));
    w.submit(job(1, 1));
    w.submit(job(2, 2));
    busy_rx.recv().unwrap();
    w.retire(1);
    gate_tx.send(()).unwrap();
    drop(w);
    let ids: Vec<u64> = rx.iter().collect();
    assert_eq!(ids, [0, 2]);
}

#[test]
fn raw_pixels_keep_their_bytes_by_row_and_swap_bgra() {
    // 1x2, rows 8 bytes apart: the padding is skipped.
    let bytes = [1, 2, 3, 4, 9, 9, 9, 9, 5, 6, 7, 8];
    let raw = Raw {
        width: 1,
        height: 2,
        row_bytes: 8,
        bgra: false,
    };
    let f = from_raw(&bytes, &raw, None, &Limits::default()).unwrap();
    assert_eq!(f.pixels, [1, 2, 3, 4, 5, 6, 7, 8]);
    let f = from_raw(&bytes, &Raw { bgra: true, ..raw }, None, &Limits::default()).unwrap();
    assert_eq!(f.pixels, [3, 2, 1, 4, 7, 6, 5, 8]);
    // Too few bytes, a short row, over the limit: refused.
    assert!(from_raw(&bytes[..11], &raw, None, &Limits::default()).is_err());
    assert!(from_raw(
        &bytes,
        &Raw {
            row_bytes: 3,
            ..raw
        },
        None,
        &Limits::default()
    )
    .is_err());
    assert!(from_raw(
        &bytes,
        &raw,
        None,
        &Limits {
            max_pixels: 1,
            ..Limits::default()
        }
    )
    .is_err());
    let huge = Raw {
        width: u32::MAX,
        height: u32::MAX,
        row_bytes: usize::MAX,
        bgra: false,
    };
    assert!(from_raw(&bytes, &huge, None, &Limits::default()).is_err());
}

/// Palette: 0 red, 1 green, 2 blue, 3 transparent.
const PALETTE: [u8; 12] = [255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0];

/// A GIF on a `w`x`h` screen: each frame its rect (left, top, width,
/// height), one palette index filling it, its disposal and delay.
fn animated_gif(
    (w, h): (u16, u16),
    repeat: Option<gif::Repeat>,
    frames: &[([u16; 4], u8, gif::DisposalMethod, u16)],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut e = gif::Encoder::new(&mut bytes, w, h, &PALETTE).unwrap();
        if let Some(r) = repeat {
            e.set_repeat(r).unwrap();
        }
        for &([l, t, fw, fh], index, dispose, delay) in frames {
            let n = usize::from(fw) * usize::from(fh);
            let mut f = gif::Frame::from_indexed_pixels(fw, fh, vec![index; n], Some(3));
            (f.left, f.top, f.dispose, f.delay) = (l, t, dispose, delay);
            e.write_frame(&f).unwrap();
        }
    }
    bytes
}

fn frames_of(bytes: Vec<u8>) -> Frames {
    Frames::new(Arc::from(bytes), Limits::default()).unwrap()
}

/// Every pixel of a frame, as palette colors: R, G, B, or . transparent.
fn colors(f: &Frame) -> String {
    f.pixels
        .chunks_exact(4)
        .map(|p| match p {
            [255, 0, 0, 255] => 'R',
            [0, 255, 0, 255] => 'G',
            [0, 0, 255, 255] => 'B',
            [0, 0, 0, 0] => '.',
            _ => '?',
        })
        .collect()
}

use gif::DisposalMethod::{Any, Background, Keep, Previous};
use std::sync::Arc;

#[test]
fn a_gif_animation_counts_its_frames_and_loops() {
    let one = [([0, 0, 1, 1], 0, Any, 1)];
    let two = [one[0], one[0]];
    for (repeat, want) in [
        (None, 0),
        (Some(gif::Repeat::Infinite), -1),
        (Some(gif::Repeat::Finite(2)), 2),
    ] {
        let f = frames_of(animated_gif((1, 1), repeat, &two));
        assert_eq!((f.frame_count(), f.repetition_count()), (2, want));
    }
    assert_eq!(frames_of(animated_gif((1, 1), None, &one)).frame_count(), 1);
    let still = frames_of(include_bytes!("../testdata/rgba.png").to_vec());
    assert_eq!((still.frame_count(), still.repetition_count()), (1, 0));
}

/// Each disposal method, on a 3x1 screen, as MultiFrameCodec composes
/// them; then the first frame again, on a clear canvas.
#[test]
fn gif_frames_are_composed_after_each_disposal() {
    let mut f = frames_of(animated_gif(
        (3, 1),
        None,
        &[
            ([0, 0, 1, 1], 0, Keep, 10),
            ([1, 0, 1, 1], 1, Background, 20),
            ([2, 0, 1, 1], 2, Previous, 30),
            ([0, 0, 3, 1], 3, Any, 40),
            ([1, 0, 1, 1], 2, Any, 5),
        ],
    ));
    let mut seen = Vec::new();
    for _ in 0..6 {
        let frame = f.next_frame().unwrap();
        seen.push((colors(&frame), frame.duration_ms));
    }
    let want = [
        ("R..", 100),
        // Keep: green on red.
        ("RG.", 200),
        // Background cleared green's square; blue drawn.
        ("R.B", 300),
        // Previous restored to before blue; the frame is all transparent.
        ("R..", 400),
        ("RB.", 50),
        // The first frame again, on a clear canvas.
        ("R..", 100),
    ];
    let want: Vec<(String, u32)> = want.iter().map(|(c, d)| ((*c).to_owned(), *d)).collect();
    assert_eq!(seen, want);
}

/// Restoring to the previous frame before any: transparent, as Skia's
/// required frame is none.
#[test]
fn a_first_frame_restoring_to_previous_restores_to_nothing() {
    let mut f = frames_of(animated_gif(
        (2, 1),
        None,
        &[([0, 0, 1, 1], 0, Previous, 1), ([1, 0, 1, 1], 1, Any, 1)],
    ));
    assert_eq!(colors(&f.next_frame().unwrap()), "R.");
    assert_eq!(colors(&f.next_frame().unwrap()), ".G");
}

/// A frame past the screen is clipped to it, not an error.
#[test]
fn a_gif_frame_past_the_screen_is_clipped() {
    let mut f = frames_of(animated_gif(
        (2, 1),
        None,
        &[([1, 0, 3, 2], 0, Any, 1), ([0, 0, 1, 1], 1, Any, 1)],
    ));
    assert_eq!(colors(&f.next_frame().unwrap()), ".R");
    assert_eq!(colors(&f.next_frame().unwrap()), "GR");
}

/// The frame limit, crossed: refused when counted, before any frame.
#[test]
fn an_animation_over_the_frame_limit_is_refused() {
    let bytes: Arc<[u8]> = Arc::from(animated_gif((1, 1), None, &[([0, 0, 1, 1], 0, Any, 1); 5]));
    let at = Limits {
        max_frames: 5,
        ..Limits::default()
    };
    assert_eq!(
        Frames::new(Arc::clone(&bytes), at).unwrap().frame_count(),
        5
    );
    let under = Limits {
        max_frames: 4,
        ..Limits::default()
    };
    assert!(matches!(
        Frames::new(bytes, under),
        Err(ImageError::TooManyFrames { limit: 4, .. })
    ));
}

/// A frame's own size, crossed: a 1x1 screen with a 3x3 frame, under a
/// 4-pixel limit, is that frame's error, and the next call starts over.
#[test]
fn a_frame_over_the_pixel_limit_is_refused() {
    let bytes: Arc<[u8]> = Arc::from(animated_gif(
        (1, 1),
        None,
        &[([0, 0, 1, 1], 0, Any, 1), ([0, 0, 3, 3], 1, Any, 1)],
    ));
    let limits = Limits {
        max_pixels: 4,
        ..Limits::default()
    };
    let mut f = Frames::new(bytes, limits).unwrap();
    assert_eq!(colors(&f.next_frame().unwrap()), "R");
    assert!(matches!(
        f.next_frame(),
        Err(ImageError::TooLarge { width: 3, .. })
    ));
    assert_eq!(colors(&f.next_frame().unwrap()), "R");
    // The first frame decoded alone holds its frame to the limit too.
    let big = animated_gif((1, 1), None, &[([0, 0, 3, 3], 1, Any, 1)]);
    assert!(matches!(
        decode(&big, None, &limits),
        Err(ImageError::TooLarge { .. })
    ));
}

/// 100,000 frames: counted, and decoded one at a time, each the size of
/// the screen (§15).
#[test]
fn a_hundred_thousand_tiny_frames_cost_one_frame() {
    let frames = vec![([0, 0, 1, 1], 1, Any, 1); 100_000];
    let mut f = frames_of(animated_gif((1, 1), Some(gif::Repeat::Infinite), &frames));
    assert_eq!(f.frame_count(), 100_000);
    for _ in 0..1000 {
        let frame = f.next_frame().unwrap();
        assert_eq!(frame.pixels.len(), 4);
    }
}

/// An animated WebP: frames composed as Skia composes them, the loop
/// count n played n times. testdata/anim.webp is
/// Pillow's: red, half-transparent green, blue; 100, 200, 300 ms; loop 3.
#[test]
fn an_animated_webp_gives_its_frames_and_loops() {
    let mut f = frames_of(include_bytes!("../testdata/anim.webp").to_vec());
    assert_eq!((f.frame_count(), f.repetition_count()), (3, 2));
    let mut seen = Vec::new();
    for _ in 0..4 {
        let frame = f.next_frame().unwrap();
        assert_eq!((frame.width, frame.height), (4, 4));
        seen.push((pixel(&frame, 1, 1), frame.duration_ms));
    }
    // Blue is blended over green, opaque: copied, as Skia does, not
    // rounded to 254 as image-webp's own blend would.
    assert_eq!(
        seen,
        [
            ([255, 0, 0, 255], 100),
            ([0, 128, 0, 128], 200),
            ([0, 0, 255, 255], 300),
            ([255, 0, 0, 255], 100),
        ]
    );
}

/// A truncated animation: counted as far as it parses; a frame cut short
/// is that frame's error, not a panic.
#[test]
fn a_truncated_animation_is_an_error_not_a_panic() {
    let bytes = animated_gif((2, 2), None, &[([0, 0, 2, 2], 0, Any, 1); 3]);
    for cut in 13..bytes.len() {
        if let Ok(mut f) = Frames::new(Arc::from(&bytes[..cut]), Limits::default()) {
            for _ in 0..4 {
                let _ = f.next_frame();
            }
        }
    }
}

/// The worker decodes an animation's frames in turn, the frames shared
/// with the caller.
#[test]
fn the_worker_gives_an_animations_frames_in_turn() {
    let frames = Arc::new(std::sync::Mutex::new(frames_of(animated_gif(
        (1, 1),
        None,
        &[([0, 0, 1, 1], 0, Any, 1), ([0, 0, 1, 1], 1, Any, 2)],
    ))));
    let (tx, rx) = std::sync::mpsc::channel();
    let w = Worker::spawn(move |d| tx.send(d).unwrap()).unwrap();
    for id in 0..3 {
        assert!(w.submit(Job {
            id,
            tag: 0,
            bytes: Arc::from(&[][..]),
            source: Source::Next(Arc::clone(&frames)),
            target: None,
            limits: Limits::default(),
        }));
    }
    let got: Vec<u32> = (0..3)
        .map(|_| rx.recv().unwrap().result.unwrap().duration_ms)
        .collect();
    assert_eq!(got, [10, 20, 10]);
}
