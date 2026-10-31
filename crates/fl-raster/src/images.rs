//! The GPU image cache (brief M3 §4, §6): each dart:ui image uploaded once,
//! kept while its [`ImageData`] lives, destroyed on the first frame after
//! the last reference goes.
//!
//! Keyed by the `Arc`'s address and held by a `Weak`, so the cache never
//! keeps an image alive and Dart's lifetimes decide GPU memory. An entry
//! whose `Weak` is dead is stale even if a new image lands at its address.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc, Weak};

use emblema::{Context, Extent2D, Image, PixelFormat};
use fl_ui::ImageData;

use crate::RasterError;

struct Entry {
    data: Weak<ImageData>,
    image: Image,
    size: Extent2D,
    bytes: u64,
}

/// The device's copies of the images frames draw.
///
/// Not `Send`: its images belong to the raster thread's device, as
/// [`crate::Offscreen`] does.
pub struct GpuImages {
    map: HashMap<usize, Entry>,
    bytes: u64,
    _raster_thread: PhantomData<*const ()>,
}

fn key(data: &Arc<ImageData>) -> usize {
    Arc::as_ptr(data) as usize
}

impl Default for GpuImages {
    fn default() -> Self {
        GpuImages {
            map: HashMap::new(),
            bytes: 0,
            _raster_thread: PhantomData,
        }
    }
}

impl GpuImages {
    /// Images on the device.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Their device memory, mip levels included.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Uploads `data` if it is not on the device yet. The CPU copy is taken
    /// and released once written, so a decoded image is not held twice.
    pub(crate) fn upload(
        &mut self,
        ctx: &mut Context,
        data: &Arc<ImageData>,
    ) -> Result<(), RasterError> {
        let k = key(data);
        if let Some(e) = self.map.get(&k) {
            if e.data.strong_count() > 0 {
                return Ok(());
            }
            // A stale entry at a reused address.
            self.remove(ctx, k);
        }
        let size = Extent2D::new(data.width, data.height);
        if size.width == 0 || size.height == 0 {
            return Err(RasterError::Unsupported("an image with no pixels".into()));
        }
        // Mipmapped, for FilterQuality.medium, the framework's default for
        // images: a third again in memory.
        let mut image = ctx.create_mipmapped_image(size, PixelFormat::Rgba8Unorm)?;
        let Some(pixels) = data.take_pixels() else {
            ctx.destroy_image(image);
            return Err(RasterError::Unsupported(
                "an image whose pixels were released without an upload".into(),
            ));
        };
        if let Err(e) = ctx.write_image(&mut image, &pixels) {
            ctx.destroy_image(image);
            return Err(e.into());
        }
        let bytes = pixels.len() as u64 * 4 / 3;
        self.bytes += bytes;
        self.map.insert(
            k,
            Entry {
                data: Arc::downgrade(data),
                image,
                size,
                bytes,
            },
        );
        Ok(())
    }

    /// The device image for `data`, and its size, if uploaded.
    pub(crate) fn get(&self, data: &Arc<ImageData>) -> Option<(&Image, Extent2D)> {
        self.map
            .get(&key(data))
            .filter(|e| e.data.strong_count() > 0)
            .map(|e| (&e.image, e.size))
    }

    /// Destroys the images nothing refers to any more.
    pub(crate) fn sweep(&mut self, ctx: &mut Context) {
        let dead: Vec<usize> = self
            .map
            .iter()
            .filter(|(_, e)| e.data.strong_count() == 0)
            .map(|(k, _)| *k)
            .collect();
        for k in dead {
            self.remove(ctx, k);
        }
    }

    fn remove(&mut self, ctx: &mut Context, k: usize) {
        if let Some(e) = self.map.remove(&k) {
            self.bytes -= e.bytes;
            ctx.destroy_image(e.image);
        }
    }

    /// Destroys every image, for when the device goes.
    pub(crate) fn clear(&mut self, ctx: &mut Context) {
        let keys: Vec<usize> = self.map.keys().copied().collect();
        for k in keys {
            self.remove(ctx, k);
        }
    }
}

/// One frame's image table: the images its paints name, by slot, in the
/// order `draw_with_images` takes them.
pub(crate) struct Slots<'a> {
    gpu: &'a GpuImages,
    pub(crate) table: Vec<&'a Image>,
    index: HashMap<usize, u32>,
    /// What glyph draws do this replay.
    pub(crate) glyphs: GlyphPass<'a>,
}

/// A replay's glyphs: none (a frame with no text draws nothing for them),
/// rasterized into the atlas, or drawn from it in its slot.
pub(crate) enum GlyphPass<'a> {
    None,
    Prepare(&'a mut crate::glyphs::Glyphs),
    Draw {
        glyphs: &'a crate::glyphs::Glyphs,
        slot: u32,
    },
}

impl<'a> Slots<'a> {
    pub(crate) fn new(gpu: &'a GpuImages) -> Slots<'a> {
        Slots {
            gpu,
            table: Vec::new(),
            index: HashMap::new(),
            glyphs: GlyphPass::None,
        }
    }

    /// For the replay that rasterizes the frame's glyphs.
    pub(crate) fn preparing(
        gpu: &'a GpuImages,
        glyphs: &'a mut crate::glyphs::Glyphs,
    ) -> Slots<'a> {
        Slots {
            glyphs: GlyphPass::Prepare(glyphs),
            ..Slots::new(gpu)
        }
    }

    /// For the replay that draws: the atlas, if uploaded, in slot 0.
    pub(crate) fn drawing(gpu: &'a GpuImages, glyphs: &'a crate::glyphs::Glyphs) -> Slots<'a> {
        let mut s = Slots::new(gpu);
        if let Some(image) = glyphs.image() {
            s.table.push(image);
            s.glyphs = GlyphPass::Draw { glyphs, slot: 0 };
        }
        s
    }

    /// The slot `data` is drawn from this frame, and its size in texels.
    pub(crate) fn slot(&mut self, data: &Arc<ImageData>) -> Result<(u32, Extent2D), RasterError> {
        let (image, size) = self
            .gpu
            .get(data)
            .ok_or_else(|| RasterError::Unsupported("an image not uploaded".into()))?;
        let k = key(data);
        if let Some(s) = self.index.get(&k) {
            return Ok((*s, size));
        }
        let s = u32::try_from(self.table.len())
            .map_err(|_| RasterError::Unsupported("too many images in one frame".into()))?;
        self.table.push(image);
        self.index.insert(k, s);
        Ok((s, size))
    }
}
