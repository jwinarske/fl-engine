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
    /// External textures' images, by texture id, gone when the texture is
    /// unregistered (`fl_ui::texture`): a pixel buffer's newest frame, one
    /// image written in place while the size holds; a dma-buf's buffers,
    /// each imported once.
    textures: HashMap<i64, Texture>,
    bytes: u64,
    _raster_thread: PhantomData<*const ()>,
}

enum Texture {
    Written {
        image: Image,
        size: Extent2D,
        bytes: u64,
    },
    /// The buffers a dma-buf producer has handed over, the newest -- what is
    /// drawn -- last; at most [`MAX_IMPORTS`]. Not counted in
    /// [`GpuImages::bytes`]: the memory is the producer's.
    Imported {
        imports: Vec<Import>,
        /// What is drawn of the newest, from its top left corner.
        visible: Extent2D,
    },
}

struct Import {
    key: fl_ui::texture::BufferKey,
    image: Image,
    size: Extent2D,
}

/// The buffers of one dma-buf texture kept imported: what a producer
/// cycles through, a swapchain's three and one spare. Past that the least
/// recently shown goes.
const MAX_IMPORTS: usize = 4;

impl Texture {
    /// The image drawn, its size, and what is drawn of it.
    fn shown(&self) -> Option<(&Image, Extent2D, Extent2D)> {
        match self {
            Texture::Written { image, size, .. } => Some((image, *size, *size)),
            Texture::Imported { imports, visible } => {
                imports.last().map(|i| (&i.image, i.size, *visible))
            }
        }
    }
}

fn key(data: &Arc<ImageData>) -> usize {
    Arc::as_ptr(data) as usize
}

impl Default for GpuImages {
    fn default() -> Self {
        GpuImages {
            map: HashMap::new(),
            textures: HashMap::new(),
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

    /// Whether [`GpuImages::sweep`] has anything to destroy.
    pub(crate) fn has_dead(&self) -> bool {
        self.map.values().any(|e| e.data.strong_count() == 0)
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
        let ids: Vec<i64> = self.textures.keys().copied().collect();
        for id in ids {
            self.remove_texture(ctx, id);
        }
    }

    /// External textures on the device.
    pub fn textures(&self) -> usize {
        self.textures.len()
    }

    /// Writes texture `id`'s frame, premultiplied RGBA8: into its image
    /// where the size is the one it has, else into a new one. Whatever was
    /// drawing from the old image must be done with it first.
    pub(crate) fn write_texture(
        &mut self,
        ctx: &mut Context,
        id: i64,
        size: Extent2D,
        rgba: &[u8],
    ) -> Result<(), RasterError> {
        if self
            .textures
            .get(&id)
            .is_some_and(|t| !matches!(t, Texture::Written { size: s, .. } if *s == size))
        {
            self.remove_texture(ctx, id);
        }
        if let Some(Texture::Written { image, .. }) = self.textures.get_mut(&id) {
            return ctx.write_image(image, rgba).map_err(Into::into);
        }
        let mut image = ctx.create_image(size, PixelFormat::Rgba8Unorm)?;
        if let Err(e) = ctx.write_image(&mut image, rgba) {
            ctx.destroy_image(image);
            return Err(e.into());
        }
        let bytes = rgba.len() as u64;
        self.bytes += bytes;
        self.textures
            .insert(id, Texture::Written { image, size, bytes });
        Ok(())
    }

    /// Shows texture `id`'s dma-buf frame: the buffer's image if it was
    /// imported before, else a new import, the least recently shown going
    /// past [`MAX_IMPORTS`]. `settle` runs before an image frames may still
    /// be drawing from is destroyed, and its failure is this one's. A buffer
    /// the device will not import is the inner error: the texture keeps
    /// what it showed.
    pub(crate) fn show_dma_buf(
        &mut self,
        ctx: &mut Context,
        id: i64,
        frame: fl_ui::texture::DmaBufFrame,
        settle: &mut dyn FnMut() -> Result<(), RasterError>,
    ) -> Result<Result<(), RasterError>, RasterError> {
        if matches!(self.textures.get(&id), Some(Texture::Written { .. })) {
            settle()?;
            self.remove_texture(ctx, id);
        }
        let visible = Extent2D::new(frame.visible_width, frame.visible_height);
        if let Some(Texture::Imported {
            imports,
            visible: v,
        }) = self.textures.get_mut(&id)
        {
            if let Some(i) = imports.iter().position(|i| i.key == frame.key) {
                let again = imports.remove(i);
                imports.push(again);
                *v = visible;
                return Ok(Ok(()));
            }
        }

        let format = match emblema::Fourcc(frame.fourcc) {
            emblema::Fourcc::ARGB8888 => PixelFormat::Bgra8Unorm,
            emblema::Fourcc::ABGR8888 => PixelFormat::Rgba8Unorm,
            other => {
                return Ok(Err(RasterError::Unsupported(format!(
                    "a dma-buf texture in {other}; AR24 and AB24 are drawn"
                ))))
            }
        };
        let size = Extent2D::new(frame.width, frame.height);
        let key = frame.key;
        let desc = emblema::ExternalImageDesc {
            planes: frame
                .planes
                .into_iter()
                .map(|p| emblema::DmaBufPlane {
                    fd: p.fd,
                    offset: p.offset,
                    stride: p.pitch,
                })
                .collect(),
            fourcc: emblema::Fourcc(frame.fourcc),
            modifier: emblema::Modifier(frame.modifier),
        };
        let image = match ctx.import_image(desc, size, format) {
            Ok(image) => image,
            Err(e) => return Ok(Err(e.into())),
        };
        let t = self.textures.entry(id).or_insert(Texture::Imported {
            imports: Vec::new(),
            visible,
        });
        let Texture::Imported {
            imports,
            visible: v,
        } = t
        else {
            unreachable!("a written texture was removed above");
        };
        imports.push(Import { key, image, size });
        *v = visible;
        if imports.len() > MAX_IMPORTS {
            settle()?;
            let oldest = imports.remove(0);
            ctx.destroy_image(oldest.image);
        }
        Ok(Ok(()))
    }

    /// Destroys texture `id`'s images, if it has any.
    pub(crate) fn remove_texture(&mut self, ctx: &mut Context, id: i64) {
        match self.textures.remove(&id) {
            Some(Texture::Written { image, bytes, .. }) => {
                self.bytes -= bytes;
                ctx.destroy_image(image);
            }
            Some(Texture::Imported { imports, .. }) => {
                for i in imports {
                    ctx.destroy_image(i.image);
                }
            }
            None => {}
        }
    }

    /// Whether texture `id` has a frame on the device.
    pub(crate) fn has_texture(&self, id: i64) -> bool {
        self.textures.contains_key(&id)
    }
}

/// One frame's image table: the images its paints name, by slot, in the
/// order `draw_with_images` takes them.
pub(crate) struct Slots<'a> {
    gpu: &'a GpuImages,
    pub(crate) table: Vec<&'a Image>,
    index: HashMap<usize, u32>,
    textures: HashMap<i64, u32>,
    /// What glyph draws do this replay.
    pub(crate) glyphs: GlyphPass<'a>,
}

/// A replay's glyphs: none (a frame with no text draws nothing for them),
/// rasterized into the atlas, or drawn from its pages, page `i` in slot
/// `first + i`.
pub(crate) enum GlyphPass<'a> {
    None,
    Prepare(&'a mut crate::glyphs::Glyphs),
    Draw {
        glyphs: &'a crate::glyphs::Glyphs,
        first: u32,
    },
}

impl<'a> Slots<'a> {
    pub(crate) fn new(gpu: &'a GpuImages) -> Slots<'a> {
        Slots {
            gpu,
            table: Vec::new(),
            index: HashMap::new(),
            textures: HashMap::new(),
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

    /// For the replay that draws: the atlas's pages, if uploaded, from slot
    /// 0, the cache first.
    pub(crate) fn drawing(gpu: &'a GpuImages, glyphs: &'a crate::glyphs::Glyphs) -> Slots<'a> {
        let mut s = Slots::new(gpu);
        let pages = glyphs.images();
        if !pages.is_empty() {
            s.table.extend(pages);
            s.glyphs = GlyphPass::Draw { glyphs, first: 0 };
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

    /// The slot texture `id`'s frame is drawn from this frame, the image's
    /// size, and what is drawn of it from its top left corner; or `None` if
    /// it has none on the device.
    pub(crate) fn texture_slot(
        &mut self,
        id: i64,
    ) -> Result<Option<(u32, Extent2D, Extent2D)>, RasterError> {
        let Some((image, size, visible)) = self.gpu.textures.get(&id).and_then(Texture::shown)
        else {
            return Ok(None);
        };
        if let Some(s) = self.textures.get(&id) {
            return Ok(Some((*s, size, visible)));
        }
        let s = u32::try_from(self.table.len())
            .map_err(|_| RasterError::Unsupported("too many images in one frame".into()))?;
        self.table.push(image);
        self.textures.insert(id, s);
        Ok(Some((s, size, visible)))
    }
}
