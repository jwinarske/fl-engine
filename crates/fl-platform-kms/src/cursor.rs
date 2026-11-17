//! The mouse cursor (brief M4 D9): an XCursor theme's cursors, read by
//! drmkit-cursor, on the cursor plane of the display's CRTC.
//!
//! On a thread of its own, with a descriptor of its own for the card (the
//! seat's, duplicated, so the same master): the renderer borrows its
//! device, and a descriptor lasts from one switch of terminals to the
//! next. The plane is driven by the legacy cursor calls, which the kernel
//! applies with no wait for a blank, so a moving pointer and the frames'
//! flips do not wait on each other; a card whose cursor those calls cannot
//! drive has no cursor here.
//!
//! What is asked of it is a position (the devices'), a shape (the
//! framework's, on `flutter/mousecursor`), and the session's going and
//! coming back. Asks are taken as they have piled up, the last position
//! the one shown.

use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use drmkit_core::Device;
use drmkit_cursor::{Paused, PlanePath, Renderer, RendererConfig, Theme};
use drmkit_planes::PlaneRegistry;
use fl_input::{MouseCursor, FALLBACK_CURSOR};

use crate::seat::Seat;

/// A cursor's nominal size in logical pixels, a theme's usual.
const NOMINAL: f64 = 24.0;

/// How often an animated cursor is looked at for its next frame.
const ANIMATION_TICK: Duration = Duration::from_millis(16);

/// The name the framework's `SystemMouseCursors.none` has.
const NONE: &str = "none";

/// A cursor name is a theme's file name; none is longer than this.
const MAX_NAME: usize = 64;

/// Which CRTC of which card a cursor is shown on: a display's
/// (`Display::cursor_target`).
#[derive(Debug, Clone)]
pub struct CursorTarget {
    pub(crate) path: PathBuf,
    pub(crate) crtc_id: u32,
    pub(crate) crtc_index: u32,
}

#[derive(Debug)]
pub(crate) enum Ask {
    Move(i32, i32),
    Hide,
    Shape(String),
    Pause,
    Resume(OwnedFd),
    Stop,
}

/// A way to ask things of a cursor, from any thread.
#[derive(Debug, Clone)]
pub struct Pointer {
    tx: Sender<Ask>,
    path: PathBuf,
}

impl Pointer {
    /// The pointer is here, in the display's pixels, and is shown.
    pub(crate) fn move_to(&self, x: f64, y: f64) {
        // Positions are held to the view before they come here.
        #[allow(clippy::cast_possible_truncation)]
        let _ = self.tx.send(Ask::Move(x.round() as i32, y.round() as i32));
    }

    /// No pointer is shown until it moves again: a finger is on the
    /// screen, or the devices are gone.
    pub(crate) fn hide(&self) {
        let _ = self.tx.send(Ask::Hide);
    }

    /// The session is going: the card is not this process's from now.
    pub(crate) fn pause(&self) {
        let _ = self.tx.send(Ask::Pause);
    }

    /// The session is back, with the card on `fd`.
    pub(crate) fn resume(&self, fd: OwnedFd) {
        let _ = self.tx.send(Ask::Resume(fd));
    }

    /// The card's node, for the seat to take again.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// A pointer whose asks a test reads.
    #[cfg(test)]
    pub(crate) fn kept() -> (Pointer, Receiver<Ask>) {
        let (tx, rx) = mpsc::channel();
        (
            Pointer {
                tx,
                path: PathBuf::new(),
            },
            rx,
        )
    }
}

impl MouseCursor for Pointer {
    fn activate(&mut self, name: &str) {
        if name.len() <= MAX_NAME {
            let _ = self.tx.send(Ask::Shape(name.to_owned()));
        }
    }
}

/// The cursor of a display. Dropped, it is taken off the screen.
pub struct Cursor {
    pointer: Pointer,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Cursor {
    /// A cursor on `target`, its card opened through `seat`, sized for
    /// `scale` physical pixels to a logical one. Nothing is shown until
    /// the pointer moves. An error is a card with no cursor to drive, or
    /// a machine with no cursor theme: the app runs without one.
    pub fn open(seat: &Seat, target: &CursorTarget, scale: f64) -> Result<Cursor, String> {
        let fd = seat
            .take_device(&target.path)
            .map_err(|e| format!("{}: {e}", target.path.display()))?;
        let theme = Theme::discover().map_err(|e| e.to_string())?;
        let scale = if scale.is_finite() && scale > 0.0 {
            scale.min(8.0)
        } else {
            1.0
        };
        // Bounded just above: 8 to 192.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let size = (NOMINAL * scale).round() as u32;
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let config = RendererConfig {
            crtc_id: target.crtc_id,
            crtc_index: target.crtc_index,
            allow_legacy: true,
            prefer_legacy: true,
            ..RendererConfig::default()
        };
        let thread = std::thread::Builder::new()
            .name("fl-cursor".into())
            .spawn(move || run(fd, &rx, &theme, &config, size, &ready_tx))
            .map_err(|e| e.to_string())?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Cursor {
                pointer: Pointer {
                    tx,
                    path: target.path.clone(),
                },
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err("the cursor's thread ended".into())
            }
        }
    }

    /// A way to move it and to set its shape.
    pub fn pointer(&self) -> Pointer {
        self.pointer.clone()
    }
}

impl Drop for Cursor {
    fn drop(&mut self) {
        let _ = self.pointer.tx.send(Ask::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// What is to be on the screen.
struct Wanted {
    x: i32,
    y: i32,
    /// The pointer has moved, and no finger has been down since.
    shown: bool,
    /// The cursor theme's name, or [`NONE`].
    shape: String,
}

/// What the asks that have arrived come to.
enum Next {
    Draw,
    Pause,
    Stop,
}

impl Wanted {
    fn take(&mut self, ask: Ask) -> Next {
        match ask {
            Ask::Move(x, y) => {
                (self.x, self.y) = (x, y);
                self.shown = true;
            }
            Ask::Hide => self.shown = false,
            Ask::Shape(shape) => self.shape = shape,
            Ask::Pause => return Next::Pause,
            Ask::Stop => return Next::Stop,
            // Not paused: there is a card already.
            Ask::Resume(_) => {}
        }
        Next::Draw
    }

    /// `first`, and every ask behind it: the last of each is what counts.
    fn take_all(&mut self, first: Ask, rx: &Receiver<Ask>) -> Next {
        let mut next = self.take(first);
        while matches!(next, Next::Draw) {
            match rx.try_recv() {
                Ok(ask) => next = self.take(ask),
                Err(_) => break,
            }
        }
        next
    }
}

/// The thread: one pass of the outer loop a descriptor.
fn run(
    fd: OwnedFd,
    rx: &Receiver<Ask>,
    theme: &Theme,
    config: &RendererConfig,
    size: u32,
    ready: &Sender<Result<(), String>>,
) {
    let mut wanted = Wanted {
        x: 0,
        y: 0,
        shown: false,
        shape: FALLBACK_CURSOR.to_owned(),
    };
    let mut fd = fd;
    let mut paused: Option<Paused> = None;
    let mut ready = Some(ready);
    loop {
        paused = on_card(
            fd,
            paused,
            rx,
            theme,
            config,
            size,
            &mut wanted,
            ready.take(),
        );
        if paused.is_none() {
            return;
        }
        // Until the session is back: what is wanted is kept up, and
        // shown then.
        fd = loop {
            match rx.recv() {
                Ok(Ask::Resume(again)) => break again,
                Ok(Ask::Stop) | Err(_) => return,
                Ok(ask) => {
                    let _ = wanted.take(ask);
                }
            }
        };
    }
}

/// The cursor on one descriptor of the card, until the session goes
/// (what a later descriptor is resumed from) or the cursor is done with
/// (`None`).
#[allow(clippy::too_many_arguments)]
fn on_card(
    card: OwnedFd,
    paused: Option<Paused>,
    rx: &Receiver<Ask>,
    theme: &Theme,
    config: &RendererConfig,
    size: u32,
    wanted: &mut Wanted,
    ready: Option<&Sender<Result<(), String>>>,
) -> Option<Paused> {
    let device = Device::from_owned_fd(card);
    // The display has asked for these of the same open card, or is about
    // to; either order is the same.
    let _ = device.enable_universal_planes();
    let _ = device.enable_atomic();
    let made = match paused {
        Some(paused) => paused.resume(&device).map_err(|e| e.to_string()),
        None => create(&device, config),
    };
    if let Some(ready) = ready {
        let _ = ready.send(made.as_ref().map(|_| ()).map_err(Clone::clone));
    }
    let mut renderer = made.ok()?;
    match serve(&mut renderer, rx, theme, size, wanted) {
        Next::Pause => Some(renderer.pause()),
        Next::Stop | Next::Draw => {
            renderer.set_visible(false);
            let _ = renderer.commit();
            None
        }
    }
}

/// A renderer on the card's cursor plane, by the legacy calls alone.
fn create<'a>(device: &'a Device, config: &RendererConfig) -> Result<Renderer<'a>, String> {
    let registry = PlaneRegistry::probe(device).map_err(|e| format!("planes: {e}"))?;
    let renderer = Renderer::create(device, &registry, config).map_err(|e| e.to_string())?;
    if renderer.plane().path != PlanePath::Legacy {
        return Err("the card's cursor is not one the legacy cursor calls drive".into());
    }
    Ok(renderer)
}

/// Shows what is wanted, as it changes, until the session goes or the
/// cursor is dropped.
fn serve(
    renderer: &mut Renderer<'_>,
    rx: &Receiver<Ask>,
    theme: &Theme,
    size: u32,
    wanted: &mut Wanted,
) -> Next {
    // The shape in the renderer, and when it was put there: an animated
    // one's frames are timed from then.
    let mut loaded: Option<String> = None;
    let mut since = Instant::now();
    let mut animated = false;
    // The buffer is the card's size for a cursor; a theme's is no larger.
    let size = size.min(renderer.size().0).max(1);
    loop {
        if loaded.as_deref() != Some(wanted.shape.as_str()) {
            loaded = Some(wanted.shape.clone());
            animated = false;
            if wanted.shape != NONE {
                // A name the theme has not is the default's; a theme with
                // not even that leaves the cursor as it was.
                let cursor = drmkit_cursor::Cursor::load_named(theme, &wanted.shape, "", size)
                    .or_else(|_| {
                        drmkit_cursor::Cursor::load_named(theme, FALLBACK_CURSOR, "", size)
                    });
                if let Ok(cursor) = cursor {
                    animated = cursor.is_animated();
                    since = Instant::now();
                    let _ = renderer.set_cursor(cursor);
                }
            }
        }
        renderer.move_to(wanted.x, wanted.y);
        renderer.set_visible(wanted.shown && wanted.shape != NONE);
        if animated {
            let _ = renderer.tick(since.elapsed());
        }
        // Refused while the card is another session's, which is told
        // of next: nothing to do about it here.
        let _ = renderer.commit();
        let ask = if animated && wanted.shown {
            match rx.recv_timeout(ANIMATION_TICK) {
                Ok(ask) => ask,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Next::Stop,
            }
        } else {
            match rx.recv() {
                Ok(ask) => ask,
                Err(_) => return Next::Stop,
            }
        };
        match wanted.take_all(ask, rx) {
            Next::Draw => {}
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted() -> Wanted {
        Wanted {
            x: 0,
            y: 0,
            shown: false,
            shape: FALLBACK_CURSOR.to_owned(),
        }
    }

    #[test]
    fn the_last_of_the_asks_that_piled_up_is_what_is_shown() {
        let (mut pointer, rx) = Pointer::kept();
        let mut w = wanted();
        pointer.move_to(10.4, 20.6);
        pointer.move_to(11.0, 21.0);
        pointer.activate("text");
        pointer.move_to(300.0, 400.0);
        let first = rx.recv().unwrap();
        assert!(matches!(w.take_all(first, &rx), Next::Draw));
        assert_eq!(
            (w.x, w.y, w.shown, w.shape.as_str()),
            (300, 400, true, "text")
        );
        // A finger hides it until it moves again.
        pointer.hide();
        let first = rx.recv().unwrap();
        assert!(matches!(w.take_all(first, &rx), Next::Draw));
        assert!(!w.shown);
        // A name no theme's file has the length of is not sent.
        pointer.activate(&"x".repeat(MAX_NAME + 1));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_pause_stops_the_taking_and_what_follows_is_kept_for_later() {
        let (pointer, rx) = Pointer::kept();
        let mut w = wanted();
        pointer.move_to(1.0, 2.0);
        pointer.pause();
        pointer.move_to(5.0, 6.0);
        let first = rx.recv().unwrap();
        assert!(matches!(w.take_all(first, &rx), Next::Pause));
        assert_eq!((w.x, w.y), (1, 2));
        // What came after the pause is still there to take.
        assert!(matches!(rx.try_recv(), Ok(Ask::Move(5, 6))));
    }
}
