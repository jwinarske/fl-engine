//! The raster thread with a display for its target (brief M4 D9), over an
//! output that is no card: it takes the ring's buffers, counts what it is
//! asked, and once says everything is to be rebuilt, as a card taken back
//! after a switch of terminals does. What a real output adds is KMS, which
//! the boards check.
//!
//! Needs a Vulkan driver that exports dma-bufs:
//!
//!     cargo test -p fl-raster --features aot,scanout --test scanout -- --ignored
#![cfg(feature = "scanout")]
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use fl_raster::scanout::{
    CommitRequest, DmaBufPlanes, Error, Extent2D, FbHandle, FormatModifierSet, Fourcc, Mode,
    Modifier, OutputEvent, ScanoutOutput,
};
use fl_raster::RasterThread;
use fl_ui::{Draw, LayerOp, Paint, Picture, Scene};

#[derive(Default)]
struct Log {
    imports: usize,
    /// The planes of each import, and its size.
    shapes: Vec<(usize, u32, u32)>,
    live: HashSet<u64>,
    commits: usize,
    refused: usize,
}

struct Fake {
    log: Arc<Mutex<Log>>,
    formats: Vec<FormatModifierSet>,
    pending: Vec<OutputEvent>,
    /// The commit at which the card is lost and had again: its buffers
    /// gone, the commit refused.
    rebuild_at: Option<usize>,
    next: u64,
}

impl ScanoutOutput for Fake {
    fn mode(&self) -> Mode {
        Mode {
            extent: Extent2D::new(320, 240),
            refresh_mhz: 60_000,
        }
    }

    fn supported_formats(&self) -> &[FormatModifierSet] {
        &self.formats
    }

    fn import_dmabuf(&mut self, buffer: DmaBufPlanes) -> Result<FbHandle, Error> {
        let mut log = self.log.lock().unwrap();
        log.imports += 1;
        log.shapes.push((
            buffer.planes.len(),
            buffer.extent.width,
            buffer.extent.height,
        ));
        self.next += 1;
        log.live.insert(self.next);
        Ok(FbHandle(self.next))
    }

    fn release_framebuffer(&mut self, fb: FbHandle) {
        self.log.lock().unwrap().live.remove(&fb.0);
    }

    fn commit(&mut self, request: CommitRequest) -> Result<(), Error> {
        let mut log = self.log.lock().unwrap();
        if self.rebuild_at == Some(log.commits) {
            self.rebuild_at = None;
            log.live.clear();
            self.pending.push(OutputEvent::Reconfigured);
        }
        if !log.live.contains(&request.fb.0) {
            log.refused += 1;
            return Err(Error::Unsupported("a framebuffer of the card before"));
        }
        log.commits += 1;
        self.pending
            .push(OutputEvent::FlipComplete { fb: request.fb });
        Ok(())
    }

    fn poll_events(&mut self) -> Vec<OutputEvent> {
        std::mem::take(&mut self.pending)
    }

    fn wait_for_event(&mut self, _timeout_nanos: u64) -> Result<Vec<OutputEvent>, Error> {
        Ok(std::mem::take(&mut self.pending))
    }
}

fn scene(shade: f32) -> Arc<Scene> {
    let rect = Draw::Rect {
        ltrb: [10.0, 10.0, 200.0, 120.0],
        paint: Paint {
            color: [shade, 0.2, 0.4, 1.0],
            ..Paint::default()
        },
    };
    Arc::new(Scene {
        ops: vec![LayerOp::AddPicture {
            dx: 0.0,
            dy: 0.0,
            picture: Arc::new(Picture { draws: vec![rect] }),
        }],
    })
}

#[test]
#[ignore = "needs a Vulkan driver that exports dma-bufs"]
fn frames_go_to_the_output_and_the_ring_is_rebuilt_when_it_says_so() {
    let log = Arc::new(Mutex::new(Log::default()));
    let fake = Fake {
        log: Arc::clone(&log),
        // What a primary plane commonly takes, and emblema renders to.
        formats: vec![FormatModifierSet::new(
            Fourcc::ARGB8888,
            vec![Modifier::LINEAR],
        )],
        pending: Vec::new(),
        rebuild_at: Some(3),
        next: 0,
    };
    let raster = RasterThread::spawn_scanout(Box::new(fake)).unwrap();
    // The target is the mode's size.
    assert_eq!(raster.size(), (320, 240));
    let ring = log.lock().unwrap().imports;
    assert!(ring >= 2, "a ring of {ring}");
    for i in 0..8 {
        assert!(raster.submit(scene(i as f32 / 8.0)));
    }
    let done = raster.finish();
    assert_eq!(done.error, None);
    assert_eq!(done.frames, 8);
    // No pixels are kept of a frame that went to a display.
    assert!(done.last.is_none());
    let log = log.lock().unwrap();
    // Every frame committed; the one the card was lost under refused once
    // and committed again from the new ring.
    assert_eq!((log.commits, log.refused), (8, 1));
    assert_eq!(log.imports, 2 * ring);
    assert!(
        log.shapes.iter().all(|s| *s == (1, 320, 240)),
        "{:?}",
        log.shapes
    );
    // And every buffer given back by the end.
    assert!(log.live.is_empty(), "{:?}", log.live);
}
