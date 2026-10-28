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
use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;

use emblema::{
    Affine2, BackendPreference, BlendMode, Canvas, Color, Context, Extent2D, FillRule, Layer,
    LineCap, LineJoin, MaskBlurStyle, Paint as EPaint, PathBuilder, PixelFormat, Rect,
    RoundingRadii, StrokeStyle, Style as EStyle, Surface, Vec2,
};
use fl_ui::{
    Cap, Clip, ClipOp, Draw, FillType, Join, LayerOp, Paint, Picture, RRect, Scene, Seg, Style,
};

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
    pub fn render(&mut self, scene: &Scene) -> Result<Vec<u8>, RasterError> {
        let mut canvas = Canvas::new(Extent2D::new(self.width, self.height));
        canvas.clear(Color::TRANSPARENT);
        replay(&mut canvas, scene)?;
        let recording = canvas.finish();
        let surface = self
            .surface
            .as_mut()
            .ok_or_else(|| RasterError::Unsupported("a destroyed target".into()))?;
        self.ctx.draw(surface, &recording)?;
        Ok(self.ctx.read(surface)?)
    }
}

impl Drop for Offscreen {
    /// emblema frees nothing on drop; the surface goes before its context.
    fn drop(&mut self) {
        if let Some(surface) = self.surface.take() {
            self.ctx.destroy_surface(surface);
        }
    }
}

/// The raster thread: an [`Offscreen`] made and used on a thread of its
/// own, drawing the scenes the UI thread submits, in order.
///
/// The queue holds one scene beyond the one being drawn, Flutter's pipeline
/// depth of two: a UI thread that gets further ahead waits in
/// [`RasterThread::submit`] rather than piling up frames.
pub struct RasterThread {
    tx: Option<SyncSender<Arc<Scene>>>,
    thread: Option<JoinHandle<Rastered>>,
}

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
}

impl RasterThread {
    /// Starts the thread and makes its target there. A target that cannot be
    /// made is this call's error.
    pub fn spawn(width: u32, height: u32) -> Result<RasterThread, RasterError> {
        let (tx, rx) = mpsc::sync_channel::<Arc<Scene>>(1);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), RasterError>>();
        let thread = std::thread::Builder::new()
            .name("fl-raster".into())
            .spawn(move || raster_main(width, height, &rx, &ready_tx))
            .map_err(|e| RasterError::Unsupported(format!("raster thread: {e}")))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(RasterThread {
                tx: Some(tx),
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
        self.tx.as_ref().is_some_and(|tx| tx.send(scene).is_ok())
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
    rx: &Receiver<Arc<Scene>>,
    ready: &mpsc::Sender<Result<(), RasterError>>,
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
    for scene in rx {
        if r.error.is_some() {
            continue;
        }
        match target.render(&scene) {
            Ok(pixels) => {
                r.frames += 1;
                r.last = Some(pixels);
            }
            Err(e) => r.error = Some(format!("frame {}: {e}", r.frames + 1)),
        }
    }
    r
}

/// Walks the layer operations into `canvas`. A `pop` with nothing pushed is
/// ignored, as the engine's builder ignores it; layers left open are closed.
fn replay(canvas: &mut Canvas, scene: &Scene) -> Result<(), RasterError> {
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
                let drawn = draw_picture(canvas, picture);
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
fn draw_picture(canvas: &mut Canvas, picture: &Picture) -> Result<(), RasterError> {
    let depth = canvas.save_depth();
    let drawn = picture.draws.iter().try_for_each(|d| draw_one(canvas, d));
    restore_to(canvas, depth);
    drawn
}

fn draw_one(canvas: &mut Canvas, draw: &Draw) -> Result<(), RasterError> {
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
            canvas.draw_paint(&paint_of(p)?)?;
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
                ..*paint
            };
            canvas.draw_line(vec2(*from), vec2(*to), &paint_of(&paint)?)?;
        }
        Draw::Rect { ltrb, paint } => {
            canvas.draw_rect(rect(*ltrb), &paint_of(paint)?)?;
        }
        Draw::RRect { rrect, paint } => {
            canvas.draw_rrect_with_radii(rect(rrect.ltrb), radii(rrect), &paint_of(paint)?)?;
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
                &paint_of(paint)?,
            )?;
        }
        Draw::Oval { ltrb, paint } => {
            canvas.draw_oval(rect(*ltrb), &paint_of(paint)?)?;
        }
        Draw::Circle {
            center,
            radius,
            paint,
        } => {
            canvas.draw_circle(vec2(*center), *radius, &paint_of(paint)?)?;
        }
        Draw::Path { path, paint } => {
            // A path with a non-finite point draws nothing, as in Skia.
            let path = path_of(path);
            if path.is_finite() {
                canvas.draw_path(&path, &paint_of(paint)?)?;
            }
        }
        Draw::Picture(p) => draw_picture(canvas, p)?,
    }
    Ok(())
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

/// dart:ui's `BlendMode`, by index, as emblema names it. The two enums
/// hold the same modes in different orders.
fn blend_of(i: i32) -> Result<BlendMode, RasterError> {
    use BlendMode::*;
    const MODES: [BlendMode; 29] = [
        Clear, Src, Dst, SrcOver, DstOver, SrcIn, DstIn, SrcOut, DstOut, SrcATop, DstATop, Xor,
        Plus, Modulate, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight,
        SoftLight, Difference, Exclusion, Multiply, Hue, Saturation, Color, Luminosity,
    ];
    usize::try_from(i)
        .ok()
        .and_then(|i| MODES.get(i).copied())
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
        return Some("a shader, color filter or image filter".into());
    }
    if p.invert_colors {
        return Some("invertColors".into());
    }
    None
}

/// A `saveLayer` paint: its alpha and blend mode composite the layer.
fn layer_of(p: &Paint) -> Result<Layer, RasterError> {
    if let Some(what) = unmapped(p).or_else(|| p.mask_blur.map(|_| "a layer mask filter".into())) {
        return Err(RasterError::Unsupported(what));
    }
    Ok(Layer::opacity(p.color[3]).with_blend(blend_of(p.blend_mode)?))
}

fn paint_of(p: &Paint) -> Result<EPaint, RasterError> {
    if let Some(what) = unmapped(p) {
        return Err(RasterError::Unsupported(what));
    }
    // dart:ui colors are sRGB-encoded components, as emblema's `srgb` takes.
    let [r, g, b, a] = p.color;
    let color = Color::srgb(r, g, b, a);
    let mut paint = match p.style {
        Style::Fill => EPaint::fill(color),
        Style::Stroke => EPaint::fill(color).with_style(EStyle::Stroke(StrokeStyle {
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
    if let Some((style, sigma)) = p.mask_blur {
        paint = paint
            .with_mask_blur(sigma)
            .with_mask_blur_style(match style {
                1 => MaskBlurStyle::Solid,
                2 => MaskBlurStyle::Outer,
                3 => MaskBlurStyle::Inner,
                _ => MaskBlurStyle::Normal,
            });
    }
    Ok(paint
        .with_blend(blend_of(p.blend_mode)?)
        .with_anti_alias(p.anti_alias))
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
            paint_of(&out_of_range),
            Err(RasterError::Unsupported(_))
        ));
        let p3 = Paint {
            color_space: 2,
            ..Paint::default()
        };
        assert!(matches!(paint_of(&p3), Err(RasterError::Unsupported(_))));
        let shader = Paint {
            objects: true,
            ..Paint::default()
        };
        assert!(matches!(
            paint_of(&shader),
            Err(RasterError::Unsupported(_))
        ));
        assert!(paint_of(&Paint::default()).is_ok());
    }

    #[test]
    fn blend_modes_map_by_name() {
        // dart:ui's multiply is index 24; emblema's enum has it elsewhere.
        assert_eq!(blend_of(24).unwrap(), BlendMode::Multiply);
        assert_eq!(blend_of(3).unwrap(), BlendMode::SrcOver);
        assert_eq!(blend_of(28).unwrap(), BlendMode::Luminosity);
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
        assert!(replay(&mut canvas, &scene).is_ok());
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
        assert!(replay(&mut canvas, &scene).is_ok());
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
            replay(&mut canvas, &scene),
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
        assert!(replay(&mut canvas, &scene).is_ok());
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
