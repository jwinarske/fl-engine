//! The raster side (brief M2 §4, §5): a dart:ui [`Scene`] replayed into one
//! emblema `Canvas` and drawn.
//!
//! The layer tree is flattened as it is walked: a transform or offset layer
//! is `save` plus `concat`/`translate`, a clip layer `save` plus a clip, an
//! opacity layer a `save_layer`, a picture is replayed in place under its
//! offset, and `pop` is `restore`.
//!
//! What emblema cannot yet be asked to draw faithfully is an error, not an
//! approximation: a paint with a color space, shader or filter not mapped
//! here fails the frame and names what it was. Pixels that are quietly wrong
//! are the failure goldens exist to catch, and they would catch it late.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use emblema::{
    Affine2, BackendPreference, BlendMode, Canvas, Color, Context, Extent2D, FillRule, Layer,
    LineCap, LineJoin, MaskBlurStyle, Paint as EPaint, PathBuilder, PixelFormat, Rect,
    RoundingRadii, Sampling, SourceRect, Sprite, StrokeStyle, Style as EStyle, Surface, TileMode,
    Vec2,
};
use fl_ui::{
    Cap, Clip, ClipOp, Draw, FillType, ImageData, ImageShader, Join, LayerOp, Paint, Picture,
    RRect, Scene, Seg, Style,
};

mod glyphs;
mod images;

pub use glyphs::Glyphs;
pub use images::GpuImages;
use images::{GlyphPass, Slots};

/// Why a frame was not drawn.
#[derive(Debug)]
pub enum RasterError {
    Emblema(emblema::Error),
    /// Something the scene asks for that is not mapped to emblema yet.
    Unsupported(String),
    Png(String),
}

impl std::fmt::Display for RasterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RasterError::Emblema(e) => write!(f, "emblema: {e}"),
            RasterError::Unsupported(what) => write!(f, "not drawn yet: {what}"),
            RasterError::Png(e) => write!(f, "png: {e}"),
        }
    }
}

impl std::error::Error for RasterError {}

impl From<emblema::Error> for RasterError {
    fn from(e: emblema::Error) -> Self {
        RasterError::Emblema(e)
    }
}

/// An offscreen target of one size, on Vulkan (decision D4).
///
/// Not `Send`: it holds the device, and the device belongs to the raster
/// thread (brief M2 §5). [`RasterThread`] makes one there; only scenes
/// cross. emblema's own types would allow the move, so this is where the
/// placement is decided.
pub struct Offscreen {
    ctx: Context,
    surface: Option<Surface>,
    images: GpuImages,
    glyphs: Glyphs,
    width: u32,
    height: u32,
    _raster_thread: PhantomData<*const ()>,
}

impl Offscreen {
    /// `width` by `height` physical pixels, RGBA8.
    pub fn new(width: u32, height: u32) -> Result<Offscreen, RasterError> {
        let mut ctx = Context::new(BackendPreference::Vulkan)?;
        let surface = ctx.create_surface(Extent2D::new(width, height), PixelFormat::Rgba8Unorm)?;
        Ok(Offscreen {
            ctx,
            surface: Some(surface),
            images: GpuImages::default(),
            glyphs: Glyphs::default(),
            width,
            height,
            _raster_thread: PhantomData,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Draws `scene` over transparent black and reads the pixels back:
    /// `width * height` RGBA8 pixels, top row first, as the target holds them.
    ///
    /// Images nothing refers to any more are destroyed first; images the
    /// scene draws and [`Offscreen::upload`] has not seen are uploaded.
    pub fn render(&mut self, scene: &Scene) -> Result<Vec<u8>, RasterError> {
        self.images.sweep(&mut self.ctx);
        let mut found = Vec::new();
        for op in &scene.ops {
            if let LayerOp::AddPicture { picture, .. } = op {
                images_in(picture, &mut found);
            }
        }
        for image in &found {
            self.images.upload(&mut self.ctx, image)?;
        }
        // The first replay finds and rasterizes the frame's glyphs; the
        // atlas goes up once; the second draws.
        self.glyphs.begin_frame();
        {
            let mut canvas = Canvas::new(Extent2D::new(self.width, self.height));
            let mut slots = Slots::preparing(&self.images, &mut self.glyphs);
            replay(&mut canvas, scene, &mut slots)?;
        }
        self.glyphs.upload(&mut self.ctx)?;
        let mut canvas = Canvas::new(Extent2D::new(self.width, self.height));
        canvas.clear(Color::TRANSPARENT);
        let mut slots = Slots::drawing(&self.images, &self.glyphs);
        replay(&mut canvas, scene, &mut slots)?;
        let recording = canvas.finish();
        let surface = self
            .surface
            .as_mut()
            .ok_or_else(|| RasterError::Unsupported("a destroyed target".into()))?;
        self.ctx
            .draw_with_images(surface, &recording, &slots.table)?;
        Ok(self.ctx.read(surface)?)
    }

    /// Puts `image` on the device ahead of the frame that draws it.
    pub fn upload(&mut self, image: &Arc<ImageData>) -> Result<(), RasterError> {
        self.images.upload(&mut self.ctx, image)
    }

    /// The images on the device.
    pub fn images(&self) -> &GpuImages {
        &self.images
    }
}

impl Drop for Offscreen {
    /// emblema frees nothing on drop; images and the surface go before
    /// their context.
    fn drop(&mut self) {
        self.images.clear(&mut self.ctx);
        self.glyphs.clear(&mut self.ctx);
        if let Some(surface) = self.surface.take() {
            self.ctx.destroy_surface(surface);
        }
    }
}

/// Every image `picture` draws, its nested pictures' and its shaders'
/// included, into `out`.
fn images_in(picture: &Picture, out: &mut Vec<Arc<ImageData>>) {
    for d in &picture.draws {
        match d {
            Draw::Image { image, .. }
            | Draw::ImageNine { image, .. }
            | Draw::Atlas { image, .. } => out.push(Arc::clone(image)),
            Draw::Picture(p) => images_in(p, out),
            _ => {}
        }
        if let Some(shader) = paint_in(d).and_then(|p| p.shader.as_ref()) {
            out.push(Arc::clone(&shader.image));
        }
    }
}

/// The paint a draw carries, if it carries one.
fn paint_in(d: &Draw) -> Option<&Paint> {
    match d {
        Draw::SaveLayer { paint, .. }
        | Draw::Line { paint, .. }
        | Draw::Rect { paint, .. }
        | Draw::RRect { paint, .. }
        | Draw::DRRect { paint, .. }
        | Draw::Oval { paint, .. }
        | Draw::Circle { paint, .. }
        | Draw::Path { paint, .. }
        | Draw::Image { paint, .. }
        | Draw::ImageNine { paint, .. }
        | Draw::Atlas { paint, .. } => Some(paint),
        Draw::Paint(paint) => Some(paint),
        _ => None,
    }
}

/// The raster thread: an [`Offscreen`] made and used on a thread of its
/// own, drawing the scenes the UI thread submits, in order, and uploading
/// images as they are decoded ([`RasterThread::uploader`]).
///
/// One scene may wait beyond the one being drawn, Flutter's pipeline depth
/// of two: a UI thread that gets further ahead waits in
/// [`RasterThread::submit`] rather than piling up frames. Uploads do not
/// wait.
pub struct RasterThread {
    tx: Option<Sender<Work>>,
    scenes: Arc<InFlight>,
    thread: Option<JoinHandle<Rastered>>,
}

enum Work {
    Scene(Arc<Scene>),
    Upload(Arc<ImageData>),
}

/// Scenes submitted and not yet drawn.
#[derive(Default)]
struct InFlight {
    n: Mutex<usize>,
    changed: Condvar,
}

impl InFlight {
    fn done(&self) {
        let mut n = self.n.lock().unwrap_or_else(|e| e.into_inner());
        *n = n.saturating_sub(1);
        self.changed.notify_all();
    }
}

/// Scenes queued or drawing before [`RasterThread::submit`] waits.
const PIPELINE_DEPTH: usize = 2;

/// What a raster thread did, from [`RasterThread::finish`].
#[derive(Debug, Default)]
pub struct Rastered {
    /// Frames drawn.
    pub frames: u64,
    /// The last frame's pixels, as [`Offscreen::render`] returns them.
    pub last: Option<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// The first failure, naming the frame. Frames after it are not drawn.
    pub error: Option<String>,
    /// Images on the device when the thread finished, those still
    /// referenced, and their bytes.
    pub gpu_images: usize,
    pub gpu_bytes: u64,
}

impl RasterThread {
    /// Starts the thread and makes its target there. A target that cannot be
    /// made is this call's error.
    pub fn spawn(width: u32, height: u32) -> Result<RasterThread, RasterError> {
        RasterThread::spawn_capturing(width, height, None)
    }

    /// As [`RasterThread::spawn`], and with `capture` every frame drawn is
    /// also written there as `frame-NNNN.png`, numbered from 1: a frame
    /// sequence to look at, not only the last. The directory must exist. A
    /// frame that cannot be written is the run's failure, as one that cannot
    /// be drawn is.
    pub fn spawn_capturing(
        width: u32,
        height: u32,
        capture: Option<PathBuf>,
    ) -> Result<RasterThread, RasterError> {
        let (tx, rx) = mpsc::channel::<Work>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), RasterError>>();
        let scenes = Arc::new(InFlight::default());
        let in_flight = Arc::clone(&scenes);
        let thread = std::thread::Builder::new()
            .name("fl-raster".into())
            .spawn(move || {
                raster_main(
                    width,
                    height,
                    capture.as_deref(),
                    &rx,
                    &ready_tx,
                    &in_flight,
                )
            })
            .map_err(|e| RasterError::Unsupported(format!("raster thread: {e}")))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(RasterThread {
                tx: Some(tx),
                scenes,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(RasterError::Unsupported(
                    "raster thread ended before its target was made".into(),
                ))
            }
        }
    }

    /// Queues `scene` to be drawn; waits while one is already queued. False
    /// if the thread has gone, which only a panic in it does.
    pub fn submit(&self, scene: Arc<Scene>) -> bool {
        let Some(tx) = self.tx.as_ref() else {
            return false;
        };
        let mut n = self.scenes.n.lock().unwrap_or_else(|e| e.into_inner());
        while *n >= PIPELINE_DEPTH {
            n = self
                .scenes
                .changed
                .wait(n)
                .unwrap_or_else(|e| e.into_inner());
        }
        *n += 1;
        drop(n);
        if tx.send(Work::Scene(scene)).is_ok() {
            true
        } else {
            self.scenes.done();
            false
        }
    }

    /// What uploads a decoded image ahead of the frame that draws it: for
    /// [`fl_ui::set_image_sink`]. Never waits.
    pub fn uploader(&self) -> impl Fn(Arc<ImageData>) + Send + 'static {
        let tx = self.tx.clone();
        move |image| {
            if let Some(tx) = &tx {
                let _ = tx.send(Work::Upload(image));
            }
        }
    }

    /// Draws what is queued, stops the thread and says what it did.
    pub fn finish(mut self) -> Rastered {
        self.stop()
    }

    fn stop(&mut self) -> Rastered {
        drop(self.tx.take());
        match self.thread.take().map(JoinHandle::join) {
            Some(Ok(r)) => r,
            Some(Err(_)) => Rastered {
                error: Some("the raster thread panicked".into()),
                ..Rastered::default()
            },
            None => Rastered::default(),
        }
    }
}

impl Drop for RasterThread {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn raster_main(
    width: u32,
    height: u32,
    capture: Option<&Path>,
    rx: &Receiver<Work>,
    ready: &mpsc::Sender<Result<(), RasterError>>,
    in_flight: &InFlight,
) -> Rastered {
    let mut target = match Offscreen::new(width, height) {
        Ok(t) => {
            let _ = ready.send(Ok(()));
            t
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return Rastered::default();
        }
    };
    let mut r = Rastered {
        width,
        height,
        ..Rastered::default()
    };
    // Until the UI side hangs up. After a failure the rest are taken and
    // dropped, so the UI thread is never left waiting on a full queue.
    for work in rx {
        let scene = match work {
            Work::Scene(s) => s,
            Work::Upload(image) => {
                // A failed upload is retried, and named, by the frame that
                // draws the image.
                if r.error.is_none() {
                    let _ = target.upload(&image);
                }
                continue;
            }
        };
        let drawn = if r.error.is_some() {
            None
        } else {
            Some(target.render(&scene))
        };
        drop(scene);
        in_flight.done();
        let Some(drawn) = drawn else {
            continue;
        };
        match drawn {
            Ok(pixels) => {
                r.frames += 1;
                if let Some(dir) = capture {
                    let file = dir.join(format!("frame-{:04}.png", r.frames));
                    if let Err(e) = write_png(&file, width, height, &pixels) {
                        r.error = Some(format!("frame {}: {e}", r.frames));
                    }
                }
                r.last = Some(pixels);
            }
            Err(e) => r.error = Some(format!("frame {}: {e}", r.frames + 1)),
        }
    }
    // What is still referenced once the UI side is done: a leak shows here.
    target.images.sweep(&mut target.ctx);
    r.gpu_images = target.images().len();
    r.gpu_bytes = target.images().bytes();
    r
}

/// Walks the layer operations into `canvas`. A `pop` with nothing pushed is
/// ignored, as the engine's builder ignores it; layers left open are closed.
fn replay(canvas: &mut Canvas, scene: &Scene, slots: &mut Slots) -> Result<(), RasterError> {
    let base = canvas.save_depth();
    // The canvas depth before each open layer, so a pop restores exactly
    // what its push saved, a clip's save layer included.
    let mut open: Vec<usize> = Vec::new();
    for op in &scene.ops {
        if !matches!(op, LayerOp::Pop | LayerOp::AddPicture { .. }) {
            open.push(canvas.save_depth());
            canvas.save();
        }
        match op {
            LayerOp::PushTransform(m) => {
                canvas.concat_4x4(&m.map(|v| v as f32));
            }
            LayerOp::PushOffset { dx, dy } => {
                canvas.translate(*dx as f32, *dy as f32);
            }
            LayerOp::PushClipRect { ltrb, clip } => {
                canvas.clip_rect(rect(*ltrb))?;
                clip_layer(canvas, *clip);
            }
            LayerOp::PushClipRRect { rrect, clip } => {
                canvas.clip_path(&rrect_path(rrect))?;
                clip_layer(canvas, *clip);
            }
            LayerOp::PushClipPath { path, clip } => {
                clip_to(canvas, path)?;
                clip_layer(canvas, *clip);
            }
            LayerOp::PushOpacity { alpha, dx, dy } => {
                canvas.translate(*dx as f32, *dy as f32);
                canvas.save_layer(Layer::opacity(f32::from(*alpha) / 255.0));
            }
            LayerOp::AddPicture { dx, dy, picture } => {
                let depth = canvas.save_depth();
                canvas.save();
                canvas.translate(*dx as f32, *dy as f32);
                let drawn = draw_picture(canvas, picture, slots);
                restore_to(canvas, depth);
                drawn?;
            }
            LayerOp::Pop => {
                if let Some(depth) = open.pop() {
                    restore_to(canvas, depth);
                }
            }
        }
    }
    restore_to(canvas, base);
    Ok(())
}

/// A clip layer's own save layer, for `Clip.antiAliasWithSaveLayer`.
fn clip_layer(canvas: &mut Canvas, clip: Clip) {
    if clip == Clip::AntiAliasWithSaveLayer {
        canvas.save_layer(Layer::default());
    }
}

fn restore_to(canvas: &mut Canvas, depth: usize) {
    while canvas.save_depth() > depth {
        canvas.restore();
    }
}

/// A picture's draws, balanced: saves it leaves open are restored.
fn draw_picture(
    canvas: &mut Canvas,
    picture: &Picture,
    slots: &mut Slots,
) -> Result<(), RasterError> {
    let depth = canvas.save_depth();
    let drawn = picture
        .draws
        .iter()
        .try_for_each(|d| draw_one(canvas, d, slots));
    restore_to(canvas, depth);
    drawn
}

fn draw_one(canvas: &mut Canvas, draw: &Draw, slots: &mut Slots) -> Result<(), RasterError> {
    match draw {
        Draw::Save => {
            canvas.save();
        }
        Draw::SaveLayer { bounds, paint } => {
            let layer = layer_of(paint)?;
            match bounds {
                Some(b) => canvas.save_layer_bounds(layer, rect(*b)),
                None => canvas.save_layer(layer),
            };
        }
        Draw::Restore => {
            canvas.restore();
        }
        Draw::Translate(x, y) => {
            canvas.translate(*x, *y);
        }
        Draw::Scale(x, y) => {
            canvas.scale(*x, *y);
        }
        Draw::Rotate(r) => {
            canvas.rotate(*r);
        }
        Draw::Skew(sx, sy) => {
            // x' = x + sx y, y' = sy x + y, as SkMatrix::setSkew.
            canvas.concat(Affine2::from_cols_array(&[1.0, *sy, *sx, 1.0, 0.0, 0.0]));
        }
        Draw::Transform(m) => {
            canvas.concat_4x4(&m.map(|v| v as f32));
        }
        Draw::ClipRect { ltrb, op, .. } => {
            match op {
                ClipOp::Intersect => canvas.clip_rect(rect(*ltrb))?,
                ClipOp::Difference => canvas.clip_out_rect(rect(*ltrb))?,
            };
        }
        Draw::ClipRRect { rrect, .. } => {
            canvas.clip_path(&rrect_path(rrect))?;
        }
        Draw::ClipPath { path, .. } => clip_to(canvas, path)?,
        Draw::Paint(p) => {
            canvas.draw_paint(&paint_of(p, slots)?)?;
        }
        Draw::Color { color, blend_mode } => {
            let [r, g, b, a] = *color;
            canvas.draw_color(Color::srgb(r, g, b, a), blend_of(*blend_mode)?)?;
        }
        Draw::Line { from, to, paint } => {
            // A line is always stroked, whatever the paint's style, as Skia
            // and Impeller draw it.
            let paint = Paint {
                style: Style::Stroke,
                ..paint.clone()
            };
            canvas.draw_line(vec2(*from), vec2(*to), &paint_of(&paint, slots)?)?;
        }
        Draw::Rect { ltrb, paint } => {
            canvas.draw_rect(rect(*ltrb), &paint_of(paint, slots)?)?;
        }
        Draw::RRect { rrect, paint } => {
            canvas.draw_rrect_with_radii(
                rect(rrect.ltrb),
                radii(rrect),
                &paint_of(paint, slots)?,
            )?;
        }
        Draw::DRRect {
            outer,
            inner,
            paint,
        } => {
            canvas.draw_drrect_with_radii(
                rect(outer.ltrb),
                radii(outer),
                rect(inner.ltrb),
                radii(inner),
                &paint_of(paint, slots)?,
            )?;
        }
        Draw::Oval { ltrb, paint } => {
            canvas.draw_oval(rect(*ltrb), &paint_of(paint, slots)?)?;
        }
        Draw::Circle {
            center,
            radius,
            paint,
        } => {
            canvas.draw_circle(vec2(*center), *radius, &paint_of(paint, slots)?)?;
        }
        Draw::Path { path, paint } => {
            // A path with a non-finite point draws nothing, as in Skia.
            let path = path_of(path);
            if path.is_finite() {
                canvas.draw_path(&path, &paint_of(paint, slots)?)?;
            }
        }
        Draw::Picture(p) => draw_picture(canvas, p, slots)?,
        Draw::Glyphs { run, origin, paint } => draw_glyphs(canvas, run, *origin, paint, slots)?,
        Draw::Shadow {
            path,
            color,
            elevation,
            transparent_occluder,
            dpr,
        } => {
            // A rect, a rounded rect with one circular radius or a circle
            // goes to emblema as that shape, which takes the analytic blur:
            // Impeller's `drawShadow` simplifies the path the same way
            // (`SimplifyOrDrawPath`) and blurs those analytically too.
            let path = shadow_path_of(path);
            if path.is_finite() {
                // Impeller's occluder height, `dpr * elevation`, in device
                // pixels, as emblema's `draw_shadow` takes it.
                let [r, g, b, a] = *color;
                canvas.draw_shadow(
                    &path,
                    Color::srgb(r, g, b, a),
                    dpr * elevation,
                    *transparent_occluder,
                )?;
            }
        }
        Draw::Image {
            image,
            src,
            dst,
            paint,
            quality,
        } => {
            let (slot, size) = slots.slot(image)?;
            let p = image_paint(paint, EPaint::image(slot, rect(*dst)), *quality)?
                .with_source_pixels(rect(*src), size);
            canvas.draw_rect(rect(*dst), &p)?;
        }
        Draw::ImageNine {
            image,
            center,
            dst,
            paint,
            quality,
        } => {
            // emblema samples the nine linearly, with its own inset; the
            // paint gives the blend and the edges, not an alpha.
            if paint.color[3] < 1.0 {
                return Err(RasterError::Unsupported(
                    "drawImageNine with a translucent paint".into(),
                ));
            }
            let (slot, size) = slots.slot(image)?;
            let p = image_paint(paint, EPaint::image(slot, rect(*dst)), *quality)?;
            canvas.draw_image_nine(slot, size, rect(*center), rect(*dst), &p)?;
        }
        Draw::Atlas {
            image,
            transforms,
            rects,
            colors,
            blend_mode,
            paint,
            quality,
        } => {
            // A sprite's color is multiplied in: modulate, and nothing else.
            if colors.is_some() && *blend_mode != MODULATE {
                return Err(RasterError::Unsupported(format!(
                    "drawAtlas colors with blend mode {blend_mode} (only modulate, {MODULATE})"
                )));
            }
            let (slot, size) = slots.slot(image)?;
            let sprites: Vec<Sprite> = transforms
                .iter()
                .zip(rects)
                .enumerate()
                .map(|(i, ([scos, ssin, tx, ty], [l, t, r, b]))| Sprite {
                    source: SourceRect::new(*l, *t, r - l, b - t),
                    color: colors
                        .as_ref()
                        .and_then(|c| c.get(i))
                        .map_or(Color::WHITE, |c| argb(*c)),
                    // x' = scos x - ssin y + tx, y' = ssin x + scos y + ty.
                    transform: Affine2::from_cols_array(&[*scos, *ssin, -ssin, *scos, *tx, *ty]),
                })
                .collect();
            let p = image_paint(
                paint,
                EPaint::image(slot, Rect::new(0.0, 0.0, 1.0, 1.0)),
                *quality,
            )?;
            canvas.draw_atlas(&sprites, size, &p)?;
        }
    }
    Ok(())
}

/// `BlendMode.modulate`.
const MODULATE: i32 = 13;

/// A dart:ui color, `0xAARRGGBB`, straight sRGB.
fn argb(c: u32) -> Color {
    let ch = |shift: u32| f32::from(((c >> shift) & 0xFF) as u8) / 255.0;
    Color::srgb(ch(16), ch(8), ch(0), ch(24))
}

/// `FilterQuality` to emblema's sampling, as Impeller samples it
/// (skia_conversions.cc `ToSamplerDescriptor`): none is nearest, low
/// linear, medium and high both linear between mip levels. Impeller has no
/// cubic; it takes `kCubic` as `kMipmapLinear`.
fn sampling_of(quality: i32) -> Sampling {
    match quality {
        0 => Sampling::Nearest,
        2 | 3 => Sampling::Mipmap,
        _ => Sampling::Linear,
    }
}

/// `TileMode`: clamp, repeated, mirror, decal, in dart:ui's order and
/// emblema's.
fn tile_of(i: i32) -> Result<TileMode, RasterError> {
    Ok(match i {
        0 => TileMode::Clamp,
        1 => TileMode::Repeat,
        2 => TileMode::Mirror,
        3 => TileMode::Decal,
        _ => return Err(RasterError::Unsupported(format!("tile mode {i}"))),
    })
}

/// An image draw's paint: `base` sampled at `quality`, with the paint's
/// alpha, blend, edges and mask filter. The paint's color is otherwise
/// unused, as Skia draws an image.
fn image_paint(p: &Paint, base: EPaint, quality: i32) -> Result<EPaint, RasterError> {
    if let Some(what) = unmapped(p) {
        return Err(RasterError::Unsupported(what));
    }
    let paint = with_mask(base, p)
        .with_sampling(sampling_of(quality))
        .with_image_alpha(p.color[3]);
    Ok(paint
        .with_blend(blend_of(p.blend_mode)?)
        .with_anti_alias(p.anti_alias))
}

/// An image shader's fill: the image placed by its matrix, which may scale
/// and translate; a rotation, skew or perspective is not mapped yet, nor
/// tile modes that differ between the axes.
fn shader_paint(
    s: &ImageShader,
    filter_quality: i32,
    slots: &mut Slots,
) -> Result<EPaint, RasterError> {
    let m = &s.matrix;
    let affine_only = m[1] == 0.0 && m[4] == 0.0 && m[3] == 0.0 && m[7] == 0.0 && m[15] == 1.0;
    if !affine_only || m[0] <= 0.0 || m[5] <= 0.0 {
        return Err(RasterError::Unsupported(
            "an ImageShader matrix other than a scale and translation".into(),
        ));
    }
    if s.tile[0] != s.tile[1] {
        return Err(RasterError::Unsupported(
            "an ImageShader with a tile mode per axis".into(),
        ));
    }
    let (slot, size) = slots.slot(&s.image)?;
    let (w, h) = (size.width as f64, size.height as f64);
    let covers = Rect::new(
        m[12] as f32,
        m[13] as f32,
        (m[12] + m[0] * w) as f32,
        (m[13] + m[5] * h) as f32,
    );
    Ok(EPaint::image(slot, covers)
        .with_tile_mode(tile_of(s.tile[0])?)
        .with_sampling(sampling_of(s.quality.unwrap_or(filter_quality))))
}

fn rect([l, t, r, b]: [f32; 4]) -> Rect {
    Rect::new(l, t, r, b)
}

fn vec2([x, y]: fl_ui::Point) -> Vec2 {
    Vec2::new(x, y)
}

/// dart:ui's corners are top-left, top-right, bottom-right, bottom-left;
/// emblema's are top-left, top-right, bottom-left, bottom-right.
fn radii(rr: &RRect) -> RoundingRadii {
    let [tl, tr, br, bl] = rr.radii;
    [tl, tr, bl, br]
}

fn rrect_path(rr: &RRect) -> emblema::Path {
    rect(rr.ltrb).to_rounded_path_with_radii(radii(rr))
}

/// Clips to `path`; to nothing if it has a non-finite point, as in Skia.
fn clip_to(canvas: &mut Canvas, path: &fl_ui::Path) -> Result<(), RasterError> {
    let path = path_of(path);
    if path.is_finite() {
        canvas.clip_path(&path)?;
    } else {
        canvas.clip_rect(Rect::new(0.0, 0.0, 0.0, 0.0))?;
    }
    Ok(())
}

fn path_of(p: &fl_ui::Path) -> emblema::Path {
    let mut b = PathBuilder::new().with_fill_rule(match p.fill {
        FillType::NonZero => FillRule::NonZero,
        FillType::EvenOdd => FillRule::EvenOdd,
    });
    for s in &p.segs {
        match *s {
            Seg::Move(p) => b.move_to(vec2(p)),
            Seg::Line(p) => b.line_to(vec2(p)),
            Seg::Quad(c, p) => b.quad_to(vec2(c), vec2(p)),
            Seg::Conic(c, p, w) => b.conic_to(vec2(c), vec2(p), w),
            Seg::Cubic(c0, c1, p) => b.cubic_to(vec2(c0), vec2(c1), vec2(p)),
            Seg::Close => b.close(),
        };
    }
    b.build()
}

/// A paragraph's glyph run: rasterized in the first replay, drawn from
/// the atlas in the second, or drawn as outlines.
fn draw_glyphs(
    canvas: &mut Canvas,
    run: &fl_text::paint::GlyphRun,
    origin: [f32; 2],
    paint: &Paint,
    slots: &mut Slots,
) -> Result<(), RasterError> {
    let transform = canvas.transform();
    let drawing = match &mut slots.glyphs {
        GlyphPass::None => return Ok(()),
        GlyphPass::Prepare(g) => {
            g.prepare(transform, run);
            return Ok(());
        }
        GlyphPass::Draw { glyphs, slot } => (*glyphs, *slot),
    };
    let (glyphs, slot) = drawing;
    // A shader or a stroke needs the glyphs' shape, not their coverage,
    // as `ShouldRenderAsPath` sends them to paths on Impeller.
    let as_path = paint.shader.is_some() || paint.objects || paint.style == Style::Stroke;
    let quads = if as_path {
        None
    } else {
        glyphs.positioned(transform, run, origin)
    };
    let p = paint_of(paint, slots)?;
    match quads {
        Some(q) => {
            canvas.draw_glyphs(&q, glyphs.atlas(), slot, &p)?;
        }
        None => {
            if let Some(path) = glyphs.outlines(run, origin) {
                canvas.draw_path(&path, &p)?;
            }
        }
    }
    Ok(())
}

/// A shadow's path, its shape recorded when it is one emblema blurs
/// analytically.
fn shadow_path_of(p: &fl_ui::Path) -> emblema::Path {
    match rounded_rect_of(p) {
        // The same outline, built by emblema so that it carries the shape.
        Some((r, radius)) => rect(r).to_rounded_path(radius),
        None => path_of(p),
    }
}

/// The rect, rounded rect or circle `p` is, as its bounds and one corner
/// radius: what Skia's `IsRect`, `IsRRect` with all corners alike, and
/// `IsOval` on a square find, read from the segments fl-ui's `addRect`,
/// `addRRect` and `addOval` write. Anything else is `None`.
fn rounded_rect_of(p: &fl_ui::Path) -> Option<([f32; 4], f32)> {
    let close = |a: f32, b: f32, size: f32| (a - b).abs() <= 1e-4 * size.max(1.0);
    match p.segs.as_slice() {
        // A closed contour of four axis-aligned sides.
        [Seg::Move(a), Seg::Line(b), Seg::Line(c), Seg::Line(d), rest @ ..]
            if matches!(rest, [Seg::Close] | [Seg::Line(_), Seg::Close]) =>
        {
            if let [Seg::Line(e), Seg::Close] = rest {
                if e != a {
                    return None;
                }
            }
            let pts = [*a, *b, *c, *d];
            let (l, r) = (
                pts.iter().map(|q| q[0]).fold(f32::MAX, f32::min),
                pts.iter().map(|q| q[0]).fold(f32::MIN, f32::max),
            );
            let (t, bo) = (
                pts.iter().map(|q| q[1]).fold(f32::MAX, f32::min),
                pts.iter().map(|q| q[1]).fold(f32::MIN, f32::max),
            );
            let size = (r - l).max(bo - t);
            let corner = |q: &[f32; 2]| {
                (close(q[0], l, size) || close(q[0], r, size))
                    && (close(q[1], t, size) || close(q[1], bo, size))
            };
            let sides = pts
                .iter()
                .zip(pts.iter().cycle().skip(1))
                .all(|(u, v)| close(u[0], v[0], size) || close(u[1], v[1], size));
            (r > l && bo > t && pts.iter().all(corner) && sides).then_some(([l, t, r, bo], 0.0))
        }
        // addRRect: four sides, four quarter conics, one circular radius.
        [Seg::Move(m), Seg::Line(_), Seg::Conic(c0, _, w0), Seg::Line(_), Seg::Conic(c1, _, w1), Seg::Line(_), Seg::Conic(c2, _, w2), Seg::Line(_), Seg::Conic(c3, e3, w3), Seg::Close] =>
        {
            let (l, t, r, b) = (c3[0], c0[1], c0[0], c1[1]);
            let size = (r - l).max(b - t);
            let radius = m[0] - l;
            let ok = close(c1[0], r, size)
                && close(c2[0], l, size)
                && close(c2[1], b, size)
                && close(c3[1], t, size)
                && [w0, w1, w2, w3]
                    .iter()
                    .all(|w| close(**w, std::f32::consts::FRAC_1_SQRT_2, 1.0))
                && radius > 0.0
                && close(e3[0], m[0], size)
                && close(e3[1], m[1], size)
                && corners_alike(&p.segs, [l, t, r, b], radius, size);
            ok.then_some(([l, t, r, b], radius))
        }
        // addOval on a square: four quarter conics around a circle.
        [Seg::Move(m), Seg::Conic(..), Seg::Conic(..), Seg::Conic(..), Seg::Conic(_, e, _), Seg::Close] =>
        {
            let mut pts = vec![*m];
            for s in &p.segs {
                if let Seg::Conic(c, q, _) = s {
                    pts.push(*c);
                    pts.push(*q);
                }
            }
            let l = pts.iter().map(|q| q[0]).fold(f32::MAX, f32::min);
            let r = pts.iter().map(|q| q[0]).fold(f32::MIN, f32::max);
            let t = pts.iter().map(|q| q[1]).fold(f32::MAX, f32::min);
            let b = pts.iter().map(|q| q[1]).fold(f32::MIN, f32::max);
            let size = (r - l).max(b - t);
            let (cx, cy, rad) = ((l + r) / 2.0, (t + b) / 2.0, (r - l) / 2.0);
            let on_circle = p.segs.iter().all(|s| match s {
                Seg::Conic(_, q, w) => {
                    close(*w, std::f32::consts::FRAC_1_SQRT_2, 1.0)
                        && close(
                            ((q[0] - cx).powi(2) + (q[1] - cy).powi(2)).sqrt(),
                            rad,
                            size,
                        )
                }
                _ => true,
            });
            let ok = r > l
                && close(r - l, b - t, size)
                && on_circle
                && close(e[0], m[0], size)
                && close(e[1], m[1], size);
            ok.then_some(([l, t, r, b], rad))
        }
        _ => None,
    }
}

/// Whether every corner of an `addRRect` contour has the circular `radius`:
/// each side's ends sit `radius` in from the corners.
fn corners_alike(segs: &[Seg], [l, t, r, b]: [f32; 4], radius: f32, size: f32) -> bool {
    let close = |a: f32, b: f32| (a - b).abs() <= 1e-4 * size.max(1.0);
    let want = [
        [l + radius, t],
        [r - radius, t],
        [r, t + radius],
        [r, b - radius],
        [r - radius, b],
        [l + radius, b],
        [l, b - radius],
        [l, t + radius],
    ];
    let got: Vec<[f32; 2]> = segs
        .iter()
        .filter_map(|s| match s {
            Seg::Move(q) | Seg::Line(q) => Some(*q),
            Seg::Conic(_, q, _) => Some(*q),
            _ => None,
        })
        .collect();
    // Move, line, conic end, line, conic end, ... : the eight points in
    // order, then the conic back to the start.
    got.len() == 9
        && got[..8]
            .iter()
            .zip(want)
            .all(|(g, w)| close(g[0], w[0]) && close(g[1], w[1]))
}

/// dart:ui's `BlendMode`, by index. emblema numbers its modes as upstream
/// does (jwinarske/emblema#225), and `ALL` is in that order.
fn blend_of(i: i32) -> Result<BlendMode, RasterError> {
    usize::try_from(i)
        .ok()
        .and_then(|i| BlendMode::ALL.get(i).copied())
        .ok_or_else(|| RasterError::Unsupported(format!("blend mode {i}")))
}

/// What of a Paint emblema is not given yet, named.
fn unmapped(p: &Paint) -> Option<String> {
    if p.color_space != 0 {
        return Some(format!(
            "color space {} (only sRGB, 0, is mapped)",
            p.color_space
        ));
    }
    if p.objects {
        return Some("a shader other than ImageShader, a color filter or an image filter".into());
    }
    if p.invert_colors {
        return Some("invertColors".into());
    }
    None
}

/// A `saveLayer` paint: its alpha and blend mode composite the layer.
fn layer_of(p: &Paint) -> Result<Layer, RasterError> {
    if let Some(what) = unmapped(p)
        .or_else(|| p.mask_blur.map(|_| "a layer mask filter".into()))
        .or_else(|| p.shader.as_ref().map(|_| "a layer shader".into()))
    {
        return Err(RasterError::Unsupported(what));
    }
    Ok(Layer::opacity(p.color[3]).with_blend(blend_of(p.blend_mode)?))
}

fn paint_of(p: &Paint, slots: &mut Slots) -> Result<EPaint, RasterError> {
    if let Some(what) = unmapped(p) {
        return Err(RasterError::Unsupported(what));
    }
    // dart:ui colors are sRGB-encoded components, as emblema's `srgb` takes.
    // A shader replaces the color, but for its alpha.
    let [r, g, b, a] = p.color;
    let base = match &p.shader {
        Some(s) => shader_paint(s, p.filter_quality, slots)?.with_image_alpha(a),
        None => EPaint::fill(Color::srgb(r, g, b, a)),
    };
    let paint = match p.style {
        Style::Fill => base,
        Style::Stroke => base.with_style(EStyle::Stroke(StrokeStyle {
            width: p.stroke_width,
            cap: match p.stroke_cap {
                Cap::Butt => LineCap::Butt,
                Cap::Round => LineCap::Round,
                Cap::Square => LineCap::Square,
            },
            join: match p.stroke_join {
                Join::Miter => LineJoin::Miter,
                Join::Round => LineJoin::Round,
                Join::Bevel => LineJoin::Bevel,
            },
            miter_limit: p.stroke_miter_limit,
        })),
    };
    Ok(with_mask(paint, p)
        .with_blend(blend_of(p.blend_mode)?)
        .with_anti_alias(p.anti_alias))
}

/// `paint` with `p`'s blur mask filter, if it has one.
fn with_mask(paint: EPaint, p: &Paint) -> EPaint {
    match p.mask_blur {
        Some((style, sigma)) => paint
            .with_mask_blur(sigma)
            .with_mask_blur_style(match style {
                1 => MaskBlurStyle::Solid,
                2 => MaskBlurStyle::Outer,
                3 => MaskBlurStyle::Inner,
                _ => MaskBlurStyle::Normal,
            }),
        None => paint,
    }
}

/// Writes `rgba` (`width * height` RGBA8 pixels, top row first) as a PNG.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), RasterError> {
    let png = |e: &dyn std::fmt::Display| RasterError::Png(format!("{}: {e}", path.display()));
    let file = std::fs::File::create(path).map_err(|e| png(&e))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| png(&e))?;
    w.write_image_data(rgba).map_err(|e| png(&e))?;
    w.finish().map_err(|e| png(&e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fl_ui::Clip;
    use std::sync::Arc;

    fn picture(draws: Vec<Draw>) -> Arc<Picture> {
        Arc::new(Picture { draws })
    }

    #[test]
    fn unmapped_paints_are_errors() {
        let out_of_range = Paint {
            blend_mode: 40,
            ..Paint::default()
        };
        assert!(matches!(
            paint_of(&out_of_range, &mut Slots::new(&GpuImages::default())),
            Err(RasterError::Unsupported(_))
        ));
        let p3 = Paint {
            color_space: 2,
            ..Paint::default()
        };
        assert!(matches!(
            paint_of(&p3, &mut Slots::new(&GpuImages::default())),
            Err(RasterError::Unsupported(_))
        ));
        let shader = Paint {
            objects: true,
            ..Paint::default()
        };
        assert!(matches!(
            paint_of(&shader, &mut Slots::new(&GpuImages::default())),
            Err(RasterError::Unsupported(_))
        ));
        assert!(paint_of(&Paint::default(), &mut Slots::new(&GpuImages::default())).is_ok());
    }

    #[test]
    fn a_shadow_keeps_the_shapes_impeller_blurs_analytically() {
        let mut rect = fl_ui::Path::default();
        rect.add_rect([20.0, 20.0, 80.0, 70.0]);
        assert_eq!(
            rounded_rect_of(&rect),
            Some(([20.0, 20.0, 80.0, 70.0], 0.0))
        );
        let rr = |radii: [[f32; 2]; 4]| {
            let mut p = fl_ui::Path::default();
            p.add_rrect(&RRect {
                ltrb: [20.0, 120.0, 80.0, 170.0],
                radii,
            });
            p
        };
        assert_eq!(
            rounded_rect_of(&rr([[12.0; 2]; 4])),
            Some(([20.0, 120.0, 80.0, 170.0], 12.0))
        );
        // Corners that differ, or that are not circular: not one radius.
        assert_eq!(
            rounded_rect_of(&rr([[12.0; 2], [12.0; 2], [12.0; 2], [6.0; 2]])),
            None
        );
        assert_eq!(rounded_rect_of(&rr([[12.0, 6.0]; 4])), None);
        let oval = |ltrb| {
            let mut p = fl_ui::Path::default();
            p.add_oval(ltrb);
            p
        };
        let (r, radius) = rounded_rect_of(&oval([10.0, 10.0, 70.0, 70.0])).unwrap();
        assert!(r
            .iter()
            .zip([10.0, 10.0, 70.0, 70.0])
            .all(|(a, b)| (a - b).abs() < 1e-3));
        assert!((radius - 30.0).abs() < 1e-3);
        // An ellipse is not a circle; upstream draws its shadow another way.
        assert_eq!(rounded_rect_of(&oval([10.0, 10.0, 90.0, 60.0])), None);
        // Two contours, or a triangle, are no shape.
        let mut two = rect.clone();
        two.add_rect([100.0, 0.0, 120.0, 20.0]);
        assert_eq!(rounded_rect_of(&two), None);
        let mut tri = fl_ui::Path::default();
        tri.add_polygon(&[0.0, 0.0, 10.0, 0.0, 5.0, 8.0], true);
        assert_eq!(rounded_rect_of(&tri), None);
        // A shape-carrying path reaches emblema's analytic route.
        assert!(shadow_path_of(&rr([[12.0; 2]; 4]))
            .as_rounded_rect()
            .is_some());
        assert!(shadow_path_of(&tri).as_rounded_rect().is_none());
    }

    #[test]
    fn blend_modes_are_dart_ui_indices() {
        // Every index to the mode with that number, as dart:ui and Impeller
        // number them; multiply is the one 0.2.0 had elsewhere.
        for i in 0..29 {
            assert_eq!(blend_of(i).unwrap() as i32, i);
        }
        assert_eq!(blend_of(24).unwrap(), BlendMode::Multiply);
        assert_eq!(blend_of(3).unwrap(), BlendMode::SrcOver);
        assert!(blend_of(29).is_err());
        assert!(blend_of(-1).is_err());
    }

    #[test]
    fn rrect_corners_are_reordered() {
        let rr = RRect {
            ltrb: [0.0, 0.0, 10.0, 10.0],
            radii: [[1.0; 2], [2.0; 2], [3.0; 2], [4.0; 2]],
        };
        // dart:ui TL TR BR BL to emblema TL TR BL BR.
        assert_eq!(radii(&rr), [[1.0; 2], [2.0; 2], [4.0; 2], [3.0; 2]]);
    }

    #[test]
    fn unbalanced_pops_are_ignored() {
        let mut canvas = Canvas::new(Extent2D::new(4, 4));
        let scene = Scene {
            ops: vec![LayerOp::Pop, LayerOp::Pop],
        };
        assert!(replay(&mut canvas, &scene, &mut Slots::new(&GpuImages::default())).is_ok());
        assert_eq!(canvas.save_depth(), 0);
    }

    #[test]
    fn each_pop_undoes_its_push_and_open_layers_close() {
        let mut canvas = Canvas::new(Extent2D::new(8, 8));
        let scene = Scene {
            ops: vec![
                LayerOp::PushClipRect {
                    ltrb: [0.0, 0.0, 4.0, 4.0],
                    clip: Clip::AntiAliasWithSaveLayer,
                },
                LayerOp::PushOpacity {
                    alpha: 128,
                    dx: 1.0,
                    dy: 1.0,
                },
                // A picture that leaves a save open.
                LayerOp::AddPicture {
                    dx: 0.0,
                    dy: 0.0,
                    picture: picture(vec![Draw::Save, Draw::Save, Draw::Restore]),
                },
                LayerOp::Pop,
                // The clip layer left open.
            ],
        };
        assert!(replay(&mut canvas, &scene, &mut Slots::new(&GpuImages::default())).is_ok());
        assert_eq!(canvas.save_depth(), 0);
    }

    #[test]
    fn a_failed_draw_leaves_the_canvas_balanced() {
        let mut canvas = Canvas::new(Extent2D::new(8, 8));
        let bad = Paint {
            objects: true,
            ..Paint::default()
        };
        let scene = Scene {
            ops: vec![LayerOp::AddPicture {
                dx: 0.0,
                dy: 0.0,
                picture: picture(vec![
                    Draw::Save,
                    Draw::Rect {
                        ltrb: [0.0, 0.0, 1.0, 1.0],
                        paint: bad,
                    },
                ]),
            }],
        };
        assert!(matches!(
            replay(&mut canvas, &scene, &mut Slots::new(&GpuImages::default())),
            Err(RasterError::Unsupported(_))
        ));
        assert_eq!(canvas.save_depth(), 0);
    }

    #[test]
    fn non_finite_paths_draw_nothing_and_clip_to_nothing() {
        let mut path = fl_ui::Path::default();
        path.move_to([0.0, 0.0]);
        path.line_to([f32::NAN, 1.0]);
        path.line_to([1.0, 1.0]);
        let path = Arc::new(path);
        let mut canvas = Canvas::new(Extent2D::new(8, 8));
        let scene = Scene {
            ops: vec![LayerOp::AddPicture {
                dx: 0.0,
                dy: 0.0,
                picture: picture(vec![
                    Draw::Path {
                        path: Arc::clone(&path),
                        paint: Paint::default(),
                    },
                    Draw::ClipPath {
                        path,
                        anti_alias: true,
                    },
                ]),
            }],
        };
        assert!(replay(&mut canvas, &scene, &mut Slots::new(&GpuImages::default())).is_ok());
        assert_eq!(canvas.finish().draw_count(), 0);
    }

    /// What crosses from the UI thread to the raster thread is only scenes
    /// and frames; they must be able to (brief M2 §5).
    #[test]
    fn scenes_and_frames_cross_threads() {
        fn crosses<T: Send + Sync>() {}
        crosses::<Scene>();
        crosses::<Picture>();
        crosses::<fl_ui::Frame>();
        crosses::<RasterThread>();
    }
}
