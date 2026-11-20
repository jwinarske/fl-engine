//! A display as the raster thread's target (brief M4 D9): emblema's
//! `DrmScanoutTarget` over an output the platform implements.
//!
//! emblema renders into a ring of buffers it allocates and exports; the
//! output imports each as a framebuffer, commits the one a frame was drawn
//! into, and says when it reached the screen. Nothing here names
//! KMS or the platform: the output is emblema's [`ScanoutOutput`], the
//! trait it keeps for a KMS stack other than its own.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use emblema::drm::{DrmScanoutTarget, DEFAULT_RING_DEPTH};
use emblema::present::PresentTarget;
use emblema::vulkan::{VulkanHal, VulkanTexture};
use emblema::{Context, Image, PixelFormat};
use fl_ui::Scene;

pub use emblema::drm::{CommitRequest, DmaBufPlanes, FbHandle, Mode, OutputEvent, ScanoutOutput};
pub use emblema::{Error, Extent2D};
pub use emblema_hal::{DmaBufPlane, FormatModifierSet, Fourcc, Modifier};

use crate::{record, Device, RasterError, Target};

/// The display side, made by the platform and used on the raster thread.
pub type Output = Box<dyn ScanoutOutput + Send>;

/// The output as the ring holds it: shared, since the ring is made again
/// over the same output when the output says everything is to be rebuilt,
/// and noting when it says so.
struct Shared {
    output: Arc<Mutex<Output>>,
    /// The output's formats when this was made: with the ring, so as the
    /// card stands.
    formats: Vec<FormatModifierSet>,
    /// The output's card as it was then, its own descriptor: the lock's
    /// guard cannot lend the output's.
    card: Option<std::os::fd::OwnedFd>,
    reconfigured: Arc<AtomicBool>,
}

impl Shared {
    fn new(output: &Arc<Mutex<Output>>, reconfigured: &Arc<AtomicBool>) -> Shared {
        let (formats, card) = {
            let output = lock(output);
            let card = output
                .device_fd()
                .and_then(|fd| fd.try_clone_to_owned().ok());
            (output.supported_formats().to_vec(), card)
        };
        Shared {
            output: Arc::clone(output),
            formats,
            card,
            reconfigured: Arc::clone(reconfigured),
        }
    }

    fn note(&self, events: &[OutputEvent]) {
        if events.contains(&OutputEvent::Reconfigured) {
            self.reconfigured.store(true, Ordering::Release);
        }
    }
}

fn lock(output: &Mutex<Output>) -> MutexGuard<'_, Output> {
    output.lock().unwrap_or_else(|e| e.into_inner())
}

impl ScanoutOutput for Shared {
    fn mode(&self) -> Mode {
        lock(&self.output).mode()
    }

    fn modes(&self) -> Vec<Mode> {
        lock(&self.output).modes()
    }

    fn set_mode(&mut self, mode: Mode) -> emblema::Result<()> {
        lock(&self.output).set_mode(mode)
    }

    fn supported_formats(&self) -> &[FormatModifierSet] {
        &self.formats
    }

    fn device_fd(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        self.card.as_ref().map(std::os::fd::AsFd::as_fd)
    }

    fn import_dmabuf(&mut self, buffer: DmaBufPlanes) -> emblema::Result<FbHandle> {
        lock(&self.output).import_dmabuf(buffer)
    }

    fn release_framebuffer(&mut self, fb: FbHandle) {
        lock(&self.output).release_framebuffer(fb);
    }

    fn commit(&mut self, request: CommitRequest) -> emblema::Result<()> {
        let mut output = lock(&self.output);
        let done = output.commit(request);
        if done.is_err() {
            // Refused for being of a card since rebuilt, perhaps: what it
            // has to say is heard before the error is acted on.
            self.note(&output.poll_events());
        }
        done
    }

    fn poll_events(&mut self) -> Vec<OutputEvent> {
        let events = lock(&self.output).poll_events();
        self.note(&events);
        events
    }

    fn wait_for_event(&mut self, timeout_nanos: u64) -> emblema::Result<Vec<OutputEvent>> {
        let events = lock(&self.output).wait_for_event(timeout_nanos)?;
        self.note(&events);
        Ok(events)
    }
}

/// The raster thread's device with a display for its target.
pub(crate) struct Scanout {
    device: Device,
    target: Option<DrmScanoutTarget<VulkanHal, Shared>>,
    output: Arc<Mutex<Output>>,
    reconfigured: Arc<AtomicBool>,
}

impl Scanout {
    /// A ring of the output's mode's size, in a format both sides take.
    pub(crate) fn new(
        output: Output,
        pipelines: Option<std::path::PathBuf>,
        memory: Option<crate::memory::Kept>,
    ) -> Result<Scanout, RasterError> {
        let extent = output.mode().extent;
        let mut device = Device::new(extent.width, extent.height, memory)?;
        if let Some(path) = pipelines {
            device.keep_pipelines(path);
        }
        let mut scanout = Scanout {
            device,
            target: None,
            output: Arc::new(Mutex::new(output)),
            reconfigured: Arc::default(),
        };
        scanout.build()?;
        Ok(scanout)
    }

    /// Makes the ring, over the output as it stands, the old one's
    /// buffers gone first. The device's size is the ring's.
    fn build(&mut self) -> Result<(), RasterError> {
        let vk = match &mut self.device.ctx {
            Context::Vulkan(vk) => vk,
            #[allow(unreachable_patterns)]
            _ => return Err(RasterError::Unsupported("scanout without Vulkan".into())),
        };
        if let Some(old) = self.target.take() {
            old.destroy(vk);
        }
        self.reconfigured.store(false, Ordering::Release);
        // The renderer's own buffers, handed to the display; and where the
        // display takes none of those (a display controller apart from the
        // GPU, which imports nothing it did not allocate), the display's,
        // allocated by GBM on its card and drawn into by the renderer.
        let shared = Shared::new(&self.output, &self.reconfigured);
        let target = match DrmScanoutTarget::new(vk, shared, DEFAULT_RING_DEPTH) {
            Ok(target) => target,
            Err(own) => {
                let shared = Shared::new(&self.output, &self.reconfigured);
                DrmScanoutTarget::with_gbm(vk, shared, DEFAULT_RING_DEPTH).map_err(|gbm| {
                    RasterError::Unsupported(format!(
                        "no buffers for the display: the renderer's ({own}), nor the card's through GBM ({gbm})"
                    ))
                })?
            }
        };
        let extent = target.extent();
        self.device.width = extent.width;
        self.device.height = extent.height;
        self.target = Some(target);
        Ok(())
    }

    /// Draws `scene` into a free buffer of the ring and commits it. Waits
    /// for the flip before, if one is pending: the display takes one
    /// commit at a time, and that wait is what paces the thread.
    fn present(&mut self, scene: &Scene) -> Result<(), RasterError> {
        let Device {
            ctx,
            images,
            glyphs,
            width,
            height,
            stages,
            compiled: _,
            ..
        } = &mut self.device;
        let target = self
            .target
            .as_mut()
            .ok_or_else(|| RasterError::Unsupported("a destroyed target".into()))?;
        let begun = Instant::now();
        // The frame before may still be drawing: it is waited for only if
        // this one destroys or writes an image it may be drawn from.
        let (recording, slots) =
            record(ctx, images, glyphs, (*width, *height), scene, &mut || {
                Ok(target.wait_for_rendering()?)
            })?;
        stages.record.add(begun);
        let vk = match ctx {
            Context::Vulkan(vk) => vk,
            #[allow(unreachable_patterns)]
            _ => return Err(RasterError::Unsupported("scanout without Vulkan".into())),
        };
        let textures = slots
            .table
            .iter()
            .map(|image| match image {
                Image::Vulkan(texture) => Ok(texture),
                #[allow(unreachable_patterns)]
                _ => Err(RasterError::Unsupported(
                    "an image of another backend".into(),
                )),
            })
            .collect::<Result<Vec<&VulkanTexture>, RasterError>>()?;
        // Handed to the GPU and not waited for: the frame's fence goes with
        // its commit, for the kernel to wait on before the flip, and the
        // next frame is recorded while this one is drawn. Where the card or
        // the driver takes no fence, emblema waits before it commits.
        let begun = Instant::now();
        target.acquire(vk)?;
        stages.wait.add(begun);
        let begun = Instant::now();
        target.submit_recording(vk, &recording, &textures)?;
        stages.draw.add(begun);
        let begun = Instant::now();
        target.present(vk)?;
        stages.show.add(begun);
        Ok(())
    }
}

impl Target for Scanout {
    fn draw(&mut self, scene: &Scene) -> Result<Option<Vec<u8>>, RasterError> {
        let mut drawn = self.present(scene);
        // The output's buffers went, with the card, while the session was
        // switched away: the ring is made again and the frame drawn into it.
        if drawn.is_err() && self.reconfigured.load(Ordering::Acquire) {
            self.build()?;
            drawn = self.present(scene);
        }
        drawn.map(|()| None)
    }

    fn device(&mut self) -> &mut Device {
        &mut self.device
    }

    fn format(&self) -> PixelFormat {
        self.target
            .as_ref()
            .map_or(PixelFormat::Bgra8Unorm, PresentTarget::format)
    }

    fn loses_its_frame(&self) -> bool {
        true
    }

    fn fence_waits(&self) -> u64 {
        self.target.as_ref().map_or(0, DrmScanoutTarget::cpu_waits)
    }

    fn settle(&mut self) {
        // A wait that fails is the next frame's to report: it submits to the
        // same ring.
        if let Some(target) = &self.target {
            let _ = target.wait_for_rendering();
        }
    }
}

impl Drop for Scanout {
    /// The ring goes before the images, and both before their context.
    fn drop(&mut self) {
        // `if let` with one backend built in is irrefutable, and with two
        // is not: a match holds for both.
        #[allow(clippy::single_match)]
        match (self.target.take(), &mut self.device.ctx) {
            (Some(target), Context::Vulkan(vk)) => target.destroy(vk),
            #[allow(unreachable_patterns)]
            _ => {}
        }
        self.device.clear();
    }
}
