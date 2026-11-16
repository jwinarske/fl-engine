//! `fl_pointer_manager_test.cc`, `fl_scrolling_manager_test.cc` and
//! `fl_touch_manager_test.cc`, case for case under their names. Times are
//! the tests' milliseconds.

use super::*;

const MOUSE: PointerDeviceKind = PointerDeviceKind::Mouse;

fn at(ms: u64, kind: PointerDeviceKind, x: f64, y: f64) -> At {
    At {
        time: Duration::from_millis(ms),
        kind,
        x,
        y,
        rotation: 0.0,
        pressure: 0.0,
    }
}

fn stylus(ms: u64, kind: PointerDeviceKind) -> At {
    At {
        pressure: 0.5,
        ..at(ms, kind, 4.0, 8.0)
    }
}

/// What the tests check of each event: timestamp in microseconds, x, y,
/// and buttons.
fn seen(events: &[PointerEvent]) -> Vec<(u128, f64, f64, i64)> {
    for e in events {
        assert_eq!((e.view_id, e.device), (42, MOUSE_DEVICE));
    }
    events
        .iter()
        .map(|e| (e.timestamp.as_micros(), e.x, e.y, e.buttons))
        .collect()
}

fn phases(events: &[PointerEvent]) -> Vec<PointerPhase> {
    events.iter().map(|e| e.phase).collect()
}

fn mouse() -> (Mouse, Vec<PointerEvent>) {
    (Mouse::new(42), Vec::new())
}

#[test]
fn enter_leave() {
    let (mut m, mut out) = mouse();
    m.enter(&at(1234, MOUSE, 1.0, 2.0), &mut out);
    m.leave(&at(1235, MOUSE, 3.0, 4.0), &mut out);
    assert_eq!(
        seen(&out),
        [(1_234_000, 1.0, 2.0, 0), (1_235_000, 3.0, 4.0, 0)]
    );
    assert_eq!(phases(&out), [PointerPhase::Add, PointerPhase::Remove]);
    assert!(out.iter().all(|e| e.device_kind == MOUSE));
}

#[test]
fn enter_enter() {
    let (mut m, mut out) = mouse();
    m.enter(&at(1234, MOUSE, 1.0, 2.0), &mut out);
    // Duplicate enter is ignored.
    m.enter(&at(1235, MOUSE, 3.0, 4.0), &mut out);
    assert_eq!(seen(&out), [(1_234_000, 1.0, 2.0, 0)]);
}

#[test]
fn enter_leave_leave() {
    let (mut m, mut out) = mouse();
    m.enter(&at(1234, MOUSE, 1.0, 2.0), &mut out);
    m.leave(&at(1235, MOUSE, 3.0, 4.0), &mut out);
    // Duplicate leave is ignored.
    m.leave(&at(1235, MOUSE, 5.0, 6.0), &mut out);
    assert_eq!(
        seen(&out),
        [(1_234_000, 1.0, 2.0, 0), (1_235_000, 3.0, 4.0, 0)]
    );
}

#[test]
fn enter_button_press() {
    let (mut m, mut out) = mouse();
    m.enter(&at(1234, MOUSE, 1.0, 2.0), &mut out);
    assert!(m.button_press(&at(1235, MOUSE, 4.0, 8.0), button::PRIMARY, &mut out));
    assert_eq!(
        seen(&out),
        [
            (1_234_000, 1.0, 2.0, 0),
            (1_235_000, 4.0, 8.0, mouse_button::PRIMARY)
        ]
    );
    assert_eq!(phases(&out), [PointerPhase::Add, PointerPhase::Down]);
}

#[test]
fn no_enter_button_press() {
    let (mut m, mut out) = mouse();
    m.button_press(&at(1234, MOUSE, 4.0, 8.0), button::PRIMARY, &mut out);
    // A synthetic enter first.
    assert_eq!(
        seen(&out),
        [
            (1_234_000, 4.0, 8.0, 0),
            (1_234_000, 4.0, 8.0, mouse_button::PRIMARY)
        ]
    );
}

/// `ButtonPressButtonReleasePrimary`, `Secondary`, `Middle`, `Back` and
/// `Forward`: the five cases differ in the button alone.
#[test]
fn button_press_button_release_each_button() {
    for (gdk, bit) in [
        (button::PRIMARY, mouse_button::PRIMARY),
        (button::SECONDARY, mouse_button::SECONDARY),
        (button::MIDDLE, mouse_button::MIDDLE),
        (8, mouse_button::BACK),
        (9, mouse_button::FORWARD),
    ] {
        let (mut m, mut out) = mouse();
        m.button_press(&at(1234, MOUSE, 4.0, 8.0), gdk, &mut out);
        m.button_release(&at(1235, MOUSE, 5.0, 9.0), gdk, &mut out);
        assert_eq!(
            seen(&out)[1..],
            [(1_234_000, 4.0, 8.0, bit), (1_235_000, 5.0, 9.0, 0)],
            "button {gdk}"
        );
        assert_eq!(
            phases(&out),
            [PointerPhase::Add, PointerPhase::Down, PointerPhase::Up]
        );
    }
}

#[test]
fn button_press_button_release_three_buttons() {
    let (mut m, mut out) = mouse();
    // Press buttons 1-2-3, release 3-2-1.
    m.button_press(&at(1234, MOUSE, 1.0, 2.0), button::PRIMARY, &mut out);
    m.button_press(&at(1235, MOUSE, 3.0, 4.0), button::SECONDARY, &mut out);
    m.button_press(&at(1236, MOUSE, 5.0, 6.0), button::MIDDLE, &mut out);
    m.button_release(&at(1237, MOUSE, 7.0, 8.0), button::MIDDLE, &mut out);
    m.button_release(&at(1238, MOUSE, 9.0, 10.0), button::SECONDARY, &mut out);
    m.button_release(&at(1239, MOUSE, 11.0, 12.0), button::PRIMARY, &mut out);
    let (p, s, mid) = (
        mouse_button::PRIMARY,
        mouse_button::SECONDARY,
        mouse_button::MIDDLE,
    );
    assert_eq!(
        seen(&out)[1..],
        [
            (1_234_000, 1.0, 2.0, p),
            (1_235_000, 3.0, 4.0, p | s),
            (1_236_000, 5.0, 6.0, p | s | mid),
            (1_237_000, 7.0, 8.0, p | s),
            (1_238_000, 9.0, 10.0, p),
            (1_239_000, 11.0, 12.0, 0),
        ]
    );
    // Down with the first, up with the last, moves between.
    use PointerPhase::{Add, Down, Move, Up};
    assert_eq!(phases(&out), [Add, Down, Move, Move, Move, Move, Up]);
}

#[test]
fn button_press_stylus_primary_button() {
    let (mut m, mut out) = mouse();
    let kind = PointerDeviceKind::Stylus;
    m.button_press(&stylus(1234, kind), button::PRIMARY, &mut out);
    m.button_press(&stylus(1235, kind), button::SECONDARY, &mut out);
    assert_eq!(out.len(), 3);
    assert_eq!(out[2].device_kind, kind);
    assert_eq!(
        out[2].buttons,
        stylus_button::CONTACT | stylus_button::PRIMARY
    );
    assert_eq!((out[2].pressure, out[2].pressure_max), (0.5, 1.0));
}

#[test]
fn button_press_stylus_contact() {
    let (mut m, mut out) = mouse();
    let kind = PointerDeviceKind::Stylus;
    m.button_press(&stylus(1234, kind), button::PRIMARY, &mut out);
    assert_eq!(out.len(), 2);
    assert_eq!(
        (out[1].device_kind, out[1].buttons),
        (kind, stylus_button::CONTACT)
    );
}

#[test]
fn button_press_inverted_stylus_contact() {
    let (mut m, mut out) = mouse();
    let kind = PointerDeviceKind::InvertedStylus;
    m.button_press(&stylus(1234, kind), button::PRIMARY, &mut out);
    assert_eq!(out.len(), 2);
    assert_eq!(
        (out[1].device_kind, out[1].buttons),
        (kind, stylus_button::CONTACT)
    );
}

#[test]
fn button_press_stylus_secondary_button() {
    let (mut m, mut out) = mouse();
    let kind = PointerDeviceKind::Stylus;
    m.button_press(&stylus(1234, kind), button::PRIMARY, &mut out);
    m.button_press(&stylus(1235, kind), button::MIDDLE, &mut out);
    assert_eq!(out.len(), 3);
    assert_eq!(
        out[2].buttons,
        stylus_button::CONTACT | stylus_button::SECONDARY
    );
}

#[test]
fn button_press_stylus_unknown_button() {
    let (mut m, mut out) = mouse();
    let pressed = m.button_press(&at(1234, PointerDeviceKind::Stylus, 4.0, 8.0), 8, &mut out);
    assert!(!pressed);
    assert!(out.is_empty());
}

#[test]
fn button_press_button_press_button_release() {
    let (mut m, mut out) = mouse();
    m.button_press(&at(1234, MOUSE, 4.0, 8.0), button::PRIMARY, &mut out);
    // Ignore duplicate press.
    assert!(!m.button_press(&at(1234, MOUSE, 6.0, 10.0), button::PRIMARY, &mut out));
    m.button_release(&at(1235, MOUSE, 5.0, 9.0), button::PRIMARY, &mut out);
    assert_eq!(
        seen(&out)[1..],
        [
            (1_234_000, 4.0, 8.0, mouse_button::PRIMARY),
            (1_235_000, 5.0, 9.0, 0)
        ]
    );
}

#[test]
fn button_press_button_release_button_release() {
    let (mut m, mut out) = mouse();
    m.button_press(&at(1234, MOUSE, 4.0, 8.0), button::PRIMARY, &mut out);
    m.button_release(&at(1235, MOUSE, 5.0, 9.0), button::PRIMARY, &mut out);
    // Ignore duplicate release.
    assert!(!m.button_release(&at(1235, MOUSE, 6.0, 10.0), button::PRIMARY, &mut out));
    assert_eq!(
        seen(&out)[1..],
        [
            (1_234_000, 4.0, 8.0, mouse_button::PRIMARY),
            (1_235_000, 5.0, 9.0, 0)
        ]
    );
}

#[test]
fn no_button_press_button_release() {
    let (mut m, mut out) = mouse();
    // Release without associated press, will be ignored.
    assert!(!m.button_release(&at(1235, MOUSE, 5.0, 9.0), button::PRIMARY, &mut out));
    assert!(out.is_empty());
}

#[test]
fn motion() {
    let (mut m, mut out) = mouse();
    m.motion(&at(1234, MOUSE, 1.0, 2.0), &mut out);
    m.motion(&at(1235, MOUSE, 3.0, 4.0), &mut out);
    m.motion(&at(1236, MOUSE, 5.0, 6.0), &mut out);
    assert_eq!(
        seen(&out)[1..],
        [
            (1_234_000, 1.0, 2.0, 0),
            (1_235_000, 3.0, 4.0, 0),
            (1_236_000, 5.0, 6.0, 0)
        ]
    );
    use PointerPhase::{Add, Hover};
    assert_eq!(phases(&out), [Add, Hover, Hover, Hover]);
}

#[test]
fn drag() {
    let (mut m, mut out) = mouse();
    m.motion(&at(1234, MOUSE, 1.0, 2.0), &mut out);
    m.button_press(&at(1235, MOUSE, 3.0, 4.0), button::PRIMARY, &mut out);
    m.motion(&at(1236, MOUSE, 5.0, 6.0), &mut out);
    m.button_release(&at(1237, MOUSE, 7.0, 8.0), button::PRIMARY, &mut out);
    m.motion(&at(1238, MOUSE, 9.0, 10.0), &mut out);
    let p = mouse_button::PRIMARY;
    assert_eq!(
        seen(&out)[1..],
        [
            (1_234_000, 1.0, 2.0, 0),
            (1_235_000, 3.0, 4.0, p),
            (1_236_000, 5.0, 6.0, p),
            (1_237_000, 7.0, 8.0, 0),
            (1_238_000, 9.0, 10.0, 0),
        ]
    );
    use PointerPhase::{Add, Down, Hover, Move, Up};
    assert_eq!(phases(&out), [Add, Hover, Down, Move, Up, Hover]);
}

#[test]
fn device_kind() {
    let (mut m, mut out) = mouse();
    let kind = PointerDeviceKind::Trackpad;
    m.enter(&at(1234, kind, 1.0, 2.0), &mut out);
    m.button_press(&at(1235, kind, 1.0, 2.0), button::PRIMARY, &mut out);
    m.motion(&at(1238, kind, 3.0, 4.0), &mut out);
    m.button_release(&at(1237, kind, 3.0, 4.0), button::PRIMARY, &mut out);
    m.leave(&at(1235, kind, 3.0, 4.0), &mut out);
    assert_eq!(out.len(), 5);
    assert!(out.iter().all(|e| e.device_kind == kind));
}

/// Not the embedder's: a pointer that leaves with a button down is kept,
/// so the drag's release still arrives, and a stylus's rotation is told
/// in radians.
#[test]
fn a_drag_goes_on_outside_the_view() {
    let (mut m, mut out) = mouse();
    m.button_press(&at(1, MOUSE, 1.0, 1.0), button::PRIMARY, &mut out);
    m.leave(&at(2, MOUSE, -5.0, 1.0), &mut out);
    m.button_release(&at(3, MOUSE, -9.0, 1.0), button::PRIMARY, &mut out);
    use PointerPhase::{Add, Down, Up};
    assert_eq!(phases(&out), [Add, Down, Up]);
    let turned = At {
        rotation: 180.0,
        ..at(4, PointerDeviceKind::Stylus, 0.0, 0.0)
    };
    m.motion(&turned, &mut out);
    assert_eq!(out[3].rotation, std::f64::consts::PI);
}

fn wheel(dx: f64, dy: f64) -> Scroll {
    Scroll {
        time: Duration::from_millis(1),
        x: 4.0,
        y: 8.0,
        delta_x: dx,
        delta_y: dy,
        touchpad: false,
        stop: false,
    }
}

/// `DiscreteDirectional`: a notch each way. GDK's directions are a
/// source's to turn into deltas.
#[test]
fn discrete_directional() {
    let mut s = Scrolling::new(0);
    let mut out = Vec::new();
    for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
        s.scroll(&wheel(dx, dy), 1.0, &mut out);
    }
    let deltas: Vec<(f64, f64)> = out
        .iter()
        .map(|e| (e.scroll_delta_x, e.scroll_delta_y))
        .collect();
    assert_eq!(
        deltas,
        [(0.0, -53.0), (0.0, 53.0), (-53.0, 0.0), (53.0, 0.0)]
    );
    for e in &out {
        assert_eq!((e.x, e.y, e.timestamp.as_micros()), (4.0, 8.0, 1000));
        assert_eq!(e.device_kind, MOUSE);
        assert_eq!(e.signal_kind, PointerSignalKind::Scroll);
        assert_eq!((e.device, e.buttons), (MOUSE_DEVICE, 0));
    }
}

#[test]
fn discrete_scrolling() {
    let mut s = Scrolling::new(0);
    let mut out = Vec::new();
    s.scroll(&wheel(1.0, 2.0), 1.0, &mut out);
    assert_eq!((out[0].x, out[0].y), (4.0, 8.0));
    assert_eq!(out[0].timestamp.as_micros(), 1000);
    assert_eq!(
        (out[0].scroll_delta_x, out[0].scroll_delta_y),
        (53.0, 106.0)
    );
    // At a view of two pixels to one, a notch is twice the pixels.
    s.scroll(&wheel(1.0, 2.0), 2.0, &mut out);
    assert_eq!(
        (out[1].scroll_delta_x, out[1].scroll_delta_y),
        (106.0, 212.0)
    );
    // No distance: no signal.
    s.scroll(&wheel(0.0, 0.0), 1.0, &mut out);
    assert_eq!(out[2].signal_kind, PointerSignalKind::None);
}

#[test]
fn panning() {
    let mut s = Scrolling::new(0);
    let mut out = Vec::new();
    let mut pad = Scroll {
        touchpad: true,
        ..wheel(1.0, 2.0)
    };
    s.scroll(&pad, 1.0, &mut out);
    s.scroll(&pad, 1.0, &mut out);
    pad.stop = true;
    s.scroll(&pad, 1.0, &mut out);
    use PointerPhase::{PanZoomEnd, PanZoomStart, PanZoomUpdate};
    assert_eq!(
        phases(&out),
        [PanZoomStart, PanZoomUpdate, PanZoomUpdate, PanZoomEnd]
    );
    for e in &out {
        assert_eq!((e.x, e.y, e.timestamp.as_micros()), (4.0, 8.0, 1000));
        assert_eq!(e.device_kind, PointerDeviceKind::Trackpad);
        assert_eq!(e.device, PAN_ZOOM_DEVICE);
    }
    // Directions get swapped.
    assert_eq!((out[1].pan_x, out[1].pan_y), (-53.0, -106.0));
    assert_eq!((out[1].scale, out[1].rotation), (1.0, 0.0));
    assert_eq!((out[2].pan_x, out[2].pan_y), (-106.0, -212.0));
    assert_eq!((out[2].scale, out[2].rotation), (1.0, 0.0));
    // The next pan starts from nothing.
    pad.stop = false;
    s.scroll(&pad, 1.0, &mut out);
    assert_eq!(out[4].phase, PanZoomStart);
    assert_eq!((out[5].pan_x, out[5].pan_y), (-53.0, -106.0));
}

fn t(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

/// What the zoom and rotation tests check: phase, pan, scale, rotation,
/// at the mouse's last position, which is the origin until it moves.
fn gesture(events: &[PointerEvent]) -> Vec<(PointerPhase, f64, f64)> {
    for e in events {
        assert_eq!((e.x, e.y, e.pan_x, e.pan_y), (0.0, 0.0, 0.0, 0.0));
    }
    events
        .iter()
        .map(|e| (e.phase, e.scale, e.rotation))
        .collect()
}

#[test]
fn zooming() {
    use PointerPhase::{PanZoomEnd, PanZoomStart, PanZoomUpdate};
    let mut s = Scrolling::new(0);
    let mut out = Vec::new();
    s.zoom_begin(t(1), &mut out);
    s.zoom_update(t(2), 1.1, &mut out);
    s.zoom_end(t(3), &mut out);
    assert_eq!(
        gesture(&out),
        [
            (PanZoomStart, 0.0, 0.0),
            (PanZoomUpdate, 1.1, 0.0),
            (PanZoomEnd, 0.0, 0.0)
        ]
    );
}

#[test]
fn rotating() {
    use PointerPhase::{PanZoomEnd, PanZoomStart, PanZoomUpdate};
    let mut s = Scrolling::new(0);
    let mut out = Vec::new();
    s.rotation_begin(t(1), &mut out);
    s.rotation_update(t(2), 0.5, &mut out);
    s.rotation_end(t(3), &mut out);
    assert_eq!(
        gesture(&out),
        [
            (PanZoomStart, 0.0, 0.0),
            (PanZoomUpdate, 1.0, 0.5),
            (PanZoomEnd, 0.0, 0.0)
        ]
    );
}

#[test]
fn synchronized_zooming_and_rotating() {
    use PointerPhase::{PanZoomEnd, PanZoomStart, PanZoomUpdate};
    let mut s = Scrolling::new(0);
    let mut out = Vec::new();
    s.zoom_begin(t(1), &mut out);
    s.zoom_update(t(2), 1.1, &mut out);
    s.rotation_begin(t(3), &mut out);
    s.rotation_update(t(4), 0.5, &mut out);
    s.zoom_end(t(5), &mut out);
    // The end is sent only after both zoom and rotate complete.
    assert_eq!(out.len(), 3);
    s.rotation_end(t(6), &mut out);
    assert_eq!(
        gesture(&out),
        [
            (PanZoomStart, 0.0, 0.0),
            (PanZoomUpdate, 1.1, 0.0),
            (PanZoomUpdate, 1.1, 0.5),
            (PanZoomEnd, 0.0, 0.0)
        ]
    );
}

/// Zoom and rotate sequences that do not end together.
#[test]
fn unsynchronized_zooming_and_rotating() {
    use PointerPhase::{PanZoomEnd, PanZoomStart, PanZoomUpdate};
    let mut s = Scrolling::new(0);
    let mut out = Vec::new();
    s.zoom_begin(t(1), &mut out);
    s.zoom_update(t(2), 1.1, &mut out);
    s.rotation_begin(t(3), &mut out);
    s.rotation_update(t(4), 0.5, &mut out);
    s.zoom_end(t(5), &mut out);
    s.rotation_update(t(6), 1.0, &mut out);
    s.rotation_end(t(7), &mut out);
    assert_eq!(
        gesture(&out),
        [
            (PanZoomStart, 0.0, 0.0),
            (PanZoomUpdate, 1.1, 0.0),
            (PanZoomUpdate, 1.1, 0.5),
            (PanZoomUpdate, 1.1, 1.0),
            (PanZoomEnd, 0.0, 0.0)
        ]
    );
}

/// Not the embedder's test: a gesture is said to be where the mouse was.
#[test]
fn a_gesture_is_where_the_mouse_last_was() {
    let mut s = Scrolling::new(7);
    let mut out = Vec::new();
    s.set_last_mouse_position(30.0, 40.0);
    s.zoom_begin(t(1), &mut out);
    assert_eq!((out[0].x, out[0].y, out[0].view_id), (30.0, 40.0, 7));
    // A wheel's scroll moves it.
    s.scroll(&wheel(0.0, 1.0), 1.0, &mut out);
    s.zoom_update(t(2), 2.0, &mut out);
    assert_eq!((out[2].x, out[2].y), (4.0, 8.0));
}

/// `TouchEvents`.
#[test]
fn touch_events() {
    let mut touches = Touches::new(0);
    let mut out = Vec::new();
    touches.touch(t(1), TouchAction::Begin, 0, 4.0, 8.0, &mut out);
    assert_eq!(out.len(), 2);
    touches.touch(t(1), TouchAction::Update, 0, 4.0, 8.0, &mut out);
    assert_eq!(out.len(), 3);
    touches.touch(t(1), TouchAction::End, 0, 4.0, 8.0, &mut out);
    assert_eq!(out.len(), 5);
    use PointerPhase::{Add, Down, Move, Remove, Up};
    assert_eq!(phases(&out), [Add, Down, Move, Up, Remove]);
    for e in &out {
        assert_eq!((e.x, e.y, e.timestamp.as_micros()), (4.0, 8.0, 1000));
        assert_eq!(e.device_kind, PointerDeviceKind::Touch);
        assert_eq!(e.device, 2 << 28);
    }
    // In contact while down, as the embedder says it with a button.
    let buttons: Vec<i64> = out.iter().map(|e| e.buttons).collect();
    assert_eq!(buttons, [0, 1, 1, 0, 0]);
}

/// Each finger is a pointer of its own, with the lowest id none has; an
/// id is given again once its finger has lifted.
#[test]
fn fingers_have_their_own_ids() {
    let mut touches = Touches::new(0);
    let mut out = Vec::new();
    let device = |e: &PointerEvent| e.device & 0xff;
    touches.touch(t(1), TouchAction::Begin, 70, 1.0, 1.0, &mut out);
    touches.touch(t(1), TouchAction::Begin, 71, 2.0, 2.0, &mut out);
    touches.touch(t(1), TouchAction::Begin, 72, 3.0, 3.0, &mut out);
    assert_eq!(
        out.iter().map(device).collect::<Vec<_>>(),
        [0, 0, 1, 1, 2, 2]
    );
    out.clear();
    touches.touch(t(2), TouchAction::End, 71, 2.0, 2.0, &mut out);
    touches.touch(t(3), TouchAction::Begin, 99, 5.0, 5.0, &mut out);
    touches.touch(t(4), TouchAction::Update, 72, 6.0, 6.0, &mut out);
    assert_eq!(
        out.iter().map(|e| (e.phase, device(e))).collect::<Vec<_>>(),
        [
            (PointerPhase::Up, 1),
            (PointerPhase::Remove, 1),
            (PointerPhase::Add, 1),
            (PointerPhase::Down, 1),
            (PointerPhase::Move, 2),
        ]
    );
}

/// More fingers than ids (brief M4 §15): the rest share id 0, and what
/// is remembered stops at the ids there are.
#[test]
fn fingers_past_the_last_id_are_bounded() {
    let mut touches = Touches::new(0);
    let mut out = Vec::new();
    for n in 0..MAX_TOUCHES + 50 {
        touches.touch(t(1), TouchAction::Begin, 1000 + n, 0.0, 0.0, &mut out);
    }
    assert_eq!(touches.ids.len(), MAX_TOUCHES as usize);
    assert_eq!(touches.added.len(), MAX_TOUCHES as usize);
    assert_eq!(out.last().map(|e| e.device & 0xff), Some(0));
    for n in 0..MAX_TOUCHES + 50 {
        touches.touch(t(2), TouchAction::End, 1000 + n, 0.0, 0.0, &mut out);
    }
    assert!(touches.ids.is_empty());
    assert!(touches.added.is_empty());
}
