//! The display (brief M4 D9): emblema's `ScanoutOutput` over drmkit.
//!
//! emblema renders and keeps the ring of buffers; this imports each as a
//! framebuffer, commits the one a frame was drawn into, and reports each
//! flip with the sequence and time the kernel gave it. The card comes from
//! the seat, so a switch to another terminal takes it: frames committed
//! meanwhile are dropped, and on the switch back the card is taken again
//! and the output says everything is to be rebuilt, since every object made
//! on the old descriptor went with it.

use std::collections::HashMap;
use std::os::fd::{AsFd as _, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use drm::control::Device as _;
use drmkit_core::{AtomicCommitFlags, AtomicRequest, CoreError, Device, ObjectType, PropertyStore};
use drmkit_display::{query_connector_modes, Probe, ScanoutTarget};
use drmkit_modeset::{PageFlip, PageFlipEvent, Timeout};
use emblema_hal::{Error, Extent2D, FormatModifierSet, Fourcc, Modifier};
use emblema_present_drm::{
    CommitRequest, DmaBufPlanes, FbHandle, Mode, OutputEvent, ScanoutOutput,
};

use crate::seat::{Activity, Seat, SeatError};

/// Why there is no display.
#[derive(Debug)]
pub enum DisplayError {
    Seat(SeatError),
    /// No card under `/dev/dri` has a connector with a display on it, or
    /// the one named has none.
    NoDisplay(String),
    /// The card refused something the display needs.
    Card(String),
}

impl std::fmt::Display for DisplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisplayError::Seat(e) => e.fmt(f),
            DisplayError::NoDisplay(what) => write!(f, "no display: {what}"),
            DisplayError::Card(what) => write!(f, "display: {what}"),
        }
    }
}

impl std::error::Error for DisplayError {}

impl From<SeatError> for DisplayError {
    fn from(e: SeatError) -> Self {
        DisplayError::Seat(e)
    }
}

/// A flip that reached the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flip {
    /// The vertical blank it happened in, as the kernel counts them.
    pub sequence: u64,
    /// When, on `Instant`'s clock.
    pub at: Instant,
    /// The mode's vertical blanks are this far apart.
    pub period: Duration,
}

/// What a display did, read from any thread.
#[derive(Debug, Default)]
pub struct Pacing {
    flips: AtomicU64,
    first: AtomicU64,
    last: AtomicU64,
    dropped: AtomicU64,
}

impl Pacing {
    /// Frames that reached the screen.
    pub fn flips(&self) -> u64 {
        self.flips.load(Ordering::Relaxed)
    }

    /// Vertical blanks from the first flip to the last. One less than
    /// [`Pacing::flips`] when a frame was shown at every blank between.
    pub fn vblanks(&self) -> u64 {
        self.last
            .load(Ordering::Relaxed)
            .saturating_sub(self.first.load(Ordering::Relaxed))
    }

    /// Frames drawn while the session was switched away, and not shown.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn observe(&self, sequence: u64) {
        if self.flips.fetch_add(1, Ordering::Relaxed) == 0 {
            self.first.store(sequence, Ordering::Relaxed);
        }
        self.last.store(sequence, Ordering::Relaxed);
    }
}

/// A mode asked for by its size, and its rate if that matters: one of the
/// display's own, the nearest in rate, the fastest with none said.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WantedMode {
    pub width: u32,
    pub height: u32,
    pub hz: Option<f64>,
}

/// Of `modes`, each its width, height and rate in millihertz, which one
/// `wanted` asks for: its size, and the rate nearest the one said, or the
/// highest. `None` if no mode is that size.
fn pick_mode(modes: &[(u32, u32, u32)], wanted: WantedMode) -> Option<usize> {
    let off = |mhz: u32| {
        let hz = f64::from(mhz) / 1000.0;
        match wanted.hz {
            Some(said) if said.is_finite() => (hz - said).abs(),
            _ => -hz,
        }
    };
    modes
        .iter()
        .enumerate()
        .filter(|(_, (w, h, _))| *w == wanted.width && *h == wanted.height)
        .min_by(|(_, a), (_, b)| off(a.2).total_cmp(&off(b.2)))
        .map(|(i, _)| i)
}

/// One connector's pipeline on an open card: what a commit names.
struct Pipeline {
    device: Device,
    connector: u32,
    crtc: u32,
    crtc_index: u32,
    plane: u32,
    mode: drm::control::Mode,
    mode_blob: u64,
    props: Props,
    formats: Vec<FormatModifierSet>,
    flips: PageFlip,
}

/// The ids of the properties a commit writes.
struct Props {
    connector_crtc_id: u32,
    crtc_mode_id: u32,
    crtc_active: u32,
    fb_id: u32,
    plane_crtc_id: u32,
    src: [u32; 4],
    dst: [u32; 4],
    in_fence_fd: Option<u32>,
}

fn card(what: &str, e: impl std::fmt::Display) -> DisplayError {
    DisplayError::Card(format!("{what}: {e}"))
}

fn backend(what: &str, e: impl std::fmt::Display) -> Error {
    Error::Backend {
        backend: "kms",
        detail: format!("{what}: {e}"),
    }
}

impl Pipeline {
    /// The pipeline of `connector` (`HDMI-A-1`), or of the first connector
    /// with a display, on the card `fd` is. `Ok(None)` if this card has no
    /// such connector: another may.
    fn open(
        fd: OwnedFd,
        connector: Option<&str>,
        wanted: Option<WantedMode>,
        events: &Arc<Mutex<Vec<PageFlipEvent>>>,
    ) -> Result<Option<Pipeline>, DisplayError> {
        let device = Device::from_owned_fd(fd);
        // A card that renders and does not display (a Pi's `v3d`) has
        // neither, and is not the card looked for.
        if device.enable_universal_planes().is_err() || device.enable_atomic().is_err() {
            return Ok(None);
        }
        let found = match connector {
            Some(name) => {
                let all = query_connector_modes(&device, Probe::Force)
                    .map_err(|e| card("connectors", e))?;
                let Some(c) = all.iter().find(|c| c.name() == name) else {
                    return Ok(None);
                };
                ScanoutTarget::discover_connector(&device, c.connector_id)
            }
            None => ScanoutTarget::discover(&device),
        }
        .map_err(|e| card("discovery", e))?;
        let mut target = match found {
            Ok(t) => t,
            // Named, and here, but not to be driven: say why.
            Err(e) if connector.is_some() => return Err(DisplayError::NoDisplay(e.to_string())),
            Err(_) => return Ok(None),
        };
        // Its preferred mode, unless another of its own is asked for.
        if let Some(wanted) = wanted {
            let all =
                query_connector_modes(&device, Probe::Cached).map_err(|e| card("connectors", e))?;
            let modes = all
                .iter()
                .find(|c| c.connector_id == target.connector_id)
                .map_or(&[][..], |c| &c.modes[..]);
            let sized: Vec<(u32, u32, u32)> = modes
                .iter()
                .map(|m| (u32::from(m.size().0), u32::from(m.size().1), refresh_mhz(m)))
                .collect();
            target.mode = pick_mode(&sized, wanted).map(|i| modes[i]).ok_or_else(|| {
                let mut sizes: Vec<String> = Vec::new();
                for m in modes {
                    let size = format!("{}x{}", m.size().0, m.size().1);
                    if !sizes.contains(&size) {
                        sizes.push(size);
                    }
                }
                DisplayError::NoDisplay(format!(
                    "the display has no mode {}x{}; it has {}",
                    wanted.width,
                    wanted.height,
                    sizes.join(", ")
                ))
            })?;
        }
        let (connector, crtc, plane) =
            (target.connector_id, target.crtc.id, target.primary_plane_id);
        let mut store = PropertyStore::new();
        for (object, kind) in [
            (connector, ObjectType::Connector),
            (crtc, ObjectType::Crtc),
            (plane, ObjectType::Plane),
        ] {
            store
                .cache_properties(&device, object, kind)
                .map_err(|e| card("properties", e))?;
        }
        let id = |object: u32, name: &str| {
            store
                .property_id(object, name)
                .map_err(|e| card("properties", e))
        };
        let props = Props {
            connector_crtc_id: id(connector, "CRTC_ID")?,
            crtc_mode_id: id(crtc, "MODE_ID")?,
            crtc_active: id(crtc, "ACTIVE")?,
            fb_id: id(plane, "FB_ID")?,
            plane_crtc_id: id(plane, "CRTC_ID")?,
            src: [
                id(plane, "SRC_X")?,
                id(plane, "SRC_Y")?,
                id(plane, "SRC_W")?,
                id(plane, "SRC_H")?,
            ],
            dst: [
                id(plane, "CRTC_X")?,
                id(plane, "CRTC_Y")?,
                id(plane, "CRTC_W")?,
                id(plane, "CRTC_H")?,
            ],
            in_fence_fd: store.property_id(plane, "IN_FENCE_FD").ok(),
        };
        // Its id alone is kept: the blob is the card's, and goes with the
        // descriptor.
        let blob = device
            .create_property_blob(&target.mode)
            .map_err(|e| card("mode blob", e))?;
        let mode_blob = blob.id();
        blob.forget();
        let mut by_format: Vec<FormatModifierSet> = Vec::new();
        for f in target.primary_formats.all() {
            let (fourcc, modifier) = (Fourcc(f.fourcc), Modifier(f.modifier.0));
            match by_format.iter_mut().find(|s| s.fourcc == fourcc) {
                Some(set) => set.modifiers.push(modifier),
                None => by_format.push(FormatModifierSet::new(fourcc, vec![modifier])),
            }
        }
        let mut flips = PageFlip::new(&device).map_err(|e| card("flip events", e))?;
        let queue = Arc::clone(events);
        flips.set_handler(Box::new(move |event| {
            queue.lock().unwrap_or_else(|e| e.into_inner()).push(event);
        }));
        Ok(Some(Pipeline {
            device,
            connector,
            crtc,
            crtc_index: target.crtc.index,
            plane,
            mode: target.mode,
            mode_blob,
            props,
            formats: by_format,
            flips,
        }))
    }

    fn size(&self) -> (u32, u32) {
        let (w, h) = self.mode.size();
        (u32::from(w), u32::from(h))
    }
}

/// `mode`'s refresh rate in millihertz, from its timings: `vrefresh` is
/// rounded to a hertz, and 59.94 is not 60.
fn refresh_mhz(mode: &drm::control::Mode) -> u32 {
    millihertz(
        mode.clock(),
        mode.hsync().2,
        mode.vsync().2,
        mode.vrefresh(),
    )
}

/// A pixel clock in kHz over a frame of `htotal` by `vtotal`, in
/// millihertz; `rounded` hertz, as the kernel has it, for timings that say
/// nothing.
fn millihertz(clock: u32, htotal: u16, vtotal: u16, rounded: u32) -> u32 {
    let frame = u64::from(htotal) * u64::from(vtotal);
    if clock == 0 || frame == 0 {
        return rounded.saturating_mul(1000);
    }
    u32::try_from(u64::from(clock) * 1_000_000 / frame).unwrap_or(u32::MAX)
}

/// A framebuffer and the buffer handles it was made from.
struct Framebuffer {
    fb: drm::control::framebuffer::Handle,
    buffers: Vec<drm::buffer::Handle>,
}

/// A connector of a card held through the seat, as emblema's output.
pub struct Display {
    seat: Arc<Seat>,
    activity: Arc<Activity>,
    path: PathBuf,
    connector: Option<String>,
    /// The mode asked for, for the card opened again after a switch.
    wanted: Option<WantedMode>,
    pipeline: Pipeline,
    framebuffers: HashMap<u64, Framebuffer>,
    in_flight: Option<FbHandle>,
    /// Set when the card went with the session: the session's count of
    /// resumes then. The card is taken again once it has come back since.
    lost: Option<u64>,
    /// The session's count of its comings back when the card was last
    /// taken: one since says the card went and came back with nobody
    /// looking, as under an app that draws nothing meanwhile.
    taken_at: u64,
    /// Whether the next commit has to set the mode whatever it asks.
    modeset: bool,
    events: Arc<Mutex<Vec<PageFlipEvent>>>,
    pending: Vec<OutputEvent>,
    on_flip: Option<Box<dyn Fn(Flip) + Send>>,
    pacing: Arc<Pacing>,
}

impl Display {
    /// The first card under `/dev/dri` with a display on `connector`
    /// (`HDMI-A-1`), or on any connector, opened through `seat`.
    /// At `mode`, one of the display's own, or at its preferred mode.
    pub fn open(
        seat: &Arc<Seat>,
        connector: Option<&str>,
        mode: Option<WantedMode>,
    ) -> Result<Display, DisplayError> {
        let mut cards: Vec<PathBuf> = std::fs::read_dir("/dev/dri")
            .map_err(|e| DisplayError::NoDisplay(format!("/dev/dri: {e}")))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("card"))
            })
            .collect();
        cards.sort();
        let events = Arc::new(Mutex::new(Vec::new()));
        for path in cards {
            // A render-only card has no connectors, and is not an error.
            let Ok(fd) = seat.take_device(&path) else {
                continue;
            };
            if let Some(pipeline) = Pipeline::open(fd, connector, mode, &events)? {
                return Ok(Display {
                    seat: Arc::clone(seat),
                    activity: seat.activity(),
                    path,
                    connector: connector.map(str::to_owned),
                    wanted: mode,
                    pipeline,
                    framebuffers: HashMap::new(),
                    in_flight: None,
                    lost: None,
                    taken_at: seat.activity().resumes(),
                    modeset: false,
                    events,
                    pending: Vec::new(),
                    on_flip: None,
                    pacing: Arc::new(Pacing::default()),
                });
            }
        }
        Err(DisplayError::NoDisplay(match connector {
            Some(name) => format!("no card has a connector {name}"),
            None => "no connector of any card has a display with a mode".into(),
        }))
    }

    /// The card's node.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The mode's size in pixels.
    pub fn size(&self) -> (u32, u32) {
        self.pipeline.size()
    }

    /// The mode's refresh rate in hertz.
    pub fn refresh_hz(&self) -> f64 {
        f64::from(refresh_mhz(&self.pipeline.mode)) / 1000.0
    }

    /// Where a cursor for this display goes ([`crate::Cursor::open`]).
    pub fn cursor_target(&self) -> crate::CursorTarget {
        crate::CursorTarget {
            path: self.path.clone(),
            crtc_id: self.pipeline.crtc,
            crtc_index: self.pipeline.crtc_index,
        }
    }

    /// What the display does, to read once it has been handed over.
    pub fn pacing(&self) -> Arc<Pacing> {
        Arc::clone(&self.pacing)
    }

    /// Calls `f` at each flip, from the thread that drives the display:
    /// the vsync a frame loop keeps time by.
    pub fn on_flip(&mut self, f: impl Fn(Flip) + Send + 'static) {
        self.on_flip = Some(Box::new(f));
    }

    /// The card is another session's from now, until this one is back.
    fn lose(&mut self) {
        if self.lost.is_none() {
            self.lost = Some(self.activity.resumes());
        }
        // Its flip is not coming.
        if let Some(fb) = self.in_flight.take() {
            self.pending.push(OutputEvent::FlipComplete { fb });
        }
    }

    /// Flips that have arrived become events; a session's coming and
    /// going is noticed.
    fn drain(&mut self) {
        if !self.activity.is_active() {
            self.lose();
            return;
        }
        // Away and back between two looks: the card held is the old one.
        if self.lost.is_none() && self.activity.resumes() != self.taken_at {
            self.lost = Some(self.taken_at);
            if let Some(fb) = self.in_flight.take() {
                self.pending.push(OutputEvent::FlipComplete { fb });
            }
        }
        if let Some(at) = self.lost {
            if self.activity.resumes() != at {
                self.retake();
            }
            return;
        }
        let _ = self.pipeline.flips.dispatch(Timeout::Poll);
        self.take_flips();
    }

    fn take_flips(&mut self) {
        let arrived = std::mem::take(&mut *self.events.lock().unwrap_or_else(|e| e.into_inner()));
        for event in arrived {
            if event.crtc_id != self.pipeline.crtc {
                continue;
            }
            self.pacing.observe(event.sequence);
            if let Some(f) = &self.on_flip {
                f(Flip {
                    sequence: event.sequence,
                    at: instant_of(event.timestamp.as_nanos()),
                    period: self.period(),
                });
            }
            if let Some(fb) = self.in_flight.take() {
                self.pending.push(OutputEvent::FlipComplete { fb });
            }
        }
    }

    fn period(&self) -> Duration {
        Duration::from_nanos(self.mode().frame_nanos())
    }

    /// The session is back: the card again, on the descriptor the seat has
    /// for it now. Nothing made on the old one is left.
    fn retake(&mut self) {
        let now = self.activity.resumes();
        let again = self
            .seat
            .take_device(&self.path)
            .map_err(DisplayError::from)
            .and_then(|fd| {
                Pipeline::open(fd, self.connector.as_deref(), self.wanted, &self.events)
            });
        // Not back yet, or its display gone meanwhile: tried again at the
        // next frame.
        let Ok(Some(pipeline)) = again else {
            return;
        };
        self.taken_at = now;
        self.pipeline = pipeline;
        self.framebuffers.clear();
        self.in_flight = None;
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.lost = None;
        self.modeset = true;
        self.pending.push(OutputEvent::Reconfigured);
    }
}

/// A kernel timestamp, nanoseconds of the monotonic clock, as an `Instant`,
/// which is that clock's too.
fn instant_of(nanos: u64) -> Instant {
    let now = Instant::now();
    let t = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let now_nanos = u64::try_from(t.tv_sec)
        .unwrap_or(0)
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::try_from(t.tv_nsec).unwrap_or(0));
    now.checked_sub(Duration::from_nanos(now_nanos.saturating_sub(nanos)))
        .unwrap_or(now)
}

struct Imported {
    extent: Extent2D,
    fourcc: Fourcc,
    modifier: Modifier,
    pitches: [u32; 4],
    offsets: [u32; 4],
    handles: [Option<drm::buffer::Handle>; 4],
}

impl drm::buffer::PlanarBuffer for Imported {
    fn size(&self) -> (u32, u32) {
        (self.extent.width, self.extent.height)
    }

    fn format(&self) -> drm::buffer::DrmFourcc {
        // Checked before one of these is made.
        drm::buffer::DrmFourcc::try_from(self.fourcc.0).unwrap_or(drm::buffer::DrmFourcc::Xrgb8888)
    }

    fn modifier(&self) -> Option<drm::buffer::DrmModifier> {
        Some(drm::buffer::DrmModifier::from(self.modifier.0))
    }

    fn pitches(&self) -> [u32; 4] {
        self.pitches
    }

    fn handles(&self) -> [Option<drm::buffer::Handle>; 4] {
        self.handles
    }

    fn offsets(&self) -> [u32; 4] {
        self.offsets
    }
}

impl ScanoutOutput for Display {
    fn mode(&self) -> Mode {
        let (width, height) = self.pipeline.size();
        Mode {
            extent: Extent2D::new(width, height),
            refresh_mhz: refresh_mhz(&self.pipeline.mode),
        }
    }

    fn supported_formats(&self) -> &[FormatModifierSet] {
        &self.pipeline.formats
    }

    fn device_fd(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        Some(self.pipeline.device.as_fd())
    }

    fn import_dmabuf(&mut self, buffer: DmaBufPlanes) -> emblema_hal::Result<FbHandle> {
        if buffer.planes.is_empty() || buffer.planes.len() > 4 {
            return Err(Error::Unsupported(
                "a buffer of no planes, or more than four",
            ));
        }
        if drm::buffer::DrmFourcc::try_from(buffer.fourcc.0).is_err() {
            return Err(Error::Unsupported("a format KMS has no name for"));
        }
        let device = &self.pipeline.device;
        let mut buffers = Vec::new();
        let (mut pitches, mut offsets, mut handles) = ([0u32; 4], [0u32; 4], [None; 4]);
        let close = |buffers: &[drm::buffer::Handle]| {
            for b in buffers {
                let _ = device.close_buffer(*b);
            }
        };
        for (i, plane) in buffer.planes.iter().enumerate() {
            let handle = match device.prime_fd_to_buffer(plane.fd.as_fd()) {
                Ok(h) => h,
                Err(e) => {
                    close(&buffers);
                    return Err(backend("importing a buffer", e));
                }
            };
            // One buffer's planes are often one handle; it is closed once.
            if !buffers.contains(&handle) {
                buffers.push(handle);
            }
            handles[i] = Some(handle);
            pitches[i] = plane.stride;
            offsets[i] = plane.offset;
        }
        let planar = Imported {
            extent: buffer.extent,
            fourcc: buffer.fourcc,
            modifier: buffer.modifier,
            pitches,
            offsets,
            handles,
        };
        let fb = match device.add_planar_framebuffer(&planar, drm::control::FbCmd2Flags::MODIFIERS)
        {
            Ok(fb) => fb,
            Err(e) => {
                close(&buffers);
                return Err(backend("a framebuffer", e));
            }
        };
        let key = u64::from(u32::from(fb));
        self.framebuffers.insert(key, Framebuffer { fb, buffers });
        Ok(FbHandle(key))
    }

    fn release_framebuffer(&mut self, fb: FbHandle) {
        // One of a card since lost is not in the table: it went with it.
        let Some(entry) = self.framebuffers.remove(&fb.0) else {
            return;
        };
        let _ = self.pipeline.device.destroy_framebuffer(entry.fb);
        for buffer in entry.buffers {
            let _ = self.pipeline.device.close_buffer(buffer);
        }
    }

    fn commit(&mut self, request: CommitRequest) -> emblema_hal::Result<()> {
        self.drain();
        if self.lost.is_some() {
            // Another session has the card. The frame is not shown, and is
            // done with as far as the ring is concerned.
            self.pacing.dropped.fetch_add(1, Ordering::Relaxed);
            self.pending
                .push(OutputEvent::FlipComplete { fb: request.fb });
            return Ok(());
        }
        if !self.framebuffers.contains_key(&request.fb.0) {
            // One of the card as it was before the session left and came
            // back, if that is what happened: the ring is to be rebuilt.
            return Err(Error::Unsupported(
                "committing a framebuffer this output did not import",
            ));
        }
        let fb = request.fb.0;
        let modeset = request.allow_modeset || self.modeset;
        let (p, pipe) = (&self.pipeline.props, &self.pipeline);
        let (width, height) = pipe.size();
        let mut atomic = AtomicRequest::with_capacity(16);
        let mut writes: Vec<(u32, u32, u64)> = Vec::with_capacity(16);
        if modeset {
            writes.push((pipe.connector, p.connector_crtc_id, u64::from(pipe.crtc)));
            writes.push((pipe.crtc, p.crtc_mode_id, pipe.mode_blob));
            writes.push((pipe.crtc, p.crtc_active, 1));
        }
        writes.push((pipe.plane, p.fb_id, fb));
        writes.push((pipe.plane, p.plane_crtc_id, u64::from(pipe.crtc)));
        // The source rectangle is 16.16 fixed point.
        for (prop, value) in
            p.src
                .iter()
                .zip([0, 0, u64::from(width) << 16, u64::from(height) << 16])
        {
            writes.push((pipe.plane, *prop, value));
        }
        for (prop, value) in p
            .dst
            .iter()
            .zip([0, 0, u64::from(width), u64::from(height)])
        {
            writes.push((pipe.plane, *prop, value));
        }
        // A commit that sets the mode waits for the drawing itself: the
        // fence is the caller's to have waited on, and none comes with one
        // it asked for. One this output asks for has its fence waited on
        // here.
        let fence = request.in_fence_fd;
        match (&fence, p.in_fence_fd) {
            (Some(fd), Some(prop)) if !modeset => {
                use std::os::fd::AsRawFd as _;
                writes.push((pipe.plane, prop, u64::try_from(fd.as_raw_fd()).unwrap_or(0)));
            }
            (Some(fd), _) => wait_signaled(fd),
            (None, _) => {}
        }
        for (object, property, value) in writes {
            atomic
                .add_property(object, property, value)
                .map_err(|e| backend("a commit", e))?;
        }
        let mut flags = AtomicCommitFlags::PAGE_FLIP_EVENT | AtomicCommitFlags::NONBLOCK;
        if modeset {
            flags |= AtomicCommitFlags::ALLOW_MODESET;
        }
        if let Err(e) = atomic.commit(&pipe.device, flags) {
            // Refused as the session left, ahead of the seat's saying so.
            if matches!(e, CoreError::NotMaster) || !self.activity.is_active() {
                if self.activity.is_active() && self.pacing.flips() == 0 {
                    // Never master at all: the card is someone else's, a
                    // compositor's or another seat client's.
                    return Err(Error::Unsupported(
                        "the card is in use: another DRM master, a compositor or a display manager, holds it",
                    ));
                }
                self.lose();
                self.pacing.dropped.fetch_add(1, Ordering::Relaxed);
                self.pending
                    .push(OutputEvent::FlipComplete { fb: request.fb });
                return Ok(());
            }
            return Err(backend("a commit", e));
        }
        drop(fence);
        self.modeset = false;
        self.in_flight = Some(request.fb);
        Ok(())
    }

    fn poll_events(&mut self) -> Vec<OutputEvent> {
        self.drain();
        std::mem::take(&mut self.pending)
    }

    fn wait_for_event(&mut self, timeout_nanos: u64) -> emblema_hal::Result<Vec<OutputEvent>> {
        self.drain();
        if self.pending.is_empty() && self.lost.is_none() {
            // Woken by the card, not by a clock.
            let _ = self
                .pipeline
                .flips
                .dispatch(Timeout::Bounded(Duration::from_nanos(timeout_nanos)));
            self.take_flips();
        } else if self.pending.is_empty() {
            // Nothing to wait on while the card is away but the time.
            std::thread::sleep(Duration::from_nanos(timeout_nanos).min(Duration::from_millis(50)));
            self.drain();
        }
        Ok(std::mem::take(&mut self.pending))
    }
}

/// Waits until `fence`, a sync file, has signaled, for a bounded time: a
/// frame's drawing, which a second is far beyond.
fn wait_signaled(fence: &OwnedFd) {
    use rustix::event::{poll, PollFd, PollFlags};
    let mut fds = [PollFd::new(fence, PollFlags::IN)];
    let timeout = rustix::event::Timespec {
        tv_sec: 1,
        tv_nsec: 0,
    };
    let _ = poll(&mut fds, Some(&timeout));
}

impl Drop for Display {
    fn drop(&mut self) {
        let keys: Vec<u64> = self.framebuffers.keys().copied().collect();
        for key in keys {
            self.release_framebuffer(FbHandle(key));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mode_asked_for_is_the_display_s_own_of_that_size() {
        // A display's modes: width, height, millihertz.
        let modes = [
            (3840, 2160, 30_000),
            (1280, 720, 60_000),
            (1280, 720, 59_940),
            (1280, 720, 50_000),
        ];
        let ask = |width, height, hz| WantedMode { width, height, hz };
        let hz = |i: Option<usize>| i.map(|i| modes[i].2);
        // The fastest of a size with no rate said; the nearest with one.
        assert_eq!(hz(pick_mode(&modes, ask(1280, 720, None))), Some(60_000));
        assert_eq!(
            hz(pick_mode(&modes, ask(1280, 720, Some(50.0)))),
            Some(50_000)
        );
        assert_eq!(
            hz(pick_mode(&modes, ask(1280, 720, Some(59.9)))),
            Some(59_940)
        );
        assert_eq!(
            hz(pick_mode(&modes, ask(3840, 2160, Some(60.0)))),
            Some(30_000)
        );
        // A size the display has not is no mode, whatever the rate.
        assert_eq!(pick_mode(&modes, ask(1920, 1080, None)), None);
        assert_eq!(pick_mode(&[], ask(1280, 720, None)), None);
        // A rate that is no number is none said.
        assert_eq!(
            hz(pick_mode(&modes, ask(1280, 720, Some(f64::NAN)))),
            Some(60_000)
        );
    }

    #[test]
    fn a_mode_s_rate_is_its_timings_not_the_rounded_hertz() {
        // 1920x1080 at 148.5 MHz over 2200x1125 is 60 Hz exactly; at
        // 148.352 MHz it is the 59.94 the kernel also calls 60.
        assert_eq!(millihertz(148_500, 2200, 1125, 60), 60_000);
        assert_eq!(millihertz(148_352, 2200, 1125, 60), 59_940);
        // Timings that say nothing leave the kernel's figure.
        assert_eq!(millihertz(0, 2200, 1125, 60), 60_000);
        assert_eq!(millihertz(148_500, 0, 1125, 50), 50_000);
        assert_eq!(millihertz(u32::MAX, 1, 1, 60), u32::MAX);
    }

    #[test]
    fn pacing_counts_flips_and_the_blanks_they_span() {
        let p = Pacing::default();
        assert_eq!((p.flips(), p.vblanks()), (0, 0));
        // A flip at every blank: one fewer blanks than flips.
        for sequence in 1000..1010 {
            p.observe(sequence);
        }
        assert_eq!((p.flips(), p.vblanks()), (10, 9));
        // One that came a blank late shows in the span, not the count.
        p.observe(1011);
        assert_eq!((p.flips(), p.vblanks()), (11, 11));
    }

    #[test]
    fn a_kernel_timestamp_is_an_instant_of_the_same_clock() {
        let t = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        let nanos = u64::try_from(t.tv_sec).unwrap_or(0) * 1_000_000_000
            + u64::try_from(t.tv_nsec).unwrap_or(0);
        let before = Instant::now();
        // Now, as the kernel would stamp it, is now; a time 5 ms ago is 5 ms
        // before it. Within the time these calls take.
        let slack = Duration::from_millis(2);
        let now = instant_of(nanos);
        assert!(before.saturating_duration_since(now) < slack);
        assert!(now.saturating_duration_since(before) < slack);
        let earlier = instant_of(nanos - 5_000_000);
        let gap = before.saturating_duration_since(earlier);
        assert!(
            gap > Duration::from_millis(3) && gap < Duration::from_millis(7),
            "{gap:?}"
        );
        // A time ahead of the clock, which the kernel does not give, is now.
        assert!(instant_of(u64::MAX).saturating_duration_since(before) < slack);
    }
}
