//! The raster side (brief M2 §4, §5): a dart:ui [`Scene`] replayed into one
//! emblema `Canvas` and drawn.
//!
//! The layer tree is flattened as it is walked: a transform or offset layer
//! is `save` plus `concat`/`translate`, a picture is replayed in place under
//! its offset, and `pop` is `restore`.
//!
//! What emblema cannot yet be asked to draw faithfully is an error, not an
//! approximation: a draw in a blend mode or color space not mapped here
//! fails the frame and names what it was. Pixels that are quietly wrong
//! are the failure goldens exist to catch, and they would catch it late.

use std::path::Path;

use emblema::{
    BackendPreference, Canvas, Color, Context, Extent2D, Paint as EPaint, PixelFormat, Rect,
    Surface,
};
use fl_ui::{Draw, LayerOp, Paint, Scene, Style};

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
pub struct Offscreen {
    ctx: Context,
    surface: Option<Surface>,
    width: u32,
    height: u32,
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

/// Walks the layer operations into `canvas`. A `pop` with nothing pushed is
/// ignored, as the engine's builder ignores it.
fn replay(canvas: &mut Canvas, scene: &Scene) -> Result<(), RasterError> {
    let mut depth = 0usize;
    for op in &scene.ops {
        match op {
            LayerOp::PushTransform(m) => {
                let m: [f32; 16] = m.map(|v| v as f32);
                canvas.save();
                canvas.concat_4x4(&m);
                depth += 1;
            }
            LayerOp::PushOffset { dx, dy } => {
                canvas.save();
                canvas.translate(*dx as f32, *dy as f32);
                depth += 1;
            }
            LayerOp::AddPicture { dx, dy, picture } => {
                canvas.save();
                canvas.translate(*dx as f32, *dy as f32);
                for draw in &picture.draws {
                    draw_one(canvas, draw)?;
                }
                canvas.restore();
            }
            LayerOp::Pop => {
                if depth > 0 {
                    canvas.restore();
                    depth -= 1;
                }
            }
        }
    }
    Ok(())
}

fn draw_one(canvas: &mut Canvas, draw: &Draw) -> Result<(), RasterError> {
    match draw {
        Draw::Rect { ltrb, paint } => {
            let [l, t, r, b] = ltrb.map(|v| v as f32);
            canvas.draw_rect(Rect::new(l, t, r, b), &paint_of(paint)?)?;
        }
    }
    Ok(())
}

/// srcOver, the default; others are not mapped yet.
const BLEND_SRC_OVER: i32 = 3;

fn paint_of(p: &Paint) -> Result<EPaint, RasterError> {
    if p.color_space != 0 {
        return Err(RasterError::Unsupported(format!(
            "color space {} (only sRGB, 0, is mapped)",
            p.color_space
        )));
    }
    if p.blend_mode != BLEND_SRC_OVER {
        return Err(RasterError::Unsupported(format!(
            "blend mode {} (only srcOver, 3, is mapped)",
            p.blend_mode
        )));
    }
    // dart:ui colors are sRGB-encoded components, as emblema's `srgb` takes.
    let [r, g, b, a] = p.color;
    let color = Color::srgb(r, g, b, a);
    let paint = match p.style {
        Style::Fill => EPaint::fill(color),
        Style::Stroke => EPaint::stroke(color, p.stroke_width),
    };
    Ok(paint.with_anti_alias(p.anti_alias))
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
    use fl_ui::Picture;
    use std::sync::Arc;

    fn rect_scene(ltrb: [f64; 4], paint: Paint) -> Scene {
        Scene {
            ops: vec![
                LayerOp::PushTransform([
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
                ]),
                LayerOp::AddPicture {
                    dx: 0.0,
                    dy: 0.0,
                    picture: Arc::new(Picture {
                        draws: vec![Draw::Rect { ltrb, paint }],
                    }),
                },
                LayerOp::Pop,
            ],
        }
    }

    #[test]
    fn unmapped_blend_and_color_space_are_errors() {
        let multiply = Paint {
            blend_mode: 24,
            ..Paint::default()
        };
        assert!(matches!(
            paint_of(&multiply),
            Err(RasterError::Unsupported(_))
        ));
        let p3 = Paint {
            color_space: 2,
            ..Paint::default()
        };
        assert!(matches!(paint_of(&p3), Err(RasterError::Unsupported(_))));
        assert!(paint_of(&Paint::default()).is_ok());
    }

    #[test]
    fn unbalanced_pops_are_ignored() {
        let mut canvas = Canvas::new(Extent2D::new(4, 4));
        let scene = Scene {
            ops: vec![LayerOp::Pop, LayerOp::Pop],
        };
        assert!(replay(&mut canvas, &scene).is_ok());
        assert_eq!(canvas.save_depth(), 0);
        let _ = rect_scene([0.0; 4], Paint::default());
    }
}
