//! Pointers from a mouse, a wheel, a touchpad and a touch screen, made
//! into what the framework is told (brief M4 §2, D1): the Linux embedder's
//! `fl_pointer_manager`, `fl_scrolling_manager` and `fl_touch_manager`
//! below GDK, ported. Every source of such devices shares them: the
//! script, the boards' devices, later a Wayland seat.
//!
//! A source says what a device did, with positions in physical pixels of
//! the view, and gets [`PointerEvent`]s to deliver. What the engine's
//! converter then synthesizes (fl-ui) is the same for every source.

use std::time::Duration;

use crate::{mouse_button, PointerDeviceKind, PointerEvent, PointerPhase, PointerSignalKind};

/// The mouse's device (`kMousePointerDeviceId`).
pub const MOUSE_DEVICE: i32 = 0;
/// The device a touchpad's gestures are sent as (`kPointerPanZoomDeviceId`).
pub const PAN_ZOOM_DEVICE: i32 = 1;
/// One past the highest id a touch is given (`kMaxTouchDeviceId`).
pub const MAX_TOUCHES: u32 = 128;
/// A touch's device is its id under these bits: the embedder's
/// `kFlutterPointerDeviceKindTouch << 28`.
const TOUCH_DEVICE_BASE: i32 = 2 << 28;

/// Pixels a wheel's notch scrolls, before the view's scale
/// (`kScrollOffsetMultiplier`).
pub const SCROLL_OFFSET_MULTIPLIER: f64 = 53.0;

/// A mouse's buttons, numbered as GDK numbers them.
pub mod button {
    pub const PRIMARY: u32 = 1;
    pub const MIDDLE: u32 = 2;
    pub const SECONDARY: u32 = 3;
    pub const BACK: u32 = 8;
    pub const FORWARD: u32 = 9;
}

/// `FlutterPointerEvent::buttons` of a stylus.
pub mod stylus_button {
    pub const CONTACT: i64 = 1 << 0;
    pub const PRIMARY: i64 = 1 << 1;
    pub const SECONDARY: i64 = 1 << 2;
}

/// Where and when a pointing device did something, and what kind it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct At {
    pub time: Duration,
    pub kind: PointerDeviceKind,
    pub x: f64,
    pub y: f64,
    /// A stylus's rotation, in degrees as GDK reports it.
    pub rotation: f64,
    /// A stylus's pressure, 0 to 1.
    pub pressure: f64,
}

impl At {
    /// A mouse at a position.
    pub fn mouse(time: Duration, x: f64, y: f64) -> At {
        At {
            time,
            kind: PointerDeviceKind::Mouse,
            x,
            y,
            rotation: 0.0,
            pressure: 0.0,
        }
    }
}

/// `fl_pointer_manager`: a mouse or a stylus, its buttons, and whether
/// the framework knows of it.
#[derive(Debug, Clone, Default)]
pub struct Mouse {
    view_id: i64,
    inside: bool,
    buttons: i64,
}

impl Mouse {
    pub fn new(view_id: i64) -> Mouse {
        Mouse {
            view_id,
            ..Mouse::default()
        }
    }

    /// The bit of a GDK button for a kind of device (`get_button`).
    fn button(kind: PointerDeviceKind, gdk_button: u32) -> Option<i64> {
        if matches!(
            kind,
            PointerDeviceKind::Stylus | PointerDeviceKind::InvertedStylus
        ) {
            return match gdk_button {
                button::PRIMARY => Some(stylus_button::CONTACT),
                button::SECONDARY => Some(stylus_button::PRIMARY),
                button::MIDDLE => Some(stylus_button::SECONDARY),
                _ => None,
            };
        }
        match gdk_button {
            button::PRIMARY => Some(mouse_button::PRIMARY),
            button::MIDDLE => Some(mouse_button::MIDDLE),
            button::SECONDARY => Some(mouse_button::SECONDARY),
            button::BACK => Some(mouse_button::BACK),
            button::FORWARD => Some(mouse_button::FORWARD),
            _ => None,
        }
    }

    /// `fl_engine_send_mouse_pointer_event`.
    fn event(&self, phase: PointerPhase, at: &At) -> PointerEvent {
        PointerEvent {
            device: MOUSE_DEVICE,
            buttons: self.buttons,
            view_id: self.view_id,
            pressure: at.pressure,
            pressure_min: 0.0,
            pressure_max: 1.0,
            rotation: at.rotation.to_radians(),
            ..PointerEvent::new(phase, at.kind, at.time, at.x, at.y)
        }
    }

    fn ensure_added(&mut self, at: &At, out: &mut Vec<PointerEvent>) {
        if !self.inside {
            self.inside = true;
            out.push(self.event(PointerPhase::Add, at));
        }
    }

    /// A button went down. `false` for a button the device has not, or
    /// one down already.
    pub fn button_press(&mut self, at: &At, gdk_button: u32, out: &mut Vec<PointerEvent>) -> bool {
        let Some(button) = Mouse::button(at.kind, gdk_button) else {
            return false;
        };
        self.ensure_added(at, out);
        if self.buttons & button != 0 {
            return false;
        }
        let phase = if self.buttons == 0 {
            PointerPhase::Down
        } else {
            PointerPhase::Move
        };
        self.buttons ^= button;
        out.push(self.event(phase, at));
        true
    }

    /// A button came up. `false` for one that was not down.
    pub fn button_release(
        &mut self,
        at: &At,
        gdk_button: u32,
        out: &mut Vec<PointerEvent>,
    ) -> bool {
        let Some(button) = Mouse::button(at.kind, gdk_button) else {
            return false;
        };
        if self.buttons & button == 0 {
            return false;
        }
        self.buttons ^= button;
        let phase = if self.buttons == 0 {
            PointerPhase::Up
        } else {
            PointerPhase::Move
        };
        out.push(self.event(phase, at));
        true
    }

    /// The pointer moved: a move with a button down, a hover with none.
    pub fn motion(&mut self, at: &At, out: &mut Vec<PointerEvent>) {
        self.ensure_added(at, out);
        let phase = if self.buttons != 0 {
            PointerPhase::Move
        } else {
            PointerPhase::Hover
        };
        out.push(self.event(phase, at));
    }

    /// The pointer came over the view.
    pub fn enter(&mut self, at: &At, out: &mut Vec<PointerEvent>) {
        self.ensure_added(at, out);
    }

    /// The pointer left the view. With a button down it is kept: the
    /// drag goes on outside.
    pub fn leave(&mut self, at: &At, out: &mut Vec<PointerEvent>) {
        if self.inside && self.buttons == 0 {
            out.push(self.event(PointerPhase::Remove, at));
            self.inside = false;
        }
    }
}

/// One scroll event, as GDK reports one: where the pointer is, and how
/// far, a notch of a wheel being 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scroll {
    pub time: Duration,
    pub x: f64,
    pub y: f64,
    pub delta_x: f64,
    pub delta_y: f64,
    /// From a touchpad: two fingers moved, which pans.
    pub touchpad: bool,
    /// The fingers lifted: the touchpad's scroll is over.
    pub stop: bool,
}

/// `fl_scrolling_manager`: a wheel's scrolls, and a touchpad's pan, zoom
/// and rotation, which the framework is told as one gesture.
#[derive(Debug, Clone, Default)]
pub struct Scrolling {
    view_id: i64,
    last_x: f64,
    last_y: f64,
    pan_started: bool,
    pan_x: f64,
    pan_y: f64,
    zoom_started: bool,
    rotate_started: bool,
    scale: f64,
    rotation: f64,
}

impl Scrolling {
    pub fn new(view_id: i64) -> Scrolling {
        Scrolling {
            view_id,
            ..Scrolling::default()
        }
    }

    /// Where the mouse last was, which is where a zoom or a rotation is
    /// said to be.
    pub fn set_last_mouse_position(&mut self, x: f64, y: f64) {
        self.last_x = x;
        self.last_y = y;
    }

    /// `fl_engine_send_pointer_pan_zoom_event`.
    fn pan_zoom(
        &self,
        phase: PointerPhase,
        time: Duration,
        (x, y): (f64, f64),
        pan: (f64, f64),
        scale: f64,
        rotation: f64,
    ) -> PointerEvent {
        PointerEvent {
            device: PAN_ZOOM_DEVICE,
            view_id: self.view_id,
            pan_x: pan.0,
            pan_y: pan.1,
            scale,
            rotation,
            ..PointerEvent::new(phase, PointerDeviceKind::Trackpad, time, x, y)
        }
    }

    /// A scroll event. `scale` is the view's pixels to a logical one: a
    /// notch scrolls [`SCROLL_OFFSET_MULTIPLIER`] logical pixels.
    pub fn scroll(&mut self, scroll: &Scroll, scale: f64, out: &mut Vec<PointerEvent>) {
        let mut dx = scroll.delta_x * SCROLL_OFFSET_MULTIPLIER * scale;
        let mut dy = scroll.delta_y * SCROLL_OFFSET_MULTIPLIER * scale;
        let at = (scroll.x, scroll.y);
        if scroll.touchpad {
            // A pan follows the fingers, the other way from a scroll.
            dx = -dx;
            dy = -dy;
            if scroll.stop {
                let pan = (self.pan_x, self.pan_y);
                out.push(self.pan_zoom(PointerPhase::PanZoomEnd, scroll.time, at, pan, 0.0, 0.0));
                self.pan_started = false;
            } else {
                if !self.pan_started {
                    self.pan_x = 0.0;
                    self.pan_y = 0.0;
                    out.push(self.pan_zoom(
                        PointerPhase::PanZoomStart,
                        scroll.time,
                        at,
                        (0.0, 0.0),
                        0.0,
                        0.0,
                    ));
                    self.pan_started = true;
                }
                self.pan_x += dx;
                self.pan_y += dy;
                let pan = (self.pan_x, self.pan_y);
                out.push(self.pan_zoom(
                    PointerPhase::PanZoomUpdate,
                    scroll.time,
                    at,
                    pan,
                    1.0,
                    0.0,
                ));
            }
        } else {
            self.last_x = scroll.x;
            self.last_y = scroll.y;
            // The phase is not read: a signal is no change of phase.
            let mut event = PointerEvent {
                device: MOUSE_DEVICE,
                view_id: self.view_id,
                scroll_delta_x: dx,
                scroll_delta_y: dy,
                pressure_max: 1.0,
                ..PointerEvent::new(
                    PointerPhase::Move,
                    PointerDeviceKind::Mouse,
                    scroll.time,
                    scroll.x,
                    scroll.y,
                )
            };
            if dx != 0.0 || dy != 0.0 {
                event.signal_kind = PointerSignalKind::Scroll;
            }
            out.push(event);
        }
    }

    fn at_mouse(
        &self,
        phase: PointerPhase,
        time: Duration,
        scale: f64,
        rotation: f64,
    ) -> PointerEvent {
        self.pan_zoom(
            phase,
            time,
            (self.last_x, self.last_y),
            (0.0, 0.0),
            scale,
            rotation,
        )
    }

    /// Two fingers began to turn.
    pub fn rotation_begin(&mut self, time: Duration, out: &mut Vec<PointerEvent>) {
        self.rotate_started = true;
        if !self.zoom_started {
            self.scale = 1.0;
            self.rotation = 0.0;
            out.push(self.at_mouse(PointerPhase::PanZoomStart, time, 0.0, 0.0));
        }
    }

    /// They have turned `rotation` radians since they began.
    pub fn rotation_update(&mut self, time: Duration, rotation: f64, out: &mut Vec<PointerEvent>) {
        self.rotation = rotation;
        out.push(self.at_mouse(PointerPhase::PanZoomUpdate, time, self.scale, self.rotation));
    }

    pub fn rotation_end(&mut self, time: Duration, out: &mut Vec<PointerEvent>) {
        self.rotate_started = false;
        if !self.zoom_started {
            out.push(self.at_mouse(PointerPhase::PanZoomEnd, time, 0.0, 0.0));
        }
    }

    /// Two fingers began to pinch.
    pub fn zoom_begin(&mut self, time: Duration, out: &mut Vec<PointerEvent>) {
        self.zoom_started = true;
        if !self.rotate_started {
            self.scale = 1.0;
            self.rotation = 0.0;
            out.push(self.at_mouse(PointerPhase::PanZoomStart, time, 0.0, 0.0));
        }
    }

    /// They are `scale` times as far apart as when they began.
    pub fn zoom_update(&mut self, time: Duration, scale: f64, out: &mut Vec<PointerEvent>) {
        self.scale = scale;
        out.push(self.at_mouse(PointerPhase::PanZoomUpdate, time, self.scale, self.rotation));
    }

    pub fn zoom_end(&mut self, time: Duration, out: &mut Vec<PointerEvent>) {
        self.zoom_started = false;
        if !self.rotate_started {
            out.push(self.at_mouse(PointerPhase::PanZoomEnd, time, 0.0, 0.0));
        }
    }
}

/// What a finger on a touch screen did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchAction {
    Begin,
    Update,
    End,
}

/// `fl_touch_manager`: the fingers on a touch screen, each a pointer of
/// its own while it is down.
#[derive(Debug, Clone, Default)]
pub struct Touches {
    view_id: i64,
    /// A source's number for each finger down, and the id given it.
    ids: Vec<(u32, u32)>,
    /// Ids the framework has been told of.
    added: Vec<u32>,
}

impl Touches {
    pub fn new(view_id: i64) -> Touches {
        Touches {
            view_id,
            ..Touches::default()
        }
    }

    /// The id of a source's finger `number`: the one it has, or the
    /// lowest none has (`get_generated_id`). Past [`MAX_TOUCHES`] fingers
    /// at once the rest share id 0, as the embedder's do, and are not
    /// remembered.
    fn id(&mut self, number: u32) -> u32 {
        if let Some((_, id)) = self.ids.iter().find(|(n, _)| *n == number) {
            return *id;
        }
        let free = (0..MAX_TOUCHES).find(|id| !self.ids.iter().any(|(_, used)| used == id));
        match free {
            Some(id) => {
                self.ids.push((number, id));
                id
            }
            None => 0,
        }
    }

    /// One finger's event; `number` is the source's name for the finger,
    /// the same from its begin to its end. A finger is added before its
    /// first event and removed with its end.
    pub fn touch(
        &mut self,
        time: Duration,
        action: TouchAction,
        number: u32,
        x: f64,
        y: f64,
        out: &mut Vec<PointerEvent>,
    ) {
        let id = self.id(number);
        let event = |phase, buttons| PointerEvent {
            device: TOUCH_DEVICE_BASE | id as i32,
            buttons,
            view_id: self.view_id,
            ..PointerEvent::new(phase, PointerDeviceKind::Touch, time, x, y)
        };
        if !self.added.contains(&id) {
            out.push(event(PointerPhase::Add, 0));
            self.added.push(id);
        }
        match action {
            TouchAction::Begin => out.push(event(PointerPhase::Down, mouse_button::PRIMARY)),
            TouchAction::Update => out.push(event(PointerPhase::Move, mouse_button::PRIMARY)),
            TouchAction::End => {
                out.push(event(PointerPhase::Up, 0));
                out.push(event(PointerPhase::Remove, 0));
                self.ids.retain(|(n, _)| *n != number);
                self.added.retain(|a| *a != id);
            }
        }
    }
}

#[cfg(test)]
#[path = "pointer_tests.rs"]
mod tests;
