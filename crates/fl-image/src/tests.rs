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
    assert!(decode(png, None, &Limits { max_pixels: 8 }).is_ok());
    assert!(matches!(
        decode(png, None, &Limits { max_pixels: 7 }),
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
    assert!(from_raw(&bytes, &raw, None, &Limits { max_pixels: 1 }).is_err());
    let huge = Raw {
        width: u32::MAX,
        height: u32::MAX,
        row_bytes: usize::MAX,
        bgra: false,
    };
    assert!(from_raw(&bytes, &huge, None, &Limits::default()).is_err());
}
