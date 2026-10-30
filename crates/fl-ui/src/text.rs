//! `ParagraphBuilder`, `Paragraph` and `FontCollection`, on fl-text
//! (brief M3 §12, stage 4).
//!
//! The natives decode what dart:ui encodes, as paragraph_builder.cc does,
//! and answer in the shapes paragraph.cc answers. Text is laid out and
//! measured; drawing its glyphs is stage 5, so `paint` is still a stub.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use fl_dart_ffi::{args, guard, peer, sys};
use fl_text::font::FontCollection;
use fl_text::style::{ParagraphArgs, ParagraphStyle, PlaceholderStyle, TextStyle, TextStyleArgs};
use fl_text::{RectHeightStyle, RectWidthStyle, TextBox};

use crate::painting::{paint, Paint};
use crate::platform::{queue_reply, ReplyArg};

static FONTS: Mutex<Option<Arc<FontCollection>>> = Mutex::new(None);

static STYLE_REPORT: AtomicBool = AtomicBool::new(false);

/// Prints every paragraph and text style as decoded, for the `metrics`
/// app's check that each field of dart:ui's encoding is read where this
/// Flutter release writes it (brief M3 §15, format drift).
pub fn set_style_report(on: bool) {
    STYLE_REPORT.store(on, Ordering::Relaxed);
}

fn reporting() -> bool {
    STYLE_REPORT.load(Ordering::Relaxed)
}

/// The process's font collection: `loadFontFromList`'s fonts, the bundle's
/// `FontManifest.json` families, then the system's. Made on first use,
/// with the bundle set then.
pub(crate) fn fonts() -> Arc<FontCollection> {
    let mut f = FONTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = f.as_ref() {
        return Arc::clone(c);
    }
    let c = Arc::new(FontCollection::new());
    if let Some(bundle) = crate::buffer::asset_bundle() {
        match bundle.fonts() {
            Ok(families) => {
                let families = families
                    .into_iter()
                    .map(|f| (f.family, f.fonts.into_iter().map(|a| a.asset).collect()))
                    .collect();
                let b = Arc::clone(&bundle);
                c.set_assets(
                    families,
                    Arc::new(move |key: &str| match b.load(key) {
                        Ok(bytes) => Some(bytes),
                        Err(e) => {
                            eprintln!("fl-ui: font {key}: {e}");
                            None
                        }
                    }),
                );
            }
            Err(e) => eprintln!("fl-ui: {e}"),
        }
    }
    *f = Some(Arc::clone(&c));
    c
}

/// Forgets the collection, for a new bundle: the next paragraph makes it
/// again.
pub(crate) fn reset_fonts() {
    *FONTS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// The `ParagraphBuilder` peer: the builder and the paints its styles
/// name by index. `None` once built.
struct Builder {
    builder: Option<fl_text::ParagraphBuilder>,
    paints: Vec<Paint>,
}

/// The `Paragraph` peer.
struct Paragraph {
    paragraph: fl_text::Paragraph,
    /// The paints the styles name, drawn in stage 5.
    #[allow(dead_code)]
    paints: Vec<Paint>,
}

/// Attaches `value` to `wrapper`, saying so on failure.
///
/// # Safety
/// A Handle-parameter native: isolate current, scope open.
unsafe fn attach<T: Send + 'static>(wrapper: sys::Dart_Handle, value: T, size: usize, what: &str) {
    // SAFETY: per this function's contract.
    if let Err(e) = unsafe { peer::attach(wrapper, value, size) } {
        eprintln!("fl-ui: {what}: {e}");
    }
}

/// The elements of an `Int32List` (or any typed data, as 32-bit words).
///
/// # Safety
/// As [`args::bytes`].
unsafe fn i32s(h: sys::Dart_Handle) -> Vec<i32> {
    // SAFETY: per this function's contract.
    unsafe { args::bytes(h) }
        .unwrap_or_default()
        .chunks_exact(4)
        .map(|c| i32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// A `List` of strings; null entries, and a null list, are left out. A
/// family named by null is the default family, as tonic's "" is.
///
/// # Safety
/// As [`args::bytes`].
unsafe fn strings(h: sys::Dart_Handle) -> Vec<String> {
    // SAFETY: per this function's contract; indices stay below the length
    // the VM reported.
    unsafe {
        if sys::Dart_IsNull(h) || !sys::Dart_IsList(h) {
            return Vec::new();
        }
        let mut n: isize = 0;
        if sys::Dart_IsError(sys::Dart_ListLength(h, &mut n)) {
            return Vec::new();
        }
        (0..n)
            .map(|i| args::string(sys::Dart_ListGetAt(h, i)).unwrap_or_default())
            .collect()
    }
}

/// `Void Function(Handle this, Handle encoded, Handle strutData, Handle
/// fontFamily, Handle strutFontFamily, Double fontSize, Double height,
/// Handle ellipsis, Handle locale)`.
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn builder_create(
    wrapper: sys::Dart_Handle,
    encoded: sys::Dart_Handle,
    strut: sys::Dart_Handle,
    family: sys::Dart_Handle,
    strut_family: sys::Dart_Handle,
    size: f64,
    height: f64,
    ellipsis: sys::Dart_Handle,
    locale: sys::Dart_Handle,
) {
    // SAFETY: a Handle-parameter native; every handle is an argument.
    guard((), || unsafe {
        let a = ParagraphArgs {
            font_family: args::string(family).unwrap_or_default(),
            strut_font_families: strings(strut_family),
            font_size: size,
            height,
            ellipsis: args::string(ellipsis).unwrap_or_default(),
            locale: args::string(locale).unwrap_or_default(),
        };
        let strut = args::bytes(strut).unwrap_or_default();
        let style = ParagraphStyle::decode(&i32s(encoded), &strut, &a);
        if reporting() {
            println!("paragraph style: {style:?}");
        }
        let builder = Builder {
            builder: Some(fl_text::ParagraphBuilder::new(style, fonts())),
            paints: Vec::new(),
        };
        attach(wrapper, builder, 0, "ParagraphBuilder::Create");
    });
}

/// `Void Function(Pointer<Void>, Handle encoded, Handle fontFamilies,
/// Double fontSize, Double letterSpacing, Double wordSpacing, Double height,
/// Double decorationThickness, Handle locale, Handle backgroundObjects,
/// Handle backgroundData, Handle foregroundObjects, Handle foregroundData,
/// Handle shadowsData, Handle fontFeaturesData, Handle fontVariationsData)`.
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn push_style(
    builder: *mut c_void,
    encoded: sys::Dart_Handle,
    families: sys::Dart_Handle,
    size: f64,
    letter: f64,
    word: f64,
    height: f64,
    thickness: f64,
    locale: sys::Dart_Handle,
    bg_objects: sys::Dart_Handle,
    bg_data: sys::Dart_Handle,
    fg_objects: sys::Dart_Handle,
    fg_data: sys::Dart_Handle,
    shadows: sys::Dart_Handle,
    features: sys::Dart_Handle,
    variations: sys::Dart_Handle,
) {
    // SAFETY: a Handle-parameter native; builder is the receiver's field,
    // every handle an argument.
    guard((), || unsafe {
        let encoded = i32s(encoded);
        let families = strings(families);
        let locale = args::string(locale).unwrap_or_default();
        let has_bg = !sys::Dart_IsNull(bg_data);
        let has_fg = !sys::Dart_IsNull(fg_data);
        let bg = has_bg.then(|| paint(bg_objects, bg_data));
        let fg = has_fg.then(|| paint(fg_objects, fg_data));
        let shadows = args::bytes(shadows).unwrap_or_default();
        let features = args::bytes(features).unwrap_or_default();
        let variations = args::bytes(variations).unwrap_or_default();
        peer::with::<Builder, _>(builder, |b| {
            let mut id = |p: Option<Paint>| {
                p.map(|p| {
                    b.paints.push(p);
                    b.paints.len() - 1
                })
            };
            let a = TextStyleArgs {
                font_families: families,
                font_size: size,
                letter_spacing: letter,
                word_spacing: word,
                height,
                decoration_thickness: thickness,
                locale,
                background: id(bg),
                foreground: id(fg),
                shadows,
                font_features: features,
                font_variations: variations,
            };
            if let Some(pb) = b.builder.as_mut() {
                let style = TextStyle::decode(&pb.peek_style(), &encoded, &a);
                if reporting() {
                    println!("text style: {style:?}");
                    for (i, p) in b.paints.iter().enumerate() {
                        println!("text style paint {i}: {:?}", p.color);
                    }
                }
                pb.push_style(style);
            }
        });
    });
}

/// `Void Function(Pointer<Void>)`, a leaf call.
pub(crate) extern "C" fn builder_pop(builder: *mut c_void) {
    guard((), || {
        // SAFETY: builder is the receiver's native field.
        unsafe {
            peer::with::<Builder, _>(builder, |b| {
                if let Some(pb) = b.builder.as_mut() {
                    pb.pop();
                }
            })
        };
    });
}

/// `Handle Function(Pointer<Void>, Handle text)`: null, or the error
/// string dart:ui throws for text that is not UTF-16.
pub(crate) extern "C" fn add_text(
    builder: *mut c_void,
    text: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native; builder is the receiver's field.
    handle(|| unsafe {
        let Some(t) = args::string(text) else {
            return error("string is not well-formed UTF-16");
        };
        peer::with::<Builder, _>(builder, |b| {
            if let Some(pb) = b.builder.as_mut() {
                pb.add_text(&t);
            }
        });
        args::null()
    })
}

/// A Dart String, which dart:ui throws.
///
/// # Safety
/// The isolate is current with an API scope open.
unsafe fn error(msg: &str) -> sys::Dart_Handle {
    let msg = std::ffi::CString::new(msg).unwrap_or_default();
    // SAFETY: per this function's contract.
    unsafe { sys::Dart_NewStringFromCString(msg.as_ptr()) }
}

/// `Void Function(Pointer<Void>, Double width, Double height, Uint32
/// alignment, Double baselineOffset, Uint32 baseline)`.
pub(crate) extern "C" fn add_placeholder(
    builder: *mut c_void,
    width: f64,
    height: f64,
    alignment: u32,
    offset: f64,
    baseline: u32,
) {
    guard((), || {
        // SAFETY: builder is the receiver's native field.
        unsafe {
            peer::with::<Builder, _>(builder, |b| {
                if let Some(pb) = b.builder.as_mut() {
                    pb.add_placeholder(PlaceholderStyle::new(
                        width, height, alignment, offset, baseline,
                    ));
                }
            })
        };
    });
}

/// `Void Function(Pointer<Void>, Handle outParagraph)`.
pub(crate) extern "C" fn build(builder: *mut c_void, out: sys::Dart_Handle) {
    // SAFETY: a Handle-parameter native; builder is the receiver's field.
    guard((), || unsafe {
        let built = peer::with::<Builder, _>(builder, |b| {
            b.builder.take().map(|pb| Paragraph {
                paragraph: pb.build(),
                paints: std::mem::take(&mut b.paints),
            })
        })
        .flatten();
        match built {
            Some(p) => {
                // What layout holds at most a byte of text: flags, the
                // cluster index, both UTF maps and a cluster.
                let size = p.paragraph.text_len().saturating_mul(64);
                attach(out, p, size, "ParagraphBuilder::build");
            }
            None => eprintln!("fl-ui: ParagraphBuilder::build: no builder"),
        }
    });
}

/// A Handle-result native's body under `guard`, Dart null on a panic.
fn handle(f: impl FnOnce() -> sys::Dart_Handle) -> sys::Dart_Handle {
    // SAFETY: every caller is a Handle-result native: isolate current,
    // scope open.
    let null = unsafe { args::null() };
    guard(null, f)
}

/// Runs `f` on the paragraph, `or` for a disposed one.
fn with<R>(p: *mut c_void, or: R, f: impl FnOnce(&mut fl_text::Paragraph) -> R) -> R {
    // SAFETY: p is the receiver's native field, as every caller's is.
    unsafe { peer::with::<Paragraph, _>(p, |x| f(&mut x.paragraph)) }.unwrap_or(or)
}

macro_rules! getter {
    ($($name:ident -> $ty:ty = $e:expr),* $(,)?) => {
        $(pub(crate) extern "C" fn $name(p: *mut c_void) -> $ty {
            guard(<$ty>::default(), || with(p, <$ty>::default(), $e))
        })*
    };
}

getter!(
    width -> f64 = |x| f64::from(x.width()),
    height -> f64 = |x| f64::from(x.height()),
    longest_line -> f64 = |x| f64::from(x.longest_line()),
    min_intrinsic_width -> f64 = |x| f64::from(x.min_intrinsic_width()),
    max_intrinsic_width -> f64 = |x| f64::from(x.max_intrinsic_width()),
    alphabetic_baseline -> f64 = |x| f64::from(x.alphabetic_baseline()),
    ideographic_baseline -> f64 = |x| f64::from(x.ideographic_baseline()),
    did_exceed_max_lines -> bool = |x| x.did_exceed_max_lines(),
    number_of_lines -> u32 = |x| u32::try_from(x.line_count()).unwrap_or(u32::MAX),
);

/// `Void Function(Pointer<Void>, Double width)`, a leaf call.
pub(crate) extern "C" fn layout(p: *mut c_void, width: f64) {
    guard((), || with(p, (), |x| x.layout(width as f32)));
}

/// `Int32 Function(Pointer<Void>, Uint32 offset)`: the line, or -1.
pub(crate) extern "C" fn line_number_at(p: *mut c_void, offset: u32) -> i32 {
    guard(-1, || {
        with(p, -1, |x| {
            x.line_number_at(offset as usize)
                .and_then(|l| i32::try_from(l).ok())
                .unwrap_or(-1)
        })
    })
}

/// Boxes as paragraph.cc returns them: five floats each, left, top,
/// right, bottom and direction.
///
/// # Safety
/// The isolate is current with an API scope open.
unsafe fn boxes(b: &[TextBox]) -> sys::Dart_Handle {
    let v: Vec<f32> = b
        .iter()
        .flat_map(|b| {
            let [l, t, r, bo] = b.rect;
            [l, t, r, bo, b.direction as u8 as f32]
        })
        .collect();
    // SAFETY: per this function's contract.
    unsafe { args::new_f32s(&v) }
}

/// `Handle Function(Pointer<Void>, Uint32 start, Uint32 end, Uint32
/// heightStyle, Uint32 widthStyle)`.
pub(crate) extern "C" fn rects_for_range(
    p: *mut c_void,
    start: u32,
    end: u32,
    h: u32,
    w: u32,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    handle(|| unsafe {
        let ws = if w == 1 {
            RectWidthStyle::Max
        } else {
            RectWidthStyle::Tight
        };
        let b = with(p, Vec::new(), |x| {
            x.rects_for_range(
                start as usize,
                end as usize,
                RectHeightStyle::from_index(h),
                ws,
            )
        });
        boxes(&b)
    })
}

/// `Handle Function(Pointer<Void>)`.
pub(crate) extern "C" fn rects_for_placeholders(p: *mut c_void) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    handle(|| unsafe { boxes(&with(p, Vec::new(), |x| x.rects_for_placeholders())) })
}

/// `Handle Function(Pointer<Void>, Double dx, Double dy)`: the position
/// and the affinity, upstream 0 and downstream 1.
pub(crate) extern "C" fn position_for_offset(p: *mut c_void, dx: f64, dy: f64) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    handle(|| unsafe {
        let r = with(p, (0, 1), |x| {
            let r = x.position_at(dx as f32, dy as f32);
            (r.position as i64, i64::from(!r.upstream))
        });
        args::new_i64s(&[r.0, r.1])
    })
}

/// The glyph info constructor called as paragraph.cc calls it: left, top,
/// right, bottom, start, end, whether left to right. Null for none.
///
/// # Safety
/// The isolate is current with an API scope open; `ctor` is a live handle.
unsafe fn glyph_info(ctor: sys::Dart_Handle, g: Option<fl_text::GlyphInfo>) -> sys::Dart_Handle {
    // SAFETY: per this function's contract.
    unsafe {
        let Some(g) = g else {
            return args::null();
        };
        let [l, t, r, b] = g.rect;
        let mut a = [
            sys::Dart_NewDouble(f64::from(l)),
            sys::Dart_NewDouble(f64::from(t)),
            sys::Dart_NewDouble(f64::from(r)),
            sys::Dart_NewDouble(f64::from(b)),
            sys::Dart_NewInteger(g.start as i64),
            sys::Dart_NewInteger(g.end as i64),
            sys::Dart_NewBoolean(g.ltr),
        ];
        sys::Dart_InvokeClosure(ctor, a.len() as i32, a.as_mut_ptr())
    }
}

/// `Handle Function(Pointer<Void>, Uint32 offset, Handle constructor)`.
pub(crate) extern "C" fn glyph_info_at(
    p: *mut c_void,
    offset: u32,
    ctor: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native; ctor is an argument.
    handle(|| unsafe { glyph_info(ctor, with(p, None, |x| x.glyph_info_at(offset as usize))) })
}

/// `Handle Function(Pointer<Void>, Double dx, Double dy, Handle
/// constructor)`.
pub(crate) extern "C" fn closest_glyph_info(
    p: *mut c_void,
    dx: f64,
    dy: f64,
    ctor: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native; ctor is an argument.
    handle(|| unsafe {
        glyph_info(
            ctor,
            with(p, None, |x| x.closest_glyph_info(dx as f32, dy as f32)),
        )
    })
}

/// `Handle Function(Pointer<Void>, Uint32 offset)`: `[start, end]`.
pub(crate) extern "C" fn word_boundary(p: *mut c_void, offset: u32) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    handle(|| unsafe {
        let (s, e) = with(p, (0, 0), |x| x.word_boundary(offset as usize));
        args::new_i64s(&[s as i64, e as i64])
    })
}

/// `Handle Function(Pointer<Void>, Uint32 offset)`: `[start, end]`, or
/// `[-1, -1]` on no line.
pub(crate) extern "C" fn line_boundary(p: *mut c_void, offset: u32) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    handle(|| unsafe {
        let (s, e) = with(p, (-1, -1), |x| x.line_boundary(offset as usize));
        args::new_i64s(&[s, e])
    })
}

/// A line's nine numbers as paragraph.cc gives them: the hard break,
/// ascent, descent, unscaled ascent, the height as ascent plus descent
/// rounded, width, left, baseline and line number.
fn line_numbers(m: &fl_text::LineMetricsInfo) -> [f64; 9] {
    [
        f64::from(u8::from(m.hard_break)),
        m.ascent,
        m.descent,
        m.unscaled_ascent,
        (m.ascent + m.descent).round(),
        m.width,
        m.left,
        m.baseline,
        m.line_number as f64,
    ]
}

/// `Handle Function(Pointer<Void>)`: nine numbers a line.
pub(crate) extern "C" fn compute_line_metrics(p: *mut c_void) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native.
    handle(|| unsafe {
        let v: Vec<f64> = with(p, Vec::new(), |x| x.line_metrics())
            .iter()
            .flat_map(line_numbers)
            .collect();
        args::new_f64s(&v)
    })
}

/// `Handle Function(Pointer<Void>, Uint32 line, Handle constructor)`: the
/// constructor called with the nine, or null for no such line.
pub(crate) extern "C" fn line_metrics_at(
    p: *mut c_void,
    line: u32,
    ctor: sys::Dart_Handle,
) -> sys::Dart_Handle {
    // SAFETY: a Handle-result native; ctor is an argument.
    handle(|| unsafe {
        let Some(m) = with(p, None, |x| x.line_metrics_at(line as usize)) else {
            return args::null();
        };
        let n = line_numbers(&m);
        let mut a = [
            sys::Dart_NewBoolean(m.hard_break),
            sys::Dart_NewDouble(n[1]),
            sys::Dart_NewDouble(n[2]),
            sys::Dart_NewDouble(n[3]),
            sys::Dart_NewDouble(n[4]),
            sys::Dart_NewDouble(n[5]),
            sys::Dart_NewDouble(n[6]),
            sys::Dart_NewDouble(n[7]),
            sys::Dart_NewInteger(m.line_number as i64),
        ];
        sys::Dart_InvokeClosure(ctor, a.len() as i32, a.as_mut_ptr())
    })
}

/// `Void Function(Pointer<Void>, Pointer<Void> canvas, Double x, Double y)`:
/// nothing drawn yet; glyphs are stage 5.
pub(crate) extern "C" fn paint_paragraph(_p: *mut c_void, _canvas: *mut c_void, _x: f64, _y: f64) {}

/// `Void Function(Pointer<Void>)`.
pub(crate) extern "C" fn paragraph_dispose(p: *mut c_void) {
    guard((), || {
        // SAFETY: p is the receiver's native field.
        let _ = unsafe { peer::take::<Paragraph>(p) };
    });
}

/// `Void Function(Handle Uint8List list, Handle callback, Handle family)`:
/// the font registered as `family` (its own name if empty), then the
/// callback answered with 0, as font_collection.cc answers it whether or
/// not the data was a font.
pub(crate) extern "C" fn load_font_from_list(
    list: sys::Dart_Handle,
    callback: sys::Dart_Handle,
    family: sys::Dart_Handle,
) {
    // SAFETY: a Handle-parameter native; every handle is an argument.
    guard((), || unsafe {
        let family = args::string(family).unwrap_or_default();
        if let Some(data) = args::bytes(list) {
            if !fonts().add_font(data, &family) {
                eprintln!("fl-ui: loadFontFromList: not a font");
            }
        }
        queue_reply(callback, ReplyArg::Int(0));
    });
}
