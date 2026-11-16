//! fl-engine's input boundary (brief M4 §7, D8): events as data, shaped as
//! the embedder API's (`embedder.h`), and what a source of them is given to
//! deliver them through.
//!
//! No OS code and no Dart: a source (a script, the boards' devices, later a
//! Wayland seat) builds [`PointerEvent`]s on any thread and hands them to an
//! [`EventSink`], which the shell implements. Everything after the sink is
//! the same for every source.

use std::time::Duration;

/// `FlutterPointerPhase`: what a pointer did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerPhase {
    Cancel,
    /// The pointer left the surface or let its last button go.
    Up,
    /// The pointer touched the surface or pressed its first button.
    Down,
    /// The pointer moved while down.
    Move,
    /// The pointer came to be: a device appeared, a finger came near.
    Add,
    /// The pointer is gone.
    Remove,
    /// The pointer moved while up.
    Hover,
    /// A trackpad gesture began.
    PanZoomStart,
    PanZoomUpdate,
    PanZoomEnd,
}

/// `FlutterPointerDeviceKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PointerDeviceKind {
    #[default]
    Mouse,
    Touch,
    Stylus,
    Trackpad,
    InvertedStylus,
}

/// `FlutterPointerSignalKind`: an event that is no change of phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PointerSignalKind {
    #[default]
    None,
    Scroll,
    ScrollInertiaCancel,
    Scale,
}

/// `FlutterPointerMouseButtons`, for [`PointerEvent::buttons`] of a mouse.
pub mod mouse_button {
    pub const PRIMARY: i64 = 1 << 0;
    pub const SECONDARY: i64 = 1 << 1;
    pub const MIDDLE: i64 = 1 << 2;
    pub const BACK: i64 = 1 << 3;
    pub const FORWARD: i64 = 1 << 4;
}

/// `FlutterPointerEvent`, field for field but the struct's size: one thing a
/// pointer did. Positions are physical pixels in the view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerEvent {
    pub phase: PointerPhase,
    /// On the frame clock: [`EventSink::now`] when it happened.
    pub timestamp: Duration,
    pub x: f64,
    pub y: f64,
    /// The pointer's device: unique among the pointers there are at once.
    /// A mouse is one; each finger on a touch screen is its own.
    pub device: i32,
    pub signal_kind: PointerSignalKind,
    pub scroll_delta_x: f64,
    pub scroll_delta_y: f64,
    pub device_kind: PointerDeviceKind,
    /// The buttons down, [`mouse_button`]'s bits for a mouse. A touch's
    /// are not read: it is in contact while down.
    pub buttons: i64,
    pub pan_x: f64,
    pub pan_y: f64,
    pub scale: f64,
    pub rotation: f64,
    pub view_id: i64,
    pub pressure: f64,
    pub pressure_min: f64,
    pub pressure_max: f64,
}

impl PointerEvent {
    /// An event of `phase` at a position, everything else as the embedder
    /// API leaves a field unset: view 0, device 0, no buttons, no signal.
    pub fn new(
        phase: PointerPhase,
        device_kind: PointerDeviceKind,
        timestamp: Duration,
        x: f64,
        y: f64,
    ) -> PointerEvent {
        PointerEvent {
            phase,
            timestamp,
            x,
            y,
            device: 0,
            signal_kind: PointerSignalKind::None,
            scroll_delta_x: 0.0,
            scroll_delta_y: 0.0,
            device_kind,
            buttons: 0,
            pan_x: 0.0,
            pan_y: 0.0,
            scale: 0.0,
            rotation: 0.0,
            view_id: 0,
            pressure: 0.0,
            pressure_min: 0.0,
            pressure_max: 0.0,
        }
    }

    /// Whether dropping this event loses only a position on the way to the
    /// next: a move or a hover that is no signal. The converter moves a
    /// pointer to where its next event is, so the framework still sees it
    /// arrive.
    pub fn is_motion(&self) -> bool {
        self.signal_kind == PointerSignalKind::None
            && matches!(self.phase, PointerPhase::Move | PointerPhase::Hover)
    }
}

/// Where a source delivers its events: the shell's loop. Callable from any
/// thread.
pub trait EventSink: Send + Sync {
    /// Now, on the frame clock: what an event that just happened is stamped
    /// with.
    fn now(&self) -> Duration;

    /// Delivers pointer events, in order. They reach the framework together,
    /// before the next frame. Events arriving faster than they are taken
    /// are bounded by dropping motion ([`PointerEvent::is_motion`]) first.
    fn pointer_events(&self, events: &[PointerEvent]);

    /// Blocks until the loop has nothing to do: every event delivered has
    /// been dispatched, and every frame that asked for has been drawn.
    /// `false` if the loop has ended instead. A timer the app is waiting on
    /// does not count as something to do.
    fn wait_idle(&self) -> bool;

    /// Whether the loop has ended: nothing delivered from now on is seen.
    fn is_closed(&self) -> bool;
}
