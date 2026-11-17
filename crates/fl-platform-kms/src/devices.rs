//! The seat's input devices (brief M4 §4, §6): libinput's events, through
//! drmkit-input, as the events the Linux embedder is given by GDK, and from
//! there the path a script's take: the key tracker, and the pointer,
//! scrolling and touch managers of fl-input.
//!
//! The devices are opened through the session, so a switch of terminals
//! takes them with the card; this runs on the seat's thread, and never
//! touches Dart. What reaches it is bounded on the way in (§15): positions
//! are held to the view, a key code outside the keymap's range is dropped,
//! and a finger is a slot libinput numbered.

use std::cell::RefCell;
use std::os::fd::{AsFd, AsRawFd as _, BorrowedFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use drmkit_input::{
    InputEvent, KeyRepeater, LibinputInterface, PointerEvent as Pointer, RepeatConfig,
    ScrollSource, SeatOptions, TouchPhase,
};
use drmkit_session::{LibseatBackend, Session};
use fl_input::pointer::{button, At, Mouse, Scroll, Scrolling, TouchAction, Touches};
use fl_input::xkb::{Keymap as _, Keys};
use fl_input::{EventSink, PointerEvent};

use crate::keymap::XkbKeymap;

/// What the devices are read for: a view, and a keymap.
#[derive(Debug, Clone)]
pub struct DevicesConfig {
    /// The XKB layout and variant keys are resolved against.
    pub layout: String,
    pub variant: String,
    /// The view's size in physical pixels: what a pointer is held to, and
    /// what a touch screen's or a tablet's extent is mapped onto.
    pub width: u32,
    pub height: u32,
    /// Physical pixels to a logical one: a wheel's notch is in logical
    /// pixels.
    pub scale: f64,
}

/// The one view there is.
const VIEW: i64 = 0;

/// evdev's codes for a mouse's buttons (`linux/input-event-codes.h`).
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;
const BTN_SIDE: u32 = 0x113;
const BTN_EXTRA: u32 = 0x114;
const BTN_FORWARD: u32 = 0x115;
const BTN_BACK: u32 = 0x116;

/// XKB keycodes are evdev's plus 8, and a keymap has none past 767.
const EVDEV_OFFSET: u32 = 8;
const MAX_KEYCODE: u32 = 767;

/// `XF86Switch_VT_1`; the twelve are consecutive.
const SWITCH_VT_1: u32 = 0x1008_fe01;
const SWITCH_VTS: u32 = 12;

/// A wheel's notch in libinput's scroll value, degrees.
const WHEEL_NOTCH: f64 = 15.0;
/// A touchpad's scroll value to GDK's scroll delta, as GDK divides it.
const FINGER_SCROLL: f64 = 10.0;

/// Fingers remembered at once: libinput's slots are few, and one past
/// these is not a finger it would number.
const MAX_SLOTS: usize = 64;

/// libinput's opens and closes, through the session.
struct Opener {
    session: Rc<RefCell<Session<LibseatBackend>>>,
    /// The descriptors handed out, and the node each is of.
    open: Vec<(RawFd, PathBuf)>,
}

/// `EACCES`, negated as libinput takes an errno.
const REFUSED: i32 = -13;

impl LibinputInterface for Opener {
    fn open_restricted(&mut self, path: &Path, _flags: i32) -> Result<OwnedFd, i32> {
        // The seat opens it as it opens every device: the flags are its.
        let device = self
            .session
            .borrow_mut()
            .take_device(path)
            .map_err(|_| REFUSED)?;
        // SAFETY: the seat holds this descriptor open until the device is
        // released, which this type does, after the duplicate is closed.
        let seats = unsafe { BorrowedFd::borrow_raw(device.fd) };
        let fd = rustix::io::fcntl_dupfd_cloexec(seats, 3).map_err(|_| REFUSED)?;
        self.open.push((fd.as_raw_fd(), path.to_path_buf()));
        Ok(fd)
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        let raw = fd.as_raw_fd();
        drop(fd);
        if let Some(i) = self.open.iter().position(|(open, _)| *open == raw) {
            let (_, path) = self.open.swap_remove(i);
            // The seat has one open device for a node however often
            // libinput opens it: given back with the last of them.
            if self.open.iter().any(|(_, other)| *other == path) {
                return;
            }
            // Refused when the session is paused and the seat has closed
            // it already: either way it is closed.
            let _ = self.session.borrow_mut().release_device(path);
        }
    }
}

/// What a batch of events asks of the seat itself.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Asked {
    /// A terminal to switch to: Ctrl+Alt and a function key.
    pub(crate) switch_to: Option<i32>,
}

/// A seat's devices, and the state the embedder keeps for them.
pub(crate) struct Devices {
    input: drmkit_input::Seat,
    repeater: KeyRepeater,
    state: State,
    sink: Arc<dyn EventSink>,
    events: Vec<InputEvent>,
    repeats: Vec<drmkit_input::KeyEvent>,
}

/// Everything but the devices themselves: what turns their events into
/// the framework's, which a test drives with no device.
pub(crate) struct State {
    keymap: XkbKeymap,
    keys: Keys,
    mouse: Mouse,
    scrolling: Scrolling,
    touches: Touches,
    /// Where the pointer is, in the view's pixels.
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
    /// Whether the mouse has been told to the framework as in the view.
    entered: bool,
    /// A touchpad's two-finger scroll is under way.
    panning: bool,
    /// Each finger down, by its slot, and where it last was: an up does
    /// not say.
    fingers: Vec<(u32, f64, f64)>,
    /// Keys that went down as the seat's, a terminal switch: their ups
    /// are the seat's too.
    seat_keys: Vec<u16>,
    out: Vec<PointerEvent>,
}

impl State {
    pub(crate) fn new(config: &DevicesConfig) -> Result<State, String> {
        let keymap =
            XkbKeymap::from_layout(&config.layout, &config.variant).map_err(|e| e.to_string())?;
        let (width, height) = (
            f64::from(config.width.max(1)),
            f64::from(config.height.max(1)),
        );
        Ok(State {
            keymap,
            keys: Keys::new(),
            mouse: Mouse::new(VIEW),
            scrolling: Scrolling::new(VIEW),
            touches: Touches::new(VIEW),
            // A pointer starts in the middle, as a compositor's does.
            x: (width / 2.0).floor(),
            y: (height / 2.0).floor(),
            width,
            height,
            scale: if config.scale.is_finite() && config.scale > 0.0 {
                config.scale
            } else {
                1.0
            },
            entered: false,
            panning: false,
            fingers: Vec::new(),
            seat_keys: Vec::new(),
            out: Vec::new(),
        })
    }

    /// Where the pointer is.
    #[cfg(test)]
    pub(crate) fn pointer(&self) -> (f64, f64) {
        (self.x, self.y)
    }

    /// A position in the view, whatever a device said.
    fn hold(&self, x: f64, y: f64) -> (f64, f64) {
        let within = |v: f64, limit: f64| {
            if v.is_finite() {
                v.clamp(0.0, limit - 1.0)
            } else {
                0.0
            }
        };
        (within(x, self.width), within(y, self.height))
    }

    fn deliver(&mut self, sink: &dyn EventSink) {
        // Each event a packet of its own, as the embedder sends them.
        for event in self.out.drain(..) {
            sink.pointer_events(&[event]);
        }
    }

    /// One key going down or coming up. A key that switches terminals is
    /// the seat's, and the framework is not told of it.
    fn key(&mut self, sink: &dyn EventSink, evdev: u32, pressed: bool, asked: &mut Asked) {
        let Some(keycode) = evdev
            .checked_add(EVDEV_OFFSET)
            .filter(|code| *code <= MAX_KEYCODE)
            .and_then(|code| u16::try_from(code).ok())
        else {
            return;
        };
        let key = self.keymap.key(sink.now(), keycode, pressed);
        if pressed && (SWITCH_VT_1..SWITCH_VT_1 + SWITCH_VTS).contains(&key.keyval) {
            asked.switch_to = i32::try_from(key.keyval - SWITCH_VT_1 + 1).ok();
            if !self.seat_keys.contains(&keycode) {
                self.seat_keys.push(keycode);
            }
            return;
        }
        // Its up, by which time the modifiers that made it the seat's may
        // be up themselves, and its repeats.
        if let Some(i) = self.seat_keys.iter().position(|k| *k == keycode) {
            if !pressed {
                self.seat_keys.swap_remove(i);
            }
            return;
        }
        let keymap = &self.keymap;
        let event = self.keys.handle(&key, |c, g, l| keymap.lookup(c, g, l));
        sink.key_event(event);
    }

    fn moved(&mut self, sink: &dyn EventSink, x: f64, y: f64) {
        (self.x, self.y) = self.hold(x, y);
        let at = At::mouse(sink.now(), self.x, self.y);
        self.entered = true;
        self.mouse.motion(&at, &mut self.out);
        self.scrolling.set_last_mouse_position(self.x, self.y);
    }

    fn pointer_event(&mut self, sink: &dyn EventSink, event: &Pointer) {
        match event {
            Pointer::Motion(m) => {
                let (dx, dy) = (finite(m.dx), finite(m.dy));
                self.moved(sink, self.x + dx, self.y + dy);
            }
            // An extent of 1 by 1, onto the view's.
            Pointer::Absolute(a) => self.moved(sink, a.x * self.width, a.y * self.height),
            Pointer::Button(b) => {
                let gdk = match b.button {
                    BTN_LEFT => button::PRIMARY,
                    BTN_MIDDLE => button::MIDDLE,
                    BTN_RIGHT => button::SECONDARY,
                    BTN_SIDE | BTN_BACK => button::BACK,
                    BTN_EXTRA | BTN_FORWARD => button::FORWARD,
                    _ => return,
                };
                let at = At::mouse(sink.now(), self.x, self.y);
                self.entered = true;
                if b.pressed {
                    self.mouse.button_press(&at, gdk, &mut self.out);
                } else {
                    self.mouse.button_release(&at, gdk, &mut self.out);
                }
            }
            Pointer::Axis(axis) => {
                let (h, v) = (finite(axis.horizontal), finite(axis.vertical));
                let touchpad = !matches!(axis.source, ScrollSource::Wheel);
                let unit = if touchpad { FINGER_SCROLL } else { WHEEL_NOTCH };
                // The fingers lifted: libinput says so with a scroll of
                // nothing.
                let stop = touchpad && h == 0.0 && v == 0.0;
                if stop && !self.panning {
                    return;
                }
                self.panning = touchpad && !stop;
                let scroll = Scroll {
                    time: sink.now(),
                    x: self.x,
                    y: self.y,
                    delta_x: h / unit,
                    delta_y: v / unit,
                    touchpad,
                    stop,
                };
                self.scrolling.scroll(&scroll, self.scale, &mut self.out);
            }
            _ => {}
        }
    }

    fn touch(&mut self, sink: &dyn EventSink, event: &drmkit_input::TouchEvent) {
        // A frame ends a batch and is no finger's; a slot libinput did
        // not number is none either.
        let Ok(slot) = u32::try_from(event.slot) else {
            return;
        };
        let known = self.fingers.iter().position(|(s, _, _)| *s == slot);
        let now = sink.now();
        match event.phase {
            TouchPhase::Down | TouchPhase::Motion => {
                let (x, y) = self.hold(event.x * self.width, event.y * self.height);
                let action = match known {
                    Some(i) => {
                        self.fingers[i] = (slot, x, y);
                        TouchAction::Update
                    }
                    // A motion with no down before it is not a finger.
                    None if event.phase == TouchPhase::Motion => return,
                    None if self.fingers.len() >= MAX_SLOTS => return,
                    None => {
                        self.fingers.push((slot, x, y));
                        TouchAction::Begin
                    }
                };
                self.touches.touch(now, action, slot, x, y, &mut self.out);
            }
            TouchPhase::Up | TouchPhase::Cancel => {
                let Some(i) = known else {
                    return;
                };
                let (_, x, y) = self.fingers.swap_remove(i);
                // A cancel is a device unplugged with a finger on it.
                let action = if event.phase == TouchPhase::Up {
                    TouchAction::End
                } else {
                    TouchAction::Cancel
                };
                self.touches.touch(now, action, slot, x, y, &mut self.out);
            }
            _ => {}
        }
    }

    pub(crate) fn event(&mut self, sink: &dyn EventSink, event: &InputEvent, asked: &mut Asked) {
        match event {
            InputEvent::Key(key) => self.key(sink, key.key, key.pressed, asked),
            InputEvent::Pointer(pointer) => self.pointer_event(sink, pointer),
            InputEvent::Touch(touch) => self.touch(sink, touch),
            _ => {}
        }
        self.deliver(sink);
    }

    /// The devices are going, with the session or for good: what is down
    /// comes up, so the framework is left holding nothing.
    pub(crate) fn release_all(&mut self, sink: &dyn EventSink) {
        let mut asked = Asked::default();
        for keycode in self.keymap.held() {
            self.key(
                sink,
                u32::from(keycode).saturating_sub(EVDEV_OFFSET),
                false,
                &mut asked,
            );
        }
        let now = sink.now();
        for (slot, x, y) in std::mem::take(&mut self.fingers) {
            self.touches
                .touch(now, TouchAction::Cancel, slot, x, y, &mut self.out);
        }
        if self.panning {
            self.panning = false;
            let scroll = Scroll {
                time: now,
                x: self.x,
                y: self.y,
                delta_x: 0.0,
                delta_y: 0.0,
                touchpad: true,
                stop: true,
            };
            self.scrolling.scroll(&scroll, self.scale, &mut self.out);
        }
        if self.entered {
            self.entered = false;
            let at = At::mouse(now, self.x, self.y);
            self.mouse.leave(&at, &mut self.out);
        }
        self.deliver(sink);
    }
}

fn finite(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

impl Devices {
    /// The devices of the session's seat, opened through it.
    pub(crate) fn open(
        session: &Rc<RefCell<Session<LibseatBackend>>>,
        seat: &str,
        config: &DevicesConfig,
        sink: Arc<dyn EventSink>,
    ) -> Result<Devices, String> {
        let state = State::new(config)?;
        let opener = Opener {
            session: Rc::clone(session),
            open: Vec::new(),
        };
        let options = SeatOptions {
            seat: seat.to_owned(),
            ..SeatOptions::default()
        };
        let input =
            drmkit_input::Seat::open_with(&options, opener).map_err(|e| format!("devices: {e}"))?;
        let repeater =
            KeyRepeater::new(RepeatConfig::default()).map_err(|e| format!("key repeat: {e}"))?;
        Ok(Devices {
            input,
            repeater,
            state,
            sink,
            events: Vec::new(),
            repeats: Vec::new(),
        })
    }

    /// Readable when a device has something to say.
    pub(crate) fn fd(&self) -> BorrowedFd<'_> {
        self.input.as_fd()
    }

    /// Readable when a held key is due to repeat.
    pub(crate) fn repeat_fd(&self) -> BorrowedFd<'_> {
        self.repeater.fd()
    }

    /// Whether the loop these are read for has ended.
    pub(crate) fn is_closed(&self) -> bool {
        self.sink.is_closed()
    }

    /// Reads what the devices have, and delivers it.
    pub(crate) fn dispatch(&mut self) -> Asked {
        let mut asked = Asked::default();
        self.events.clear();
        if self.input.dispatch(&mut self.events).is_err() {
            return asked;
        }
        for event in &self.events {
            if let InputEvent::Key(key) = event {
                let repeats = key
                    .key
                    .checked_add(EVDEV_OFFSET)
                    .and_then(|code| u16::try_from(code).ok())
                    .is_some_and(|code| self.state.keymap.repeats(code));
                self.repeater.on_key(key, repeats);
            }
            self.state.event(self.sink.as_ref(), event, &mut asked);
        }
        asked
    }

    /// A held key's repeats that are due: each a down for a key that is
    /// down, which is what a repeat is to the embedder.
    pub(crate) fn repeat(&mut self) {
        self.repeats.clear();
        if self.repeater.dispatch(0, &mut self.repeats).is_err() {
            return;
        }
        let mut asked = Asked::default();
        for key in &self.repeats {
            self.state
                .key(self.sink.as_ref(), key.key, true, &mut asked);
        }
    }

    /// The session is paused: the devices are closed, and what was down
    /// is let go.
    pub(crate) fn pause(&mut self) {
        self.repeater.cancel();
        self.state.release_all(self.sink.as_ref());
        self.input.suspend();
    }

    /// The session is back: the devices are opened again.
    pub(crate) fn resume(&mut self) {
        let _ = self.input.resume();
    }
}

impl Drop for Devices {
    fn drop(&mut self) {
        self.repeater.cancel();
        self.state.release_all(self.sink.as_ref());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use drmkit_input::{PointerAxis, PointerButton, PointerMotion, TouchEvent};
    use fl_input::xkb::keycode_named;
    use fl_input::{KeyEvent, PointerPhase, PointerSignalKind};

    use super::*;

    /// A loop that keeps what it is given.
    #[derive(Default)]
    struct Kept {
        pointers: Mutex<Vec<PointerEvent>>,
        keys: Mutex<Vec<KeyEvent>>,
    }

    impl EventSink for Kept {
        fn now(&self) -> Duration {
            Duration::from_millis(5)
        }

        fn pointer_events(&self, events: &[PointerEvent]) {
            // A packet for each event.
            assert_eq!(events.len(), 1);
            self.pointers.lock().unwrap().extend_from_slice(events);
        }

        fn key_event(&self, event: KeyEvent) {
            self.keys.lock().unwrap().push(event);
        }

        fn wait_idle(&self) -> bool {
            true
        }

        fn is_closed(&self) -> bool {
            false
        }
    }

    impl Kept {
        fn phases(&self) -> Vec<PointerPhase> {
            let mut kept = self.pointers.lock().unwrap();
            kept.drain(..).map(|e| e.phase).collect()
        }

        fn taken(&self) -> Vec<PointerEvent> {
            std::mem::take(&mut *self.pointers.lock().unwrap())
        }

        /// The keys delivered, each as whether it went down.
        fn keys(&self) -> Vec<bool> {
            let mut kept = self.keys.lock().unwrap();
            kept.drain(..)
                .map(|e| e.native.is_some_and(|k| k.pressed))
                .collect()
        }
    }

    fn state() -> State {
        State::new(&DevicesConfig {
            layout: "us".into(),
            variant: String::new(),
            width: 800,
            height: 600,
            scale: 2.0,
        })
        .unwrap()
    }

    fn evdev(name: &str) -> u32 {
        u32::from(keycode_named(name).unwrap()) - EVDEV_OFFSET
    }

    fn motion(dx: f64, dy: f64) -> InputEvent {
        InputEvent::Pointer(Pointer::Motion(PointerMotion {
            time_ms: 0,
            dx,
            dy,
            device: None,
        }))
    }

    fn pressed(button: u32, pressed: bool) -> InputEvent {
        InputEvent::Pointer(Pointer::Button(PointerButton {
            time_ms: 0,
            button,
            pressed,
        }))
    }

    fn axis(horizontal: f64, vertical: f64, source: ScrollSource) -> InputEvent {
        InputEvent::Pointer(Pointer::Axis(PointerAxis {
            time_ms: 0,
            horizontal,
            vertical,
            source,
        }))
    }

    fn touch(slot: i32, x: f64, y: f64, phase: TouchPhase) -> InputEvent {
        InputEvent::Touch(TouchEvent {
            time_ms: 0,
            slot,
            x,
            y,
            phase,
            device: None,
        })
    }

    fn key(name: &str, down: bool) -> InputEvent {
        InputEvent::Key(drmkit_input::KeyEvent {
            key: evdev(name),
            pressed: down,
            ..drmkit_input::KeyEvent::default()
        })
    }

    #[test]
    fn a_mouse_moves_from_the_middle_and_stays_in_the_view() {
        let (mut s, sink, mut asked) = (state(), Kept::default(), Asked::default());
        assert_eq!(s.pointer(), (400.0, 300.0));
        s.event(&sink, &motion(10.0, -5.0), &mut asked);
        assert_eq!(s.pointer(), (410.0, 295.0));
        // Added where it is, then hovering.
        let events = sink.taken();
        assert_eq!(events.first().map(|e| e.phase), Some(PointerPhase::Add));
        assert_eq!(events.last().map(|e| (e.x, e.y)), Some((410.0, 295.0)));
        // Past every edge, and a device that says nothing sensible.
        s.event(&sink, &motion(1e9, 1e9), &mut asked);
        assert_eq!(s.pointer(), (799.0, 599.0));
        s.event(&sink, &motion(-1e9, f64::NAN), &mut asked);
        assert_eq!(s.pointer(), (0.0, 599.0));
        s.event(&sink, &motion(f64::INFINITY, -1e300), &mut asked);
        assert_eq!(s.pointer(), (0.0, 0.0));
    }

    #[test]
    fn a_mouse_s_buttons_are_the_embedder_s() {
        let (mut s, sink, mut asked) = (state(), Kept::default(), Asked::default());
        s.event(&sink, &motion(1.0, 1.0), &mut asked);
        sink.taken();
        s.event(&sink, &pressed(BTN_LEFT, true), &mut asked);
        s.event(&sink, &motion(3.0, 0.0), &mut asked);
        s.event(&sink, &pressed(BTN_LEFT, false), &mut asked);
        assert_eq!(
            sink.phases(),
            [PointerPhase::Down, PointerPhase::Move, PointerPhase::Up]
        );
        // A button evdev has and a mouse does not is no event.
        s.event(&sink, &pressed(0x120, true), &mut asked);
        assert!(sink.taken().is_empty());
    }

    #[test]
    fn a_wheel_s_notch_scrolls_and_a_touchpad_s_fingers_pan() {
        let (mut s, sink, mut asked) = (state(), Kept::default(), Asked::default());
        // One notch down: 53 logical pixels, at a scale of 2.
        s.event(&sink, &axis(0.0, 15.0, ScrollSource::Wheel), &mut asked);
        let wheel = sink.taken();
        assert_eq!(wheel.len(), 1);
        assert_eq!(wheel[0].signal_kind, PointerSignalKind::Scroll);
        assert_eq!(wheel[0].scroll_delta_y, 106.0);
        // Two fingers: a pan begun, moved, and ended when they lift.
        s.event(&sink, &axis(0.0, 20.0, ScrollSource::Finger), &mut asked);
        s.event(&sink, &axis(0.0, 10.0, ScrollSource::Finger), &mut asked);
        s.event(&sink, &axis(0.0, 0.0, ScrollSource::Finger), &mut asked);
        assert_eq!(
            sink.phases(),
            [
                PointerPhase::PanZoomStart,
                PointerPhase::PanZoomUpdate,
                PointerPhase::PanZoomUpdate,
                PointerPhase::PanZoomEnd
            ]
        );
        // A lift with no pan before it ends nothing.
        s.event(&sink, &axis(0.0, 0.0, ScrollSource::Finger), &mut asked);
        assert!(sink.taken().is_empty());
    }

    #[test]
    fn a_finger_is_a_pointer_from_its_down_to_its_up() {
        let (mut s, sink, mut asked) = (state(), Kept::default(), Asked::default());
        // A motion with no down, and a frame, are no finger's.
        s.event(&sink, &touch(0, 0.5, 0.5, TouchPhase::Motion), &mut asked);
        s.event(&sink, &touch(-1, 0.0, 0.0, TouchPhase::Frame), &mut asked);
        assert!(sink.taken().is_empty());
        s.event(&sink, &touch(0, 0.25, 0.5, TouchPhase::Down), &mut asked);
        s.event(&sink, &touch(0, 0.5, 0.5, TouchPhase::Motion), &mut asked);
        // An up says no position: it is where the finger last was.
        s.event(&sink, &touch(0, 0.0, 0.0, TouchPhase::Up), &mut asked);
        let events = sink.taken();
        let phases: Vec<_> = events.iter().map(|e| e.phase).collect();
        assert_eq!(
            phases,
            [
                PointerPhase::Add,
                PointerPhase::Down,
                PointerPhase::Move,
                PointerPhase::Up,
                PointerPhase::Remove
            ]
        );
        assert_eq!((events[1].x, events[1].y), (200.0, 300.0));
        assert_eq!((events[3].x, events[3].y), (400.0, 300.0));
        // Positions outside the screen are held to it.
        s.event(&sink, &touch(1, 7.0, -3.0, TouchPhase::Down), &mut asked);
        let down = sink.taken();
        assert_eq!((down[1].x, down[1].y), (799.0, 0.0));
        // A device unplugged under a finger: given up, not lifted.
        s.event(&sink, &touch(1, 0.0, 0.0, TouchPhase::Cancel), &mut asked);
        let phases: Vec<_> = sink.taken().iter().map(|e| e.phase).collect();
        assert_eq!(phases, [PointerPhase::Cancel, PointerPhase::Remove]);
        // No more fingers are remembered than a device has slots for.
        for slot in 1..200 {
            s.event(&sink, &touch(slot, 0.5, 0.5, TouchPhase::Down), &mut asked);
        }
        assert_eq!(s.fingers.len(), MAX_SLOTS);
    }

    #[test]
    fn keys_reach_the_framework_and_the_terminal_keys_reach_the_seat() {
        let (mut s, sink, mut asked) = (state(), Kept::default(), Asked::default());
        s.event(&sink, &key("keyA", true), &mut asked);
        s.event(&sink, &key("keyA", false), &mut asked);
        assert_eq!(sink.keys(), [true, false]);
        assert_eq!(asked, Asked::default());
        // Ctrl+Alt+F2: the modifiers are the framework's, F2 the seat's.
        s.event(&sink, &key("controlLeft", true), &mut asked);
        s.event(&sink, &key("altLeft", true), &mut asked);
        s.event(&sink, &key("f2", true), &mut asked);
        assert_eq!(asked.switch_to, Some(2));
        s.event(&sink, &key("f2", false), &mut asked);
        assert_eq!(sink.keys(), [true, true]);
        // The session goes with F3 still down, as it does: the modifiers
        // come up, and F3, with nothing held by then, is still the seat's.
        s.event(&sink, &key("f3", true), &mut asked);
        assert_eq!(asked.switch_to, Some(3));
        s.release_all(&sink);
        assert_eq!(sink.keys(), [false, false]);
        assert!(s.seat_keys.is_empty());
        // A code no keymap has is dropped.
        let none = InputEvent::Key(drmkit_input::KeyEvent {
            key: u32::MAX,
            pressed: true,
            ..drmkit_input::KeyEvent::default()
        });
        s.event(&sink, &none, &mut asked);
        s.event(
            &sink,
            &InputEvent::Key(drmkit_input::KeyEvent {
                key: 5000,
                pressed: true,
                ..drmkit_input::KeyEvent::default()
            }),
            &mut asked,
        );
        assert!(sink.keys().is_empty());
    }

    #[test]
    fn what_is_down_comes_up_when_the_devices_go() {
        let (mut s, sink, mut asked) = (state(), Kept::default(), Asked::default());
        s.event(&sink, &key("shiftLeft", true), &mut asked);
        s.event(&sink, &key("keyB", true), &mut asked);
        s.event(&sink, &motion(1.0, 1.0), &mut asked);
        s.event(&sink, &touch(3, 0.5, 0.5, TouchPhase::Down), &mut asked);
        s.event(&sink, &axis(4.0, 0.0, ScrollSource::Finger), &mut asked);
        sink.taken();
        sink.keys();
        s.release_all(&sink);
        // Both keys up; the finger given up, the pan ended, the mouse gone.
        assert_eq!(sink.keys(), [false, false]);
        assert_eq!(
            sink.phases(),
            [
                PointerPhase::Cancel,
                PointerPhase::Remove,
                PointerPhase::PanZoomEnd,
                PointerPhase::Remove
            ]
        );
        // And nothing is left to let go a second time.
        s.release_all(&sink);
        assert!(sink.keys().is_empty() && sink.taken().is_empty());
    }
}
