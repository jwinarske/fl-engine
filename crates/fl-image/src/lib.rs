//! Image decoding (brief M3 §4): an encoded image's header, its first frame
//! as premultiplied RGBA8, and the decode thread. No dart:ui, no emblema.
//!
//! Every decode goes through the rules of the M3 review (§15): the header's
//! size is checked against a pixel limit before any pixel buffer exists, and
//! every buffer this crate allocates is reserved with `try_reserve`, so a
//! decompression bomb is an error rather than the process aborting. The
//! decoders are safe Rust; a panic in one is caught by the [`Worker`] and is
//! that job's error.
//!
//! Formats: PNG, JPEG, GIF and WebP, one crate each (decision D3).
//! [`decode`] gives an image's first frame; [`Frames`], an animation's
//! frames one at a time.

use std::io::Cursor;

mod animation;
mod worker;

pub use animation::{Frames, DEFAULT_MAX_FRAMES};
pub use worker::{Done, Job, Source, Worker};

/// The default pixel limit: 64 Mpx, 256 MiB of RGBA8.
pub const DEFAULT_MAX_PIXELS: u64 = 64 << 20;

/// What a header read may allocate: a PNG's ancillary chunks before its
/// pixels (an ICC profile, text), read on the UI thread.
const HEADER_BYTES: usize = 16 << 20;

/// An encoded image's format, from its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    Webp,
}

/// Why an image was not decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// Not a format this crate reads.
    Unknown,
    /// The header claims more pixels than the limit allows.
    TooLarge { width: u64, height: u64, limit: u64 },
    /// An animation has more frames than the limit allows: counted up to
    /// one past it.
    TooManyFrames { frames: u32, limit: u32 },
    /// An allocation the decode needed failed.
    OutOfMemory,
    /// The decoder refused the data, or panicked on it.
    Decode(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::Unknown => write!(f, "not an image format fl-engine decodes"),
            ImageError::TooLarge {
                width,
                height,
                limit,
            } => write!(
                f,
                "a {width}x{height} image is over the {limit}-pixel limit"
            ),
            ImageError::TooManyFrames { limit, .. } => {
                write!(f, "an animation over the {limit}-frame limit")
            }
            ImageError::OutOfMemory => write!(f, "out of memory decoding the image"),
            ImageError::Decode(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ImageError {}

fn decode_err(what: &str, e: impl std::fmt::Display) -> ImageError {
    ImageError::Decode(format!("{what}: {e}"))
}

/// What may be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most pixels an image, a frame of one, or a size it is decoded
    /// to, may have.
    pub max_pixels: u64,
    /// The most frames an animation may have.
    pub max_frames: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_pixels: DEFAULT_MAX_PIXELS,
            max_frames: DEFAULT_MAX_FRAMES,
        }
    }
}

impl Limits {
    /// `width * height` within the limit, overflow included.
    pub fn check(&self, width: u32, height: u32) -> Result<(), ImageError> {
        let too_large = ImageError::TooLarge {
            width: width.into(),
            height: height.into(),
            limit: self.max_pixels,
        };
        match u64::from(width).checked_mul(u64::from(height)) {
            Some(n) if n <= self.max_pixels => Ok(()),
            _ => Err(too_large),
        }
    }

    /// The bytes an RGBA8 image at the limit takes: what a decoder's own
    /// memory limit is set to.
    fn bytes(&self) -> usize {
        usize::try_from(self.max_pixels.saturating_mul(4)).unwrap_or(usize::MAX)
    }
}

/// What an encoded image's header says, read without decoding pixels:
/// what `ImageDescriptor.encoded` needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub format: Format,
    pub width: u32,
    pub height: u32,
}

/// A decoded frame: `width * height` RGBA8 pixels, premultiplied, top row
/// first, as emblema samples them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// How long the frame shows, for an animation; 0 for a still.
    pub duration_ms: u32,
}

/// The format `bytes` start with, if one of the four.
pub fn format_of(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Format::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Format::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(Format::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(Format::Webp)
    } else {
        None
    }
}

/// The header of `bytes`. A JPEG's size is as displayed: its EXIF
/// orientation applied, as Skia's codec reports it.
pub fn header(bytes: &[u8]) -> Result<Header, ImageError> {
    let format = format_of(bytes).ok_or(ImageError::Unknown)?;
    let (width, height) = match format {
        Format::Png => {
            let d = png::Decoder::new_with_limits(
                Cursor::new(bytes),
                png::Limits {
                    bytes: HEADER_BYTES,
                },
            );
            let r = d.read_info().map_err(|e| decode_err("png", e))?;
            r.info().size()
        }
        Format::Jpeg => {
            let mut d = jpeg_decoder(bytes, &Limits::default());
            d.decode_headers().map_err(|e| decode_err("jpeg", e))?;
            let (w, h) = d
                .dimensions()
                .ok_or_else(|| decode_err("jpeg", "no size"))?;
            let (w, h) = (to_u32(w)?, to_u32(h)?);
            if orientation(d.exif().map(Vec::as_slice)).swaps_axes() {
                (h, w)
            } else {
                (w, h)
            }
        }
        Format::Gif => {
            let d = gif_options(&Limits::default())
                .read_info(Cursor::new(bytes))
                .map_err(|e| decode_err("gif", e))?;
            (d.width().into(), d.height().into())
        }
        Format::Webp => {
            let d = image_webp::WebPDecoder::new(Cursor::new(bytes))
                .map_err(|e| decode_err("webp", e))?;
            d.dimensions()
        }
    };
    if width == 0 || height == 0 {
        return Err(decode_err("image", "no pixels"));
    }
    Ok(Header {
        format,
        width,
        height,
    })
}

fn to_u32(n: usize) -> Result<u32, ImageError> {
    u32::try_from(n).map_err(|_| decode_err("image", "a size past u32"))
}

/// Decodes the first frame of `bytes`, resized to `target` if given and
/// different, premultiplied. The source size and the target are both held
/// to `limits` before anything is allocated.
pub fn decode(
    bytes: &[u8],
    target: Option<(u32, u32)>,
    limits: &Limits,
) -> Result<Frame, ImageError> {
    let h = header(bytes)?;
    limits.check(h.width, h.height)?;
    if let Some((w, h)) = target {
        limits.check(w, h)?;
    }
    let mut frame = match h.format {
        Format::Png => decode_png(bytes, limits)?,
        Format::Jpeg => decode_jpeg(bytes, limits)?,
        Format::Gif => decode_gif(bytes, limits)?,
        Format::Webp => decode_webp(bytes, limits)?,
    };
    premultiply(&mut frame.pixels);
    resized(frame, target)
}

/// Pixels as `decodeImageFromPixels` gives them: already premultiplied
/// (`PixelFormat`'s documentation), `row_bytes` apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Raw {
    pub width: u32,
    pub height: u32,
    /// Bytes from one row's start to the next; at least `width * 4`.
    pub row_bytes: usize,
    /// `PixelFormat.bgra8888`; otherwise `rgba8888`.
    pub bgra: bool,
}

/// `bytes` laid out as `raw` says, as a frame, resized to `target` if given.
/// Both sizes held to `limits`, and the rows to the bytes there are.
pub fn from_raw(
    bytes: &[u8],
    raw: &Raw,
    target: Option<(u32, u32)>,
    limits: &Limits,
) -> Result<Frame, ImageError> {
    limits.check(raw.width, raw.height)?;
    if let Some((w, h)) = target {
        limits.check(w, h)?;
    }
    let row = raw.width as usize * 4;
    if raw.width == 0 || raw.height == 0 || raw.row_bytes < row {
        return Err(decode_err("pixels", "a row shorter than its width"));
    }
    let needed = (raw.height as usize - 1)
        .checked_mul(raw.row_bytes)
        .and_then(|n| n.checked_add(row));
    if needed.is_none_or(|n| n > bytes.len()) {
        return Err(decode_err("pixels", "fewer bytes than the size given"));
    }
    let mut pixels = buffer(rgba_len(raw.width, raw.height)?)?;
    for (y, out) in pixels.chunks_exact_mut(row).enumerate() {
        let at = y * raw.row_bytes;
        out.copy_from_slice(&bytes[at..at + row]);
        if raw.bgra {
            out.chunks_exact_mut(4).for_each(|px| px.swap(0, 2));
        }
    }
    let frame = Frame {
        width: raw.width,
        height: raw.height,
        pixels,
        duration_ms: 0,
    };
    resized(frame, target)
}

/// `frame` at `target`, when that is given and differs.
fn resized(frame: Frame, target: Option<(u32, u32)>) -> Result<Frame, ImageError> {
    match target {
        Some((w, h)) if (w, h) != (frame.width, frame.height) && w > 0 && h > 0 => {
            let pixels = resize(&frame.pixels, frame.width, frame.height, w, h)?;
            Ok(Frame {
                width: w,
                height: h,
                pixels,
                ..frame
            })
        }
        _ => Ok(frame),
    }
}

/// A zeroed buffer of `len` bytes, or `OutOfMemory`.
fn buffer(len: usize) -> Result<Vec<u8>, ImageError> {
    let mut v = Vec::new();
    v.try_reserve_exact(len)
        .map_err(|_| ImageError::OutOfMemory)?;
    v.resize(len, 0);
    Ok(v)
}

/// `width * height * 4`, which [`Limits::check`] has already bounded.
fn rgba_len(width: u32, height: u32) -> Result<usize, ImageError> {
    usize::try_from(u64::from(width) * u64::from(height) * 4).map_err(|_| ImageError::OutOfMemory)
}

/// `src`, `channels` bytes a pixel, as RGBA8.
fn expand(
    src: &[u8],
    channels: usize,
    width: u32,
    height: u32,
    stride: usize,
) -> Result<Vec<u8>, ImageError> {
    let mut out = buffer(rgba_len(width, height)?)?;
    let w = width as usize;
    for (y, row) in out.chunks_exact_mut(w * 4).enumerate() {
        let line = src
            .get(y * stride..y * stride + w * channels)
            .ok_or_else(|| decode_err("image", "short pixel data"))?;
        for (o, i) in row.chunks_exact_mut(4).zip(line.chunks_exact(channels)) {
            let px = match *i {
                [g] => [g, g, g, 255],
                [g, a] => [g, g, g, a],
                [r, g, b] => [r, g, b, 255],
                [r, g, b, a] => [r, g, b, a],
                _ => return Err(decode_err("image", "a pixel layout not read")),
            };
            o.copy_from_slice(&px);
        }
    }
    Ok(out)
}

fn decode_png(bytes: &[u8], limits: &Limits) -> Result<Frame, ImageError> {
    let mut d = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: limits.bytes(),
        },
    );
    d.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = d.read_info().map_err(|e| decode_err("png", e))?;
    let len = r
        .output_buffer_size()
        .ok_or_else(|| decode_err("png", "too large"))?;
    let mut raw = buffer(len)?;
    let info = r.next_frame(&mut raw).map_err(|e| decode_err("png", e))?;
    let channels = info.color_type.samples();
    let rgba = rgba_len(info.width, info.height)?;
    // Tightly packed RGBA8 already: the decoder's buffer is the frame.
    let pixels = if channels == 4 && info.line_size == info.width as usize * 4 && raw.len() >= rgba
    {
        raw.truncate(rgba);
        raw
    } else {
        expand(&raw, channels, info.width, info.height, info.line_size)?
    };
    Ok(Frame {
        width: info.width,
        height: info.height,
        pixels,
        duration_ms: 0,
    })
}

fn jpeg_decoder<'a>(
    bytes: &'a [u8],
    limits: &Limits,
) -> zune_jpeg::JpegDecoder<zune_jpeg::zune_core::bytestream::ZCursor<&'a [u8]>> {
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    // A side over the limit is refused by the decoder too; the pixel count
    // is ours to check.
    let side = usize::try_from(limits.max_pixels.min(u64::from(u16::MAX))).unwrap_or(usize::MAX);
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_max_width(side)
        .set_max_height(side);
    zune_jpeg::JpegDecoder::new_with_options(
        zune_jpeg::zune_core::bytestream::ZCursor::new(bytes),
        options,
    )
}

fn decode_jpeg(bytes: &[u8], limits: &Limits) -> Result<Frame, ImageError> {
    let mut d = jpeg_decoder(bytes, limits);
    d.decode_headers().map_err(|e| decode_err("jpeg", e))?;
    let (w, h) = d
        .dimensions()
        .ok_or_else(|| decode_err("jpeg", "no size"))?;
    let (w, h) = (to_u32(w)?, to_u32(h)?);
    limits.check(w, h)?;
    let mut pixels = buffer(rgba_len(w, h)?)?;
    d.decode_into(&mut pixels)
        .map_err(|e| decode_err("jpeg", e))?;
    let turn = orientation(d.exif().map(Vec::as_slice));
    let (pixels, w, h) = orient(pixels, w, h, turn)?;
    Ok(Frame {
        width: w,
        height: h,
        pixels,
        duration_ms: 0,
    })
}

fn gif_options(limits: &Limits) -> gif::DecodeOptions {
    let mut o = gif::DecodeOptions::new();
    o.set_color_output(gif::ColorOutput::RGBA);
    if let Some(n) = std::num::NonZeroU64::new(limits.bytes() as u64) {
        o.set_memory_limit(gif::MemoryLimit::Bytes(n));
    }
    o
}

/// The first frame on the logical screen: transparent where the frame does
/// not cover it, as browsers and Skia draw it, the background color unused.
/// The frame's own size is held to the limit too.
fn decode_gif(bytes: &[u8], limits: &Limits) -> Result<Frame, ImageError> {
    let mut d = gif_options(limits)
        .read_info(Cursor::new(bytes))
        .map_err(|e| decode_err("gif", e))?;
    let (w, h) = (u32::from(d.width()), u32::from(d.height()));
    let mut pixels = buffer(rgba_len(w, h)?)?;
    let f = d
        .next_frame_info()
        .map_err(|e| decode_err("gif", e))?
        .ok_or_else(|| decode_err("gif", "no frames"))?;
    let rect = [
        usize::from(f.left),
        usize::from(f.top),
        usize::from(f.width),
        usize::from(f.height),
    ];
    let delay = f.delay;
    let (fw, fh) = (u32::from(f.width), u32::from(f.height));
    limits.check(fw, fh)?;
    let mut px = buffer(rgba_len(fw, fh)?)?;
    d.read_into_buffer(&mut px)
        .map_err(|e| decode_err("gif", e))?;
    animation::draw(&mut pixels, w as usize, h as usize, &px, rect);
    Ok(Frame {
        width: w,
        height: h,
        pixels,
        duration_ms: u32::from(delay) * 10,
    })
}

fn decode_webp(bytes: &[u8], limits: &Limits) -> Result<Frame, ImageError> {
    let mut d =
        image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| decode_err("webp", e))?;
    d.set_memory_limit(limits.bytes());
    let (w, h) = d.dimensions();
    let len = d
        .output_buffer_size()
        .ok_or_else(|| decode_err("webp", "too large"))?;
    let mut raw = buffer(len)?;
    d.read_image(&mut raw).map_err(|e| decode_err("webp", e))?;
    let pixels = if d.has_alpha() {
        raw
    } else {
        expand(&raw, 3, w, h, w as usize * 3)?
    };
    Ok(Frame {
        width: w,
        height: h,
        pixels,
        duration_ms: 0,
    })
}

/// Straight to premultiplied alpha, rounding as Skia's `SkMulDiv255Round`.
pub fn premultiply(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        let a = u32::from(px[3]);
        if a == 255 {
            continue;
        }
        for c in &mut px[..3] {
            let p = u32::from(*c) * a + 128;
            *c = ((p + (p >> 8)) >> 8) as u8;
        }
    }
}

/// `src` (premultiplied RGBA8, `sw` by `sh`) at `dw` by `dh`. Shrinking
/// averages each output pixel's footprint; enlarging interpolates
/// bilinearly between pixel centers. Skia's resize at decode is linear with
/// mipmaps, which agree with these at the sizes images are drawn at; not
/// bit-exact, and recorded as such.
pub fn resize(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Result<Vec<u8>, ImageError> {
    let mut out = buffer(rgba_len(dw, dh)?)?;
    if src.len() < rgba_len(sw, sh)? {
        return Err(decode_err("resize", "short pixel data"));
    }
    let (sw, sh, dw, dh) = (sw as usize, sh as usize, dw as usize, dh as usize);
    if sw == 0 || sh == 0 {
        return Ok(out);
    }
    let px = |x: usize, y: usize, c: usize| f32::from(src[(y * sw + x) * 4 + c]);
    for y in 0..dh {
        for x in 0..dw {
            let o = (y * dw + x) * 4;
            let mut acc = [0f32; 4];
            if dw <= sw && dh <= sh {
                // The source pixels whose centers fall in this footprint.
                let x0 = x * sw / dw;
                let x1 = ((x + 1) * sw).div_ceil(dw).min(sw).max(x0 + 1);
                let y0 = y * sh / dh;
                let y1 = ((y + 1) * sh).div_ceil(dh).min(sh).max(y0 + 1);
                let n = ((x1 - x0) * (y1 - y0)) as f32;
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        for (c, a) in acc.iter_mut().enumerate() {
                            *a += px(sx, sy, c);
                        }
                    }
                }
                acc.iter_mut().for_each(|a| *a /= n);
            } else {
                let fx =
                    ((x as f32 + 0.5) * sw as f32 / dw as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
                let fy =
                    ((y as f32 + 0.5) * sh as f32 / dh as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
                let (x0, y0) = (fx as usize, fy as usize);
                let (x1, y1) = ((x0 + 1).min(sw - 1), (y0 + 1).min(sh - 1));
                let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
                for (c, a) in acc.iter_mut().enumerate() {
                    let top = px(x0, y0, c) * (1.0 - tx) + px(x1, y0, c) * tx;
                    let bottom = px(x0, y1, c) * (1.0 - tx) + px(x1, y1, c) * tx;
                    *a = top * (1.0 - ty) + bottom * ty;
                }
            }
            for (c, a) in acc.iter().enumerate() {
                out[o + c] = a.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(out)
}

/// An EXIF orientation, 1 to 8: how the stored pixels are turned to be
/// shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Orientation(u16);

impl Orientation {
    /// 5 to 8 are a quarter turn, with or without a mirror.
    fn swaps_axes(self) -> bool {
        (5..=8).contains(&self.0)
    }
}

/// The orientation tag (0x0112) of a JPEG's EXIF block, the APP1 payload
/// after `Exif\0\0`, or 1 when there is none or it does not parse.
fn orientation(exif: Option<&[u8]>) -> Orientation {
    fn read(exif: &[u8]) -> Option<u16> {
        let tiff = exif.strip_prefix(b"Exif\0\0").unwrap_or(exif);
        let le = match tiff.get(..2)? {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };
        let u16_at = |i: usize| -> Option<u16> {
            let b: [u8; 2] = tiff.get(i..i + 2)?.try_into().ok()?;
            Some(if le {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            })
        };
        let u32_at = |i: usize| -> Option<u32> {
            let b: [u8; 4] = tiff.get(i..i + 4)?.try_into().ok()?;
            Some(if le {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            })
        };
        let ifd = usize::try_from(u32_at(4)?).ok()?;
        let entries = usize::from(u16_at(ifd)?);
        (0..entries).find_map(|i| {
            let e = ifd + 2 + i * 12;
            (u16_at(e)? == 0x0112).then(|| u16_at(e + 8)).flatten()
        })
    }
    Orientation(
        exif.and_then(read)
            .filter(|o| (1..=8).contains(o))
            .unwrap_or(1),
    )
}

/// `pixels` turned as `o` says, and the new size.
fn orient(
    pixels: Vec<u8>,
    w: u32,
    h: u32,
    o: Orientation,
) -> Result<(Vec<u8>, u32, u32), ImageError> {
    if o.0 == 1 {
        return Ok((pixels, w, h));
    }
    let (ow, oh) = if o.swaps_axes() { (h, w) } else { (w, h) };
    let mut out = buffer(pixels.len())?;
    let (w, h, ow) = (w as usize, h as usize, ow as usize);
    for y in 0..h {
        for x in 0..w {
            // Where the stored pixel (x, y) is shown.
            let (dx, dy) = match o.0 {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                8 => (y, w - 1 - x),
                _ => (x, y),
            };
            let s = (y * w + x) * 4;
            let d = (dy * ow + dx) * 4;
            out[d..d + 4].copy_from_slice(&pixels[s..s + 4]);
        }
    }
    Ok((out, ow as u32, oh))
}

#[cfg(test)]
mod tests;
