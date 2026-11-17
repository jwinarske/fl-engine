//! The seat (brief M4 §4, §15): devices are held through libseat, so the
//! card and the input devices come with no root and no `video` or `input`
//! group, and go together when the terminal is switched away from.
//!
//! libseat's connection is one thread's, so the seat has a thread: it
//! opens what it is asked to, and hears the seat say the session is paused
//! or back. A seat that cannot be had is an error that says what is
//! missing, not something worked around by opening the nodes directly.

use std::cell::RefCell;
use std::os::fd::{AsFd as _, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

use drmkit_session::{LibseatBackend, Session, SessionError, SessionEvent};
use fl_input::EventSink;
use rustix::event::{eventfd, poll, EventfdFlags, PollFd, PollFlags};

use crate::devices::{Devices, DevicesConfig};

/// Why there is no seat, or no device from it.
#[derive(Debug)]
pub enum SeatError {
    /// No seat to be had: no logind session on this terminal, no seatd
    /// this user may talk to, and not root on a terminal.
    NoSeat,
    /// The seat refused, or could not open, what was asked of it.
    Seat(String),
    /// The seat's thread has gone.
    Gone,
}

impl std::fmt::Display for SeatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SeatError::NoSeat => f.write_str(
                "no seat: run from a login on the machine's console, or with seatd running and this user in its group (LIBSEAT_BACKEND picks one)",
            ),
            SeatError::Seat(e) => write!(f, "seat: {e}"),
            SeatError::Gone => f.write_str("seat: its thread has ended"),
        }
    }
}

impl std::error::Error for SeatError {}

/// Whether a seat's session has its devices, for the threads that use
/// them.
#[derive(Debug)]
pub struct Activity {
    active: AtomicBool,
    resumes: AtomicU64,
}

impl Activity {
    /// False between a switch away from this terminal and the switch back.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// How many times the session has come back. A device lost is to be
    /// had again once this has moved on: the descriptor from before is
    /// not the seat's any more.
    pub fn resumes(&self) -> u64 {
        self.resumes.load(Ordering::Acquire)
    }
}

enum Request {
    Take(PathBuf, Sender<Result<OwnedFd, SeatError>>),
    /// Read the seat's devices for a loop, in place of any read before.
    Devices(
        DevicesConfig,
        Arc<dyn EventSink>,
        Sender<Result<(), SeatError>>,
    ),
    /// Stop reading them, and let the loop they were read for go.
    NoDevices(Sender<()>),
    Stop,
}

/// A held seat. Dropping it gives the devices back.
pub struct Seat {
    requests: Sender<Request>,
    wake: Arc<OwnedFd>,
    activity: Arc<Activity>,
    name: String,
    thread: Option<JoinHandle<()>>,
}

impl Seat {
    /// Takes the seat this process can have, and waits for it to be ours.
    pub fn open() -> Result<Seat, SeatError> {
        let (requests, rx) = mpsc::channel();
        let (ready_tx, ready) = mpsc::channel();
        let wake = Arc::new(
            eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)
                .map_err(|e| SeatError::Seat(format!("eventfd: {e}")))?,
        );
        let activity = Arc::new(Activity {
            active: AtomicBool::new(true),
            resumes: AtomicU64::new(0),
        });
        let (thread_wake, thread_active) = (Arc::clone(&wake), Arc::clone(&activity));
        let thread = std::thread::Builder::new()
            .name("fl-seat".into())
            .spawn(move || seat_main(&rx, &ready_tx, &thread_wake, &thread_active))
            .map_err(|e| SeatError::Seat(format!("seat thread: {e}")))?;
        match ready.recv() {
            Ok(Ok(name)) => Ok(Seat {
                requests,
                wake,
                activity,
                name,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(SeatError::Gone)
            }
        }
    }

    /// The seat's name, `seat0` on most machines.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the session has its devices: false between a switch away
    /// from this terminal and the switch back.
    pub fn is_active(&self) -> bool {
        self.activity.is_active()
    }

    /// What says whether the session has its devices, for a thread that
    /// uses one.
    pub fn activity(&self) -> Arc<Activity> {
        Arc::clone(&self.activity)
    }

    /// Opens `path` through the seat. The descriptor is a duplicate of the
    /// seat's: the same open device, so what the seat does to its own (DRM
    /// master taken and given back) is done to this one.
    pub fn take_device(&self, path: impl AsRef<Path>) -> Result<OwnedFd, SeatError> {
        let (tx, rx) = mpsc::channel();
        self.requests
            .send(Request::Take(path.as_ref().to_path_buf(), tx))
            .map_err(|_| SeatError::Gone)?;
        self.wake()?;
        rx.recv().map_err(|_| SeatError::Gone)?
    }

    /// Reads the seat's keyboards, mice, touchpads and touch screens from
    /// now on, delivering to `sink`: a loop's way in, which keeps that loop
    /// alive until [`Seat::release_devices`]. The devices are opened
    /// through the seat, on its thread, and go and come back with the
    /// session. Devices read for another loop before are let go first.
    pub fn read_devices(
        &self,
        config: DevicesConfig,
        sink: Arc<dyn EventSink>,
    ) -> Result<(), SeatError> {
        let (tx, rx) = mpsc::channel();
        self.requests
            .send(Request::Devices(config, sink, tx))
            .map_err(|_| SeatError::Gone)?;
        self.wake()?;
        rx.recv().map_err(|_| SeatError::Gone)?
    }

    /// Stops reading the devices: what was down is told as up, and the
    /// loop they were read for is no longer held open by them.
    pub fn release_devices(&self) {
        let (tx, rx) = mpsc::channel();
        if self.requests.send(Request::NoDevices(tx)).is_ok() && self.wake().is_ok() {
            let _ = rx.recv();
        }
    }

    fn wake(&self) -> Result<(), SeatError> {
        rustix::io::write(&*self.wake, &1u64.to_ne_bytes())
            .map(|_| ())
            .map_err(|e| SeatError::Seat(format!("wake: {e}")))
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        let _ = self.requests.send(Request::Stop);
        let _ = self.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn seat_error(e: &SessionError) -> SeatError {
    match e {
        SessionError::NoBackend => SeatError::NoSeat,
        other => SeatError::Seat(other.to_string()),
    }
}

/// The seat's thread: the session is made here and stays here.
fn seat_main(
    requests: &Receiver<Request>,
    ready: &Sender<Result<String, SeatError>>,
    wake: &OwnedFd,
    activity: &Activity,
) {
    let mut backend = match LibseatBackend::open() {
        Ok(b) => b,
        Err(e) => {
            let _ = ready.send(Err(seat_error(&e)));
            return;
        }
    };
    let name = backend.name().to_owned();
    let session = Rc::new(RefCell::new(Session::new(backend)));
    let _ = ready.send(Ok(name.clone()));
    let mut devices: Option<Devices> = None;
    loop {
        // Requests first: one may have been sent before the wait began.
        loop {
            match requests.try_recv() {
                Ok(Request::Take(path, reply)) => {
                    let _ = reply.send(take(&mut session.borrow_mut(), &path));
                }
                Ok(Request::Devices(config, sink, reply)) => {
                    // The last loop's first: its keys come up before these
                    // are read.
                    devices = None;
                    let opened =
                        Devices::open(&session, &name, &config, sink).map_err(SeatError::Seat);
                    let _ = reply.send(opened.map(|d| devices = Some(d)));
                }
                Ok(Request::NoDevices(reply)) => {
                    devices = None;
                    let _ = reply.send(());
                }
                Ok(Request::Stop) | Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        // A loop that has ended takes nothing more.
        if devices.as_ref().is_some_and(Devices::is_closed) {
            devices = None;
        }
        let seat_fd = session.borrow().poll_fd();
        // SAFETY: the seat's descriptor is the connection's, open for as
        // long as `session` is, which outlives this borrow.
        let seat_fd = seat_fd.map(|fd| unsafe { BorrowedFd::borrow_raw(fd) });
        let (mut seat_ready, mut input_ready, mut repeat_ready) = (false, false, false);
        {
            let mut fds = vec![PollFd::new(wake, PollFlags::IN)];
            if let Some(fd) = seat_fd.as_ref() {
                fds.push(PollFd::new(fd, PollFlags::IN));
            }
            let first_device = fds.len();
            let device_fds = devices.as_ref().map(|d| (d.fd(), d.repeat_fd()));
            if let Some((input, repeat)) = device_fds.as_ref() {
                fds.push(PollFd::new(input, PollFlags::IN));
                fds.push(PollFd::new(repeat, PollFlags::IN));
            }
            // Bounded while devices are read, so a loop that ended with
            // none of them saying anything is still let go.
            let bound = rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 250_000_000,
            };
            match poll(&mut fds, device_fds.is_some().then_some(&bound)) {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(_) => return,
            }
            let ready = |i: usize| fds.get(i).is_some_and(|fd| !fd.revents().is_empty());
            if seat_fd.is_some() {
                seat_ready = ready(1);
            }
            if device_fds.is_some() {
                input_ready = ready(first_device);
                repeat_ready = ready(first_device + 1);
            }
        }
        let mut count = [0u8; 8];
        let _ = rustix::io::read(wake.as_fd(), &mut count);
        if seat_ready {
            let dispatched = session.borrow_mut().dispatch(0);
            if dispatched.is_err() {
                // The connection is gone, and the devices with it.
                activity.active.store(false, Ordering::Release);
                return;
            }
            loop {
                let event = session.borrow_mut().next_event();
                match event {
                    Some(SessionEvent::Paused) => {
                        activity.active.store(false, Ordering::Release);
                        if let Some(d) = devices.as_mut() {
                            d.pause();
                        }
                    }
                    Some(SessionEvent::Resumed { .. }) => {
                        // Counted first: whoever sees it active sees the
                        // count that says its devices are new.
                        activity.resumes.fetch_add(1, Ordering::AcqRel);
                        activity.active.store(true, Ordering::Release);
                        if let Some(d) = devices.as_mut() {
                            d.resume();
                        }
                    }
                    Some(_) => {}
                    None => break,
                }
            }
        }
        if let Some(d) = devices.as_mut() {
            if repeat_ready {
                d.repeat();
            }
            if input_ready {
                // Ctrl+Alt and a function key: another terminal, asked of
                // the seat, which says when this session is paused for it.
                if let Some(vt) = d.dispatch().switch_to {
                    let _ = session.borrow_mut().switch_session(vt);
                }
            }
        }
    }
}

fn take(session: &mut Session<LibseatBackend>, path: &Path) -> Result<OwnedFd, SeatError> {
    let device = session.take_device(path).map_err(|e| seat_error(&e))?;
    // SAFETY: the seat holds this descriptor open until the device is
    // released or the session ends, neither of which happens in this call.
    let seats = unsafe { BorrowedFd::borrow_raw(device.fd) };
    rustix::io::fcntl_dupfd_cloexec(seats, 3)
        .map_err(|e| SeatError::Seat(format!("{}: {e}", path.display())))
}
