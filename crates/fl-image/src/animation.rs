//! Animated images (brief M3 §4, stage 7): a GIF's or a WebP's frames, one
//! at a time, as the engine's `MultiFrameCodec` gives them.
//!
//! [`Frames::new`] reads only what `frameCount` and `repetitionCount` need:
//! a GIF's frame headers with their pixel data skipped, a WebP's chunk list.
//! [`Frames::next_frame`] decodes one frame onto the image's canvas, after
//! the previous frame's disposal, and gives a copy, premultiplied: what is
//! held between calls is the canvas, and for a frame that restores to the
//! previous one, the canvas before it. A GIF of 100,000 tiny frames costs
//! one frame of memory (§15).
//!
//! Disposal as Skia's codecs (Wuffs for GIF, libwebp's demuxer for WebP)
//! and `MultiFrameCodec` apply it: restoring to the background clears to
//! transparent, the background color unused; restoring to the previous
//! frame before any frame restores to transparent. After the last frame
//! the first comes again, on a clear canvas.
//!
//! A WebP animation's frames are composed here, not by image-webp, which
//! blends an opaque pixel too and rounds 255 to 254 where libwebp and Skia
//! copy it: each frame's bitstream is given to image-webp as an image of
//! its own, its size held to the limit first.

use std::io::Cursor;
use std::sync::Arc;

use crate::{buffer, decode_err, expand, format_of, premultiply, rgba_len, Format, Frame};
use crate::{ImageError, Limits};

/// The default frame limit, past which an animation is refused.
pub const DEFAULT_MAX_FRAMES: u32 = 1 << 20;

type GifDecoder = gif::Decoder<Cursor<Arc<[u8]>>>;

/// An encoded image's frames, decoded one at a time.
pub struct Frames {
    bytes: Arc<[u8]>,
    limits: Limits,
    format: Format,
    count: u32,
    repetitions: i32,
    width: u32,
    height: u32,
    /// The frame [`Frames::next_frame`] decodes next.
    next: u32,
    gif: GifState,
    webp: WebpState,
}

impl std::fmt::Debug for Frames {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Frames({:?} {}x{}, {} frames, next {})",
            self.format, self.width, self.height, self.count, self.next
        )
    }
}

/// Where a WebP animation is, and what its frames are drawn on.
#[derive(Default)]
struct WebpState {
    /// The offset of the chunk after the last frame's.
    at: usize,
    /// The canvas, straight RGBA8, as the last frame left it.
    canvas: Vec<u8>,
    /// The last frame's rectangle, if it is disposed to the background.
    dispose: Option<[usize; 4]>,
}

/// A GIF's decoder, mid-stream, and what its frames are drawn on.
#[derive(Default)]
struct GifState {
    decoder: Option<GifDecoder>,
    /// The logical screen, straight RGBA8, as the last frame left it.
    canvas: Vec<u8>,
    /// The canvas before the last frame, if that frame restores to it.
    saved: Option<Vec<u8>>,
    /// The last frame's disposal and rectangle (left, top, width, height),
    /// applied before the next frame.
    dispose: Option<(gif::DisposalMethod, [usize; 4])>,
}

impl Frames {
    /// The frames of `bytes`, counted, none decoded. A still image is one
    /// frame; past `limits.max_frames` the image is refused.
    pub fn new(bytes: Arc<[u8]>, limits: Limits) -> Result<Frames, ImageError> {
        let format = format_of(&bytes).ok_or(ImageError::Unknown)?;
        let h = crate::header(&bytes)?;
        limits.check(h.width, h.height)?;
        let (count, repetitions) = match format {
            Format::Gif => gif_counts(&bytes, &limits)?,
            Format::Webp => webp_counts(&bytes)?,
            Format::Png | Format::Jpeg => (1, 0),
        };
        if count > limits.max_frames {
            return Err(ImageError::TooManyFrames {
                frames: count,
                limit: limits.max_frames,
            });
        }
        Ok(Frames {
            bytes,
            limits,
            format,
            count,
            repetitions,
            width: h.width,
            height: h.height,
            next: 0,
            gif: GifState::default(),
            webp: WebpState::default(),
        })
    }

    /// `frameCount`.
    pub fn frame_count(&self) -> u32 {
        self.count
    }

    /// `repetitionCount`: -1 for forever, else the times it plays after
    /// the first.
    pub fn repetition_count(&self) -> i32 {
        self.repetitions
    }

    /// Starts again from the first frame, on a clear canvas.
    pub fn restart(&mut self) {
        self.next = 0;
    }

    /// The next frame, on the whole canvas, premultiplied, with its
    /// duration; after the last, the first again.
    pub fn next_frame(&mut self) -> Result<Frame, ImageError> {
        if self.next >= self.count {
            self.next = 0;
        }
        if self.next == 0 {
            self.gif = GifState::default();
            self.webp = WebpState::default();
        }
        // A still is decoded whole, premultiplied; an animation's frames
        // come straight off the canvas.
        let r = match self.format {
            Format::Gif => self.next_gif(),
            Format::Webp if self.count > 1 => self.next_webp(),
            _ => {
                self.next = self.count;
                return crate::decode(&self.bytes, None, &self.limits);
            }
        };
        // A frame that failed leaves its decoder mid-frame: start over.
        let mut frame = r.inspect_err(|_| self.next = 0)?;
        self.next += 1;
        premultiply(&mut frame.pixels);
        Ok(frame)
    }

    fn next_gif(&mut self) -> Result<Frame, ImageError> {
        let (w, h) = (self.width, self.height);
        let s = &mut self.gif;
        if s.decoder.is_none() {
            let d = crate::gif_options(&self.limits)
                .read_info(Cursor::new(Arc::clone(&self.bytes)))
                .map_err(|e| decode_err("gif", e))?;
            s.canvas = buffer(rgba_len(w, h)?)?;
            s.decoder = Some(d);
        }
        match s.dispose.take() {
            Some((gif::DisposalMethod::Background, rect)) => {
                clear(&mut s.canvas, w as usize, h as usize, rect);
            }
            Some((gif::DisposalMethod::Previous, _)) => {
                if let Some(saved) = s.saved.take() {
                    s.canvas = saved;
                }
            }
            _ => {}
        }
        let d = s
            .decoder
            .as_mut()
            .ok_or_else(|| decode_err("gif", "no decoder"))?;
        let f = d
            .next_frame_info()
            .map_err(|e| decode_err("gif", e))?
            .ok_or_else(|| decode_err("gif", "fewer frames than counted"))?;
        let (fw, fh) = (u32::from(f.width), u32::from(f.height));
        let rect = [
            usize::from(f.left),
            usize::from(f.top),
            usize::from(f.width),
            usize::from(f.height),
        ];
        let (dispose, delay) = (f.dispose, f.delay);
        self.limits.check(fw, fh)?;
        let mut px = buffer(rgba_len(fw, fh)?)?;
        d.read_into_buffer(&mut px)
            .map_err(|e| decode_err("gif", e))?;
        if dispose == gif::DisposalMethod::Previous {
            let mut saved = buffer(s.canvas.len())?;
            saved.copy_from_slice(&s.canvas);
            s.saved = Some(saved);
        }
        draw(&mut s.canvas, w as usize, h as usize, &px, rect);
        s.dispose = Some((dispose, rect));
        let mut pixels = buffer(s.canvas.len())?;
        pixels.copy_from_slice(&s.canvas);
        Ok(Frame {
            width: w,
            height: h,
            pixels,
            duration_ms: u32::from(delay) * 10,
        })
    }

    fn next_webp(&mut self) -> Result<Frame, ImageError> {
        let (w, h) = (self.width, self.height);
        let s = &mut self.webp;
        if s.canvas.is_empty() {
            s.canvas = buffer(rgba_len(w, h)?)?;
            s.at = 12;
        }
        if let Some(rect) = s.dispose.take() {
            clear(&mut s.canvas, w as usize, h as usize, rect);
        }
        let (anmf, next) = loop {
            let (fourcc, body, next) = chunk(&self.bytes, s.at)
                .ok_or_else(|| decode_err("webp", "fewer frames than counted"))?;
            s.at = next;
            if fourcc == *b"ANMF" {
                break (body, next);
            }
        };
        s.at = next;
        let field = |i: usize| -> Result<usize, ImageError> {
            let b = anmf
                .get(i..i + 3)
                .ok_or_else(|| decode_err("webp", "a short frame header"))?;
            Ok(usize::from(b[0]) | usize::from(b[1]) << 8 | usize::from(b[2]) << 16)
        };
        let rect = [field(0)? * 2, field(3)? * 2, field(6)? + 1, field(9)? + 1];
        let duration_ms = field(12)? as u32;
        let flags = *anmf
            .get(15)
            .ok_or_else(|| decode_err("webp", "a short frame"))?;
        let (blend, dispose) = (flags & 2 == 0, flags & 1 != 0);
        let (fw, fh) = (to_u32(rect[2])?, to_u32(rect[3])?);
        self.limits.check(fw, fh)?;
        let px = webp_frame(&anmf[16..], fw, fh, &self.limits)?;
        compose(&mut s.canvas, w as usize, h as usize, &px, rect, blend);
        s.dispose = dispose.then_some(rect);
        let mut pixels = buffer(s.canvas.len())?;
        pixels.copy_from_slice(&s.canvas);
        Ok(Frame {
            width: w,
            height: h,
            pixels,
            duration_ms,
        })
    }
}

fn to_u32(n: usize) -> Result<u32, ImageError> {
    u32::try_from(n).map_err(|_| decode_err("webp", "a size past u32"))
}

/// The RIFF chunk at `at`: its fourcc, its body, and where the next
/// starts. `None` past the end or for a chunk that runs past it.
fn chunk(bytes: &[u8], at: usize) -> Option<([u8; 4], &[u8], usize)> {
    let head = bytes.get(at..at.checked_add(8)?)?;
    let fourcc = [head[0], head[1], head[2], head[3]];
    let len = usize::try_from(u32::from_le_bytes([head[4], head[5], head[6], head[7]])).ok()?;
    let start = at + 8;
    let end = start.checked_add(len)?;
    let body = bytes.get(start..end)?;
    Some((fourcc, body, end + (len & 1)))
}

/// An ANMF chunk's frame data, its ALPH and VP8 or VP8L chunks, as a WebP
/// of its own, decoded to straight RGBA8 at `fw`x`fh`.
fn webp_frame(data: &[u8], fw: u32, fh: u32, limits: &Limits) -> Result<Vec<u8>, ImageError> {
    let (mut alph, mut image) = (None, None);
    let mut at = 0;
    while let Some((fourcc, body, next)) = chunk(data, at) {
        match &fourcc {
            b"ALPH" => alph = Some(&data[at..next.min(data.len())]),
            b"VP8 " | b"VP8L" => {
                image = Some((fourcc, &data[at..next.min(data.len())], body.len()));
                break;
            }
            _ => {}
        }
        at = next;
    }
    let (fourcc, image, _) = image.ok_or_else(|| decode_err("webp", "a frame with no image"))?;
    // RIFF, then VP8X with the alpha flag when ALPH comes with VP8.
    let alph = alph.filter(|_| fourcc == *b"VP8 ");
    let vp8x_len = if alph.is_some() { 18 } else { 0 };
    let body = 4 + vp8x_len + alph.map_or(0, <[u8]>::len) + image.len();
    let mut file = Vec::new();
    file.try_reserve_exact(8 + body)
        .map_err(|_| ImageError::OutOfMemory)?;
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(
        &u32::try_from(body)
            .map_err(|_| ImageError::OutOfMemory)?
            .to_le_bytes(),
    );
    file.extend_from_slice(b"WEBP");
    if let Some(alph) = alph {
        file.extend_from_slice(b"VP8X");
        file.extend_from_slice(&10u32.to_le_bytes());
        file.extend_from_slice(&[0x10, 0, 0, 0]);
        file.extend_from_slice(&(fw - 1).to_le_bytes()[..3]);
        file.extend_from_slice(&(fh - 1).to_le_bytes()[..3]);
        file.extend_from_slice(alph);
    }
    file.extend_from_slice(image);
    let mut d =
        image_webp::WebPDecoder::new(Cursor::new(&file[..])).map_err(|e| decode_err("webp", e))?;
    d.set_memory_limit(limits.bytes());
    if d.dimensions() != (fw, fh) {
        return Err(decode_err("webp", "a frame not the size its header says"));
    }
    let len = d
        .output_buffer_size()
        .ok_or_else(|| decode_err("webp", "too large"))?;
    let mut raw = buffer(len)?;
    d.read_image(&mut raw).map_err(|e| decode_err("webp", e))?;
    if d.has_alpha() {
        Ok(raw)
    } else {
        expand(&raw, 3, fw, fh, fw as usize * 3)
    }
}

/// Draws a WebP frame at `rect`: blended, an opaque pixel copied, a
/// transparent one leaving the canvas, the rest source-over; else the
/// rectangle replaced.
fn compose(canvas: &mut [u8], sw: usize, sh: usize, px: &[u8], rect: [usize; 4], blend: bool) {
    let (rows, cols) = clipped(sw, sh, rect);
    let fw = rect[2];
    for y in rows {
        let fy = y - rect[1];
        let (Some(src), Some(dst)) = (
            px.get(fy * fw * 4..(fy * fw + cols) * 4),
            canvas.get_mut((y * sw + rect[0]) * 4..(y * sw + rect[0] + cols) * 4),
        ) else {
            continue;
        };
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            match s[3] {
                _ if !blend => d.copy_from_slice(s),
                255 => d.copy_from_slice(s),
                0 => {}
                sa => {
                    let (sa, da) = (f32::from(sa) / 255.0, f32::from(d[3]) / 255.0);
                    let a = sa + da * (1.0 - sa);
                    for c in 0..3 {
                        let v = (f32::from(s[c]) * sa + f32::from(d[c]) * da * (1.0 - sa)) / a;
                        d[c] = v.round().clamp(0.0, 255.0) as u8;
                    }
                    d[3] = (a * 255.0).round() as u8;
                }
            }
        }
    }
}

/// A GIF's frame count and `repetitionCount`, its pixel data skipped. The
/// count stops at the first frame header that does not parse, as Wuffs
/// counts the frames it can configure; none is an error.
fn gif_counts(bytes: &[u8], limits: &Limits) -> Result<(u32, i32), ImageError> {
    let mut o = crate::gif_options(limits);
    o.skip_frame_decoding(true);
    let mut d = o
        .read_info(Cursor::new(bytes))
        .map_err(|e| decode_err("gif", e))?;
    let mut count = 0u32;
    while let Ok(Some(_)) = d.next_frame_info() {
        count += 1;
        if count > limits.max_frames {
            break;
        }
    }
    if count == 0 {
        return Err(decode_err("gif", "no frames"));
    }
    // The NETSCAPE extension's loop count n plays n + 1 times; 0 is
    // forever; none, once (Wuffs' num_animation_loops, then Skia's).
    let repetitions = match d.repeat() {
        gif::Repeat::Infinite => -1,
        gif::Repeat::Finite(n) => i32::from(n),
    };
    Ok((count, repetitions))
}

/// A WebP's frame count and `repetitionCount`, from its chunks: a still is
/// one frame, played once. An animation's loop count n plays n times; 0
/// is forever (SkWebpCodec).
fn webp_counts(bytes: &[u8]) -> Result<(u32, i32), ImageError> {
    let d = image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| decode_err("webp", e))?;
    if !d.is_animated() {
        return Ok((1, 0));
    }
    let count = d.num_frames();
    if count == 0 {
        return Err(decode_err("webp", "no frames"));
    }
    let repetitions = match d.loop_count() {
        image_webp::LoopCount::Forever => -1,
        image_webp::LoopCount::Times(n) => i32::from(n.get()) - 1,
    };
    Ok((count, repetitions))
}

/// `rect` (left, top, width, height) of a `sw`-wide, `sh`-high RGBA8
/// canvas, clipped to it: the rows it covers, and the columns.
fn clipped(sw: usize, sh: usize, [l, t, w, h]: [usize; 4]) -> (std::ops::Range<usize>, usize) {
    let rows = t.min(sh)..(t + h).min(sh);
    let cols = w.min(sw.saturating_sub(l));
    (rows, cols)
}

/// Clears `rect` of the canvas to transparent.
fn clear(canvas: &mut [u8], sw: usize, sh: usize, rect: [usize; 4]) {
    let (rows, cols) = clipped(sw, sh, rect);
    for y in rows {
        let at = (y * sw + rect[0]) * 4;
        if let Some(row) = canvas.get_mut(at..at + cols * 4) {
            row.fill(0);
        }
    }
}

/// Draws a frame's straight RGBA8 pixels at `rect` on the canvas: its
/// transparent pixels leave the canvas as it is, as GIF's do.
pub(crate) fn draw(canvas: &mut [u8], sw: usize, sh: usize, px: &[u8], rect: [usize; 4]) {
    let (rows, cols) = clipped(sw, sh, rect);
    let fw = rect[2];
    for y in rows {
        let fy = y - rect[1];
        let (Some(src), Some(dst)) = (
            px.get(fy * fw * 4..(fy * fw + cols) * 4),
            canvas.get_mut((y * sw + rect[0]) * 4..(y * sw + rect[0] + cols) * 4),
        ) else {
            continue;
        };
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            if s[3] != 0 {
                d.copy_from_slice(s);
            }
        }
    }
}
