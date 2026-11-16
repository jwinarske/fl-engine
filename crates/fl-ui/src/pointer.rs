//! Pointer packets (brief M4 §2): `PointerData` as `_unpackPointerDataPacket`
//! reads it, an embedder event made into one as `FlutterEngineSendPointerEvent`
//! makes it, and the engine's `PointerDataPacketConverter`, which makes the
//! stream the framework is given well-formed.
//!
//! The converter is a port of `lib/ui/window/pointer_data_packet_converter.cc`
//! at the pin, its unit tests with it. Where the engine asserts (a move for
//! a pointer never added), the event is dropped here: in a release engine
//! that is a read of a map's end.

use std::collections::BTreeMap;

use fl_input::{PointerDeviceKind, PointerEvent, PointerPhase, PointerSignalKind};

/// `PointerData::Change`, `PointerChange` in pointer.dart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum Change {
    Cancel,
    Add,
    Remove,
    Hover,
    Down,
    Move,
    Up,
    PanZoomStart,
    PanZoomUpdate,
    PanZoomEnd,
}

/// `PointerData::DeviceKind`, `PointerDeviceKind` in pointer.dart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum DeviceKind {
    Touch,
    Mouse,
    Stylus,
    InvertedStylus,
    Trackpad,
}

/// `PointerData::SignalKind`, `PointerSignalKind` in pointer.dart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum SignalKind {
    None,
    Scroll,
    ScrollInertiaCancel,
    Scale,
}

/// `kPointerButtonTouchContact`.
const TOUCH_CONTACT: i64 = 1;

/// `kPointerDataFieldCount`: every field is 8 bytes.
pub const FIELD_COUNT: usize = 36;

/// The engine's `PointerData`, in its field order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerData {
    pub embedder_id: i64,
    /// Microseconds.
    pub time_stamp: i64,
    pub change: Change,
    pub kind: DeviceKind,
    pub signal_kind: SignalKind,
    pub device: i64,
    pub pointer_identifier: i64,
    pub physical_x: f64,
    pub physical_y: f64,
    pub physical_delta_x: f64,
    pub physical_delta_y: f64,
    pub buttons: i64,
    pub obscured: i64,
    pub synthesized: i64,
    pub pressure: f64,
    pub pressure_min: f64,
    pub pressure_max: f64,
    pub distance: f64,
    pub distance_max: f64,
    pub size: f64,
    pub radius_major: f64,
    pub radius_minor: f64,
    pub radius_min: f64,
    pub radius_max: f64,
    pub orientation: f64,
    pub tilt: f64,
    pub platform_data: i64,
    pub scroll_delta_x: f64,
    pub scroll_delta_y: f64,
    pub pan_x: f64,
    pub pan_y: f64,
    pub pan_delta_x: f64,
    pub pan_delta_y: f64,
    pub scale: f64,
    pub rotation: f64,
    pub view_id: i64,
}

impl PointerData {
    /// `PointerData::Clear`, with a change and a kind: every other field 0.
    pub fn cleared(change: Change, kind: DeviceKind) -> PointerData {
        PointerData {
            embedder_id: 0,
            time_stamp: 0,
            change,
            kind,
            signal_kind: SignalKind::None,
            device: 0,
            pointer_identifier: 0,
            physical_x: 0.0,
            physical_y: 0.0,
            physical_delta_x: 0.0,
            physical_delta_y: 0.0,
            buttons: 0,
            obscured: 0,
            synthesized: 0,
            pressure: 0.0,
            pressure_min: 0.0,
            pressure_max: 0.0,
            distance: 0.0,
            distance_max: 0.0,
            size: 0.0,
            radius_major: 0.0,
            radius_minor: 0.0,
            radius_min: 0.0,
            radius_max: 0.0,
            orientation: 0.0,
            tilt: 0.0,
            platform_data: 0,
            scroll_delta_x: 0.0,
            scroll_delta_y: 0.0,
            pan_x: 0.0,
            pan_y: 0.0,
            pan_delta_x: 0.0,
            pan_delta_y: 0.0,
            scale: 0.0,
            rotation: 0.0,
            view_id: 0,
        }
    }

    /// An embedder event as `FlutterEngineSendPointerEvent` makes it a
    /// `PointerData`: a touch is in contact while down or moving, whatever
    /// its buttons say; any other kind's buttons are its own.
    pub fn from_event(e: &PointerEvent) -> PointerData {
        let change = match e.phase {
            PointerPhase::Cancel => Change::Cancel,
            PointerPhase::Up => Change::Up,
            PointerPhase::Down => Change::Down,
            PointerPhase::Move => Change::Move,
            PointerPhase::Add => Change::Add,
            PointerPhase::Remove => Change::Remove,
            PointerPhase::Hover => Change::Hover,
            PointerPhase::PanZoomStart => Change::PanZoomStart,
            PointerPhase::PanZoomUpdate => Change::PanZoomUpdate,
            PointerPhase::PanZoomEnd => Change::PanZoomEnd,
        };
        let kind = match e.device_kind {
            PointerDeviceKind::Mouse => DeviceKind::Mouse,
            PointerDeviceKind::Touch => DeviceKind::Touch,
            PointerDeviceKind::Stylus => DeviceKind::Stylus,
            PointerDeviceKind::Trackpad => DeviceKind::Trackpad,
            PointerDeviceKind::InvertedStylus => DeviceKind::InvertedStylus,
        };
        let buttons = match kind {
            DeviceKind::Touch if matches!(change, Change::Down | Change::Move) => TOUCH_CONTACT,
            DeviceKind::Touch => 0,
            _ => e.buttons,
        };
        PointerData {
            time_stamp: i64::try_from(e.timestamp.as_micros()).unwrap_or(i64::MAX),
            signal_kind: match e.signal_kind {
                PointerSignalKind::None => SignalKind::None,
                PointerSignalKind::Scroll => SignalKind::Scroll,
                PointerSignalKind::ScrollInertiaCancel => SignalKind::ScrollInertiaCancel,
                PointerSignalKind::Scale => SignalKind::Scale,
            },
            device: i64::from(e.device),
            physical_x: e.x,
            physical_y: e.y,
            buttons,
            scroll_delta_x: e.scroll_delta_x,
            scroll_delta_y: e.scroll_delta_y,
            pan_x: e.pan_x,
            pan_y: e.pan_y,
            scale: e.scale,
            rotation: e.rotation,
            pressure: e.pressure,
            pressure_min: e.pressure_min,
            pressure_max: e.pressure_max,
            view_id: e.view_id,
            ..PointerData::cleared(change, kind)
        }
    }

    /// Appends the record as a packet holds it: its 36 fields in order,
    /// 8 bytes each, little-endian (`_kFakeHostEndian`).
    pub fn encode(&self, out: &mut Vec<u8>) {
        let i = |v: i64| v.to_le_bytes();
        let f = |v: f64| v.to_le_bytes();
        let fields: [[u8; 8]; FIELD_COUNT] = [
            i(self.embedder_id),
            i(self.time_stamp),
            i(self.change as i64),
            i(self.kind as i64),
            i(self.signal_kind as i64),
            i(self.device),
            i(self.pointer_identifier),
            f(self.physical_x),
            f(self.physical_y),
            f(self.physical_delta_x),
            f(self.physical_delta_y),
            i(self.buttons),
            i(self.obscured),
            i(self.synthesized),
            f(self.pressure),
            f(self.pressure_min),
            f(self.pressure_max),
            f(self.distance),
            f(self.distance_max),
            f(self.size),
            f(self.radius_major),
            f(self.radius_minor),
            f(self.radius_min),
            f(self.radius_max),
            f(self.orientation),
            f(self.tilt),
            i(self.platform_data),
            f(self.scroll_delta_x),
            f(self.scroll_delta_y),
            f(self.pan_x),
            f(self.pan_y),
            f(self.pan_delta_x),
            f(self.pan_delta_y),
            f(self.scale),
            f(self.rotation),
            i(self.view_id),
        ];
        out.extend(fields.iter().flatten());
    }
}

/// A `PointerDataPacket`'s bytes: the records one after another.
pub fn encode_packet(records: &[PointerData]) -> Vec<u8> {
    let mut out = Vec::with_capacity(records.len() * FIELD_COUNT * 8);
    for r in records {
        r.encode(&mut out);
    }
    out
}

/// `PointerState`: what the converter keeps for a device.
#[derive(Debug, Clone, Copy)]
struct State {
    pointer_identifier: i64,
    is_down: bool,
    is_pan_zoom_active: bool,
    physical_x: f64,
    physical_y: f64,
    pan_x: f64,
    pan_y: f64,
    scale: f64,
    rotation: f64,
    buttons: i64,
    view_id: i64,
}

/// `PointerDataPacketConverter`: adds the events the framework expects and
/// a source may leave out, and gives each contact its pointer identifier.
///
/// * an `add` before a pointer's first hover, down, pan-zoom or signal
/// * a hover or move to an event's position when the pointer is not there
/// * a cancel for a pointer that is down when it is added again or removed,
///   and a remove before it is added again
/// * each event's deltas, from where the pointer was
///
/// An event for a view that does not exist is dropped, as is a cancel,
/// move, up or remove for a pointer never added.
///
/// Its state is the UI thread's, as the engine's is (`RuntimeController`'s):
/// not `Send`.
#[derive(Debug, Default)]
pub struct Converter {
    states: BTreeMap<i64, State>,
    /// The last pointer identifier given out.
    pointer: i64,
    _ui_thread: std::marker::PhantomData<*const ()>,
}

impl Converter {
    pub fn new() -> Converter {
        Converter::default()
    }

    /// `Convert`: the records of `packet`, well-formed, appended to `out`.
    /// `view_exists` is the delegate's `ViewExists`.
    pub fn convert(
        &mut self,
        packet: &[PointerData],
        view_exists: impl Fn(i64) -> bool,
        out: &mut Vec<PointerData>,
    ) {
        for data in packet {
            self.convert_one(*data, &view_exists, out);
        }
    }

    fn convert_one(
        &mut self,
        mut data: PointerData,
        view_exists: &impl Fn(i64) -> bool,
        out: &mut Vec<PointerData>,
    ) {
        if !view_exists(data.view_id) {
            return;
        }
        if data.signal_kind != SignalKind::None {
            let mut state = match self.states.get(&data.device) {
                Some(s) => *s,
                None => {
                    let add = PointerData {
                        signal_kind: SignalKind::None,
                        ..synthesized(data, Change::Add, 0)
                    };
                    out.push(add);
                    self.ensure_state(&add)
                }
            };
            if location_needs_update(&data, &state) {
                let (change, buttons) = if state.is_down {
                    (Change::Move, state.buttons)
                } else {
                    (Change::Hover, 0)
                };
                let mut moved = PointerData {
                    signal_kind: SignalKind::None,
                    ..synthesized(data, change, buttons)
                };
                self.update_delta_and_state(&mut moved, &mut state);
                out.push(moved);
            }
            out.push(data);
            return;
        }
        match data.change {
            Change::Cancel => {
                let Some(mut state) = self.states.get(&data.device).copied() else {
                    return;
                };
                data.pointer_identifier = state.pointer_identifier;
                if location_needs_update(&data, &state) {
                    let mut moved = synthesized(data, Change::Move, data.buttons);
                    self.update_delta_and_state(&mut moved, &mut state);
                    out.push(moved);
                }
                state.is_down = false;
                self.states.insert(data.device, state);
                out.push(data);
            }
            Change::Add => {
                if let Some(state) = self.states.get(&data.device).copied() {
                    // Added again without a remove: the pointer it was is
                    // ended first, where it was and in the view it was in.
                    let mut ended = PointerData {
                        physical_x: state.physical_x,
                        physical_y: state.physical_y,
                        pan_x: state.pan_x,
                        pan_y: state.pan_y,
                        scale: state.scale,
                        rotation: state.rotation,
                        buttons: state.buttons,
                        synthesized: 1,
                        ..data
                    };
                    if view_exists(state.view_id) {
                        ended.view_id = state.view_id;
                        if state.is_down {
                            out.push(PointerData {
                                change: Change::Cancel,
                                pointer_identifier: state.pointer_identifier,
                                ..ended
                            });
                        }
                    }
                    out.push(PointerData {
                        change: Change::Remove,
                        ..ended
                    });
                }
                self.ensure_state(&data);
                out.push(data);
            }
            Change::Remove => {
                let Some(mut state) = self.states.get(&data.device).copied() else {
                    return;
                };
                // Removed already, by an add in another view.
                if state.view_id != data.view_id {
                    return;
                }
                if state.is_down {
                    let mut cancel = synthesized(data, Change::Cancel, data.buttons);
                    cancel.pointer_identifier = state.pointer_identifier;
                    state.is_down = false;
                    self.states.insert(data.device, state);
                    out.push(cancel);
                }
                if location_needs_update(&data, &state) {
                    let mut hover = synthesized(data, Change::Hover, data.buttons);
                    self.update_delta_and_state(&mut hover, &mut state);
                    out.push(hover);
                }
                self.states.remove(&data.device);
                out.push(data);
            }
            Change::Hover => {
                let mut state = self.state_or_add(&data, out);
                // Kept only if the pointer moved, as the engine keeps it.
                state.buttons = data.buttons;
                if location_needs_update(&data, &state) {
                    self.update_delta_and_state(&mut data, &mut state);
                    out.push(data);
                }
            }
            Change::Down => {
                let mut state = self.state_or_add(&data, out);
                if location_needs_update(&data, &state) {
                    let mut hover = synthesized(data, Change::Hover, 0);
                    self.update_delta_and_state(&mut hover, &mut state);
                    out.push(hover);
                }
                self.pointer += 1;
                state.pointer_identifier = self.pointer;
                data.pointer_identifier = state.pointer_identifier;
                state.is_down = true;
                state.buttons = data.buttons;
                self.states.insert(data.device, state);
                out.push(data);
            }
            Change::Move => {
                let Some(mut state) = self.states.get(&data.device).copied() else {
                    return;
                };
                data.pointer_identifier = state.pointer_identifier;
                self.update_delta_and_state(&mut data, &mut state);
                out.push(data);
            }
            Change::Up => {
                let Some(mut state) = self.states.get(&data.device).copied() else {
                    return;
                };
                data.pointer_identifier = state.pointer_identifier;
                if location_needs_update(&data, &state) {
                    let mut moved = synthesized(data, Change::Move, state.buttons);
                    self.update_delta_and_state(&mut moved, &mut state);
                    out.push(moved);
                }
                state.is_down = false;
                state.buttons = data.buttons;
                self.states.insert(data.device, state);
                out.push(data);
            }
            Change::PanZoomStart => {
                let mut state = self.state_or_add(&data, out);
                if location_needs_update(&data, &state) {
                    let mut hover = synthesized(data, Change::Hover, 0);
                    self.update_delta_and_state(&mut hover, &mut state);
                    out.push(hover);
                }
                self.pointer += 1;
                state.pointer_identifier = self.pointer;
                data.pointer_identifier = state.pointer_identifier;
                state.is_pan_zoom_active = true;
                state.pan_x = 0.0;
                state.pan_y = 0.0;
                state.scale = 1.0;
                state.rotation = 0.0;
                self.states.insert(data.device, state);
                out.push(data);
            }
            Change::PanZoomUpdate => {
                let Some(mut state) = self.states.get(&data.device).copied() else {
                    return;
                };
                data.pointer_identifier = state.pointer_identifier;
                self.update_delta_and_state(&mut data, &mut state);
                out.push(data);
            }
            Change::PanZoomEnd => {
                let Some(mut state) = self.states.get(&data.device).copied() else {
                    return;
                };
                data.pointer_identifier = state.pointer_identifier;
                if location_needs_update(&data, &state) {
                    let mut moved = PointerData {
                        pan_x: state.pan_x,
                        pan_y: state.pan_y,
                        pan_delta_x: 0.0,
                        pan_delta_y: 0.0,
                        scale: state.scale,
                        rotation: state.rotation,
                        ..synthesized(data, Change::PanZoomUpdate, data.buttons)
                    };
                    self.update_delta_and_state(&mut moved, &mut state);
                    out.push(moved);
                }
                state.is_pan_zoom_active = false;
                self.states.insert(data.device, state);
                out.push(data);
            }
        }
    }

    /// The device's state, or a new one with the `add` that makes it.
    fn state_or_add(&mut self, data: &PointerData, out: &mut Vec<PointerData>) -> State {
        if let Some(s) = self.states.get(&data.device) {
            return *s;
        }
        let add = synthesized(*data, Change::Add, 0);
        out.push(add);
        self.ensure_state(&add)
    }

    /// `EnsurePointerState`: a pointer newly there, up, where `data` is.
    fn ensure_state(&mut self, data: &PointerData) -> State {
        let state = State {
            pointer_identifier: 0,
            is_down: false,
            is_pan_zoom_active: false,
            physical_x: data.physical_x,
            physical_y: data.physical_y,
            pan_x: 0.0,
            pan_y: 0.0,
            scale: 0.0,
            rotation: 0.0,
            buttons: 0,
            view_id: data.view_id,
        };
        self.states.insert(data.device, state);
        state
    }

    /// `UpdateDeltaAndState`: the event's deltas from where the pointer
    /// was, and the pointer moved to where the event is.
    fn update_delta_and_state(&mut self, data: &mut PointerData, state: &mut State) {
        data.physical_delta_x = data.physical_x - state.physical_x;
        data.physical_delta_y = data.physical_y - state.physical_y;
        data.pan_delta_x = data.pan_x - state.pan_x;
        data.pan_delta_y = data.pan_y - state.pan_y;
        state.physical_x = data.physical_x;
        state.physical_y = data.physical_y;
        state.pan_x = data.pan_x;
        state.pan_y = data.pan_y;
        state.scale = data.scale;
        state.rotation = data.rotation;
        self.states.insert(data.device, *state);
    }
}

/// `data` as an event the converter adds: of `change`, with `buttons`.
fn synthesized(data: PointerData, change: Change, buttons: i64) -> PointerData {
    PointerData {
        change,
        buttons,
        synthesized: 1,
        ..data
    }
}

/// `LocationNeedsUpdate`.
fn location_needs_update(data: &PointerData, state: &State) -> bool {
    state.physical_x != data.physical_x || state.physical_y != data.physical_y
}

#[cfg(test)]
mod tests {
    //! `pointer_data_packet_converter_unittests.cc` at the pin, case for
    //! case under the engine's test names, and the packet's layout.

    use super::*;

    /// `CreateSimulatedPointerData`: a touch in view 0.
    fn touch(change: Change, device: i64, x: f64, y: f64, buttons: i64) -> PointerData {
        PointerData {
            device,
            physical_x: x,
            physical_y: y,
            buttons,
            ..PointerData::cleared(change, DeviceKind::Touch)
        }
    }

    fn in_view(view_id: i64, data: PointerData) -> PointerData {
        PointerData { view_id, ..data }
    }

    /// `CreateSimulatedMousePointerData`.
    fn mouse(
        change: Change,
        signal_kind: SignalKind,
        device: i64,
        (x, y): (f64, f64),
        (scroll_x, scroll_y): (f64, f64),
        buttons: i64,
    ) -> PointerData {
        PointerData {
            signal_kind,
            device,
            physical_x: x,
            physical_y: y,
            scroll_delta_x: scroll_x,
            scroll_delta_y: scroll_y,
            buttons,
            ..PointerData::cleared(change, DeviceKind::Mouse)
        }
    }

    /// `CreateSimulatedTrackpadGestureData`: at the origin.
    fn trackpad(change: Change, device: i64, pan: (f64, f64), scale: f64) -> PointerData {
        PointerData {
            device,
            pan_x: pan.0,
            pan_y: pan.1,
            scale,
            ..PointerData::cleared(change, DeviceKind::Mouse)
        }
    }

    fn convert_in(c: &mut Converter, views: &[i64], packet: &[PointerData]) -> Vec<PointerData> {
        let mut out = Vec::new();
        c.convert(packet, |v| views.contains(&v), &mut out);
        out
    }

    fn convert(packet: &[PointerData]) -> Vec<PointerData> {
        convert_in(&mut Converter::new(), &[0], packet)
    }

    /// Each record's change and whether the converter added it.
    fn changes(records: &[PointerData]) -> Vec<(Change, i64)> {
        records.iter().map(|r| (r.change, r.synthesized)).collect()
    }

    fn delta(r: &PointerData) -> (f64, f64) {
        (r.physical_delta_x, r.physical_delta_y)
    }

    fn at(r: &PointerData) -> (f64, f64) {
        (r.physical_x, r.physical_y)
    }

    use Change::*;

    #[test]
    fn can_convert_pointer_data_packet() {
        let r = convert(&[
            touch(Add, 0, 0.0, 0.0, 0),
            touch(Hover, 0, 3.0, 0.0, 0),
            touch(Down, 0, 3.0, 0.0, 1),
            touch(Move, 0, 3.0, 4.0, 1),
            touch(Up, 0, 3.0, 4.0, 0),
            touch(Remove, 0, 3.0, 4.0, 0),
        ]);
        assert_eq!(
            changes(&r),
            [
                (Add, 0),
                (Hover, 0),
                (Down, 0),
                (Move, 0),
                (Up, 0),
                (Remove, 0)
            ]
        );
        assert_eq!(delta(&r[1]), (3.0, 0.0));
        assert_eq!(delta(&r[3]), (0.0, 4.0));
        for d in &r[2..=4] {
            assert_eq!(d.pointer_identifier, 1);
        }
    }

    #[test]
    fn can_synthesize_down_and_up() {
        let r = convert(&[
            touch(Add, 0, 0.0, 0.0, 0),
            touch(Down, 0, 3.0, 0.0, 1),
            touch(Up, 0, 3.0, 4.0, 0),
            touch(Remove, 0, 3.0, 4.0, 0),
        ]);
        assert_eq!(
            changes(&r),
            [
                (Add, 0),
                (Hover, 1),
                (Down, 0),
                (Move, 1),
                (Up, 0),
                (Remove, 0)
            ]
        );
        assert_eq!((delta(&r[1]), r[1].buttons), ((3.0, 0.0), 0));
        assert_eq!((r[2].pointer_identifier, r[2].buttons), (1, 1));
        assert_eq!((delta(&r[3]), r[3].buttons), ((0.0, 4.0), 1));
        assert_eq!(r[3].pointer_identifier, 1);
        assert_eq!((r[4].pointer_identifier, r[4].buttons), (1, 0));
    }

    #[test]
    fn can_update_pointer_identifier() {
        let r = convert(&[
            touch(Add, 0, 0.0, 0.0, 0),
            touch(Down, 0, 0.0, 0.0, 1),
            touch(Up, 0, 0.0, 0.0, 0),
            touch(Down, 0, 0.0, 0.0, 1),
            touch(Move, 0, 3.0, 0.0, 1),
            touch(Up, 0, 3.0, 0.0, 0),
            touch(Remove, 0, 3.0, 0.0, 0),
        ]);
        assert_eq!(
            changes(&r),
            [
                (Add, 0),
                (Down, 0),
                (Up, 0),
                (Down, 0),
                (Move, 0),
                (Up, 0),
                (Remove, 0)
            ]
        );
        let ids: Vec<i64> = r[1..6].iter().map(|d| d.pointer_identifier).collect();
        assert_eq!(ids, [1, 1, 2, 2, 2]);
        assert_eq!(delta(&r[4]), (3.0, 0.0));
    }

    #[test]
    fn always_forward_move_event() {
        let r = convert(&[
            touch(Add, 0, 0.0, 0.0, 0),
            touch(Down, 0, 0.0, 0.0, 1),
            touch(Move, 0, 0.0, 0.0, 1),
            touch(Up, 0, 0.0, 0.0, 0),
        ]);
        assert_eq!(changes(&r), [(Add, 0), (Down, 0), (Move, 0), (Up, 0)]);
        for d in &r[1..] {
            assert_eq!(d.pointer_identifier, 1);
        }
    }

    #[test]
    fn can_work_with_different_devices() {
        let r = convert(&[
            touch(Add, 0, 0.0, 0.0, 0),
            touch(Down, 0, 0.0, 0.0, 1),
            touch(Add, 1, 0.0, 0.0, 0),
            touch(Down, 1, 0.0, 0.0, 1),
            touch(Up, 0, 0.0, 0.0, 0),
            touch(Down, 0, 0.0, 0.0, 1),
            touch(Move, 1, 0.0, 4.0, 1),
            touch(Move, 0, 3.0, 0.0, 1),
            touch(Up, 1, 0.0, 4.0, 0),
            touch(Up, 0, 3.0, 0.0, 0),
            touch(Remove, 0, 3.0, 0.0, 0),
            touch(Remove, 1, 0.0, 4.0, 0),
        ]);
        let got: Vec<(Change, i64, i64, i64)> = r
            .iter()
            .map(|d| (d.change, d.device, d.pointer_identifier, d.synthesized))
            .collect();
        assert_eq!(
            got,
            [
                (Add, 0, 0, 0),
                (Down, 0, 1, 0),
                (Add, 1, 0, 0),
                (Down, 1, 2, 0),
                (Up, 0, 1, 0),
                (Down, 0, 3, 0),
                (Move, 1, 2, 0),
                (Move, 0, 3, 0),
                (Up, 1, 2, 0),
                (Up, 0, 3, 0),
                (Remove, 0, 0, 0),
                (Remove, 1, 0, 0),
            ]
        );
        assert_eq!(delta(&r[6]), (0.0, 4.0));
        assert_eq!(delta(&r[7]), (3.0, 0.0));
    }

    #[test]
    fn can_synthesize_add() {
        let r = convert(&[touch(Down, 0, 330.0, 450.0, 1), touch(Up, 0, 0.0, 0.0, 0)]);
        assert_eq!(changes(&r), [(Add, 1), (Down, 0), (Move, 1), (Up, 0)]);
        assert_eq!((at(&r[0]), r[0].buttons), ((330.0, 450.0), 0));
        assert_eq!((at(&r[1]), r[1].buttons), ((330.0, 450.0), 1));
        assert_eq!((at(&r[2]), r[2].buttons), ((0.0, 0.0), 1));
        assert_eq!(delta(&r[2]), (-330.0, -450.0));
        assert_eq!((at(&r[3]), r[3].buttons), ((0.0, 0.0), 0));
    }

    #[test]
    fn can_synthesize_remove() {
        let r = convert_in(
            &mut Converter::new(),
            &[100, 200],
            &[
                in_view(100, touch(Add, 0, 0.0, 0.0, 0)),
                in_view(100, touch(Down, 0, 3.0, 4.0, 1)),
                in_view(200, touch(Add, 0, 0.0, 0.0, 0)),
            ],
        );
        assert_eq!(
            changes(&r),
            [
                (Add, 0),
                (Hover, 1),
                (Down, 0),
                (Cancel, 1),
                (Remove, 1),
                (Add, 0)
            ]
        );
        assert_eq!(r[0].view_id, 100);
        assert_eq!((delta(&r[1]), r[1].buttons), ((3.0, 4.0), 0));
        assert_eq!((r[2].pointer_identifier, r[2].buttons), (1, 1));
        assert_eq!(
            (r[3].pointer_identifier, at(&r[3]), r[3].buttons),
            (1, (3.0, 4.0), 1)
        );
        assert_eq!((at(&r[4]), r[4].view_id), ((3.0, 4.0), 100));
        assert_eq!(r[5].view_id, 200);
    }

    #[test]
    fn can_avoid_double_remove_after_synthesized_remove() {
        let mut c = Converter::new();
        let r = convert_in(
            &mut c,
            &[100, 200],
            &[
                in_view(100, touch(Add, 0, 0.0, 0.0, 0)),
                in_view(200, touch(Add, 0, 0.0, 0.0, 0)),
            ],
        );
        let got: Vec<(Change, i64, i64)> = r
            .iter()
            .map(|d| (d.change, d.synthesized, d.view_id))
            .collect();
        assert_eq!(got, [(Add, 0, 100), (Remove, 1, 100), (Add, 0, 200)]);
        let r = convert_in(
            &mut c,
            &[100, 200],
            &[in_view(100, touch(Remove, 0, 0.0, 0.0, 0))],
        );
        assert!(r.is_empty(), "{r:?}");
    }

    #[test]
    fn can_handle_three_finger_gesture() {
        let mut c = Converter::new();
        let mut r = convert_in(&mut c, &[0], &[touch(Down, 0, 0.0, 0.0, 1)]);
        r.extend(convert_in(&mut c, &[0], &[touch(Down, 1, 33.0, 44.0, 1)]));
        // The third was never down: its cancel is dropped.
        r.extend(convert_in(
            &mut c,
            &[0],
            &[
                touch(Cancel, 1, 33.0, 44.0, 0),
                touch(Cancel, 0, 0.0, 0.0, 0),
                touch(Cancel, 2, 40.0, 50.0, 0),
            ],
        ));
        let got: Vec<_> = r
            .iter()
            .map(|d| (d.change, d.device, at(d), d.synthesized, d.buttons))
            .collect();
        assert_eq!(
            got,
            [
                (Add, 0, (0.0, 0.0), 1, 0),
                (Down, 0, (0.0, 0.0), 0, 1),
                (Add, 1, (33.0, 44.0), 1, 0),
                (Down, 1, (33.0, 44.0), 0, 1),
                (Cancel, 1, (33.0, 44.0), 0, 0),
                (Cancel, 0, (0.0, 0.0), 0, 0),
            ]
        );
    }

    #[test]
    fn can_convert_pointer_signals() {
        for kind in [
            SignalKind::Scroll,
            SignalKind::ScrollInertiaCancel,
            SignalKind::Scale,
        ] {
            let none = SignalKind::None;
            let r = convert(&[
                mouse(Add, none, 0, (0.0, 0.0), (0.0, 0.0), 0),
                mouse(Add, none, 1, (0.0, 0.0), (0.0, 0.0), 0),
                mouse(Down, none, 1, (0.0, 0.0), (0.0, 0.0), 1),
                mouse(Hover, kind, 0, (34.0, 34.0), (30.0, 0.0), 0),
                mouse(Hover, kind, 1, (49.0, 49.0), (50.0, 0.0), 0),
                mouse(Hover, kind, 2, (10.0, 20.0), (30.0, 40.0), 0),
            ]);
            let got: Vec<_> = r
                .iter()
                .map(|d| (d.change, d.signal_kind, d.device, at(d), d.synthesized))
                .collect();
            assert_eq!(
                got,
                [
                    (Add, none, 0, (0.0, 0.0), 0),
                    (Add, none, 1, (0.0, 0.0), 0),
                    (Down, none, 1, (0.0, 0.0), 0),
                    // Up: a hover to where the signal is.
                    (Hover, none, 0, (34.0, 34.0), 1),
                    (Hover, kind, 0, (34.0, 34.0), 0),
                    // Down: a move, with the buttons it went down with.
                    (Move, none, 1, (49.0, 49.0), 1),
                    (Hover, kind, 1, (49.0, 49.0), 0),
                    // Never added.
                    (Add, none, 2, (10.0, 20.0), 1),
                    (Hover, kind, 2, (10.0, 20.0), 0),
                ],
                "{kind:?}"
            );
            assert_eq!((delta(&r[3]), r[3].buttons), ((34.0, 34.0), 0));
            assert_eq!((r[4].scroll_delta_x, r[4].scroll_delta_y), (30.0, 0.0));
            assert_eq!((delta(&r[5]), r[5].buttons), ((49.0, 49.0), 1));
            assert_eq!((r[6].scroll_delta_x, r[6].scroll_delta_y), (50.0, 0.0));
            assert_eq!((r[8].scroll_delta_x, r[8].scroll_delta_y), (30.0, 40.0));
        }
    }

    #[test]
    fn can_convert_trackpad_gesture() {
        let r = convert(&[
            trackpad(PanZoomStart, 0, (0.0, 0.0), 1.0),
            trackpad(PanZoomUpdate, 0, (3.0, 4.0), 1.0),
            trackpad(PanZoomEnd, 0, (0.0, 0.0), 1.0),
        ]);
        assert_eq!(
            changes(&r),
            [
                (Add, 1),
                (PanZoomStart, 0),
                (PanZoomUpdate, 0),
                (PanZoomEnd, 0)
            ]
        );
        for d in &r {
            assert_eq!(
                (d.signal_kind, d.device, at(d)),
                (SignalKind::None, 0, (0.0, 0.0))
            );
        }
        assert_eq!((r[2].pan_x, r[2].pan_y), (3.0, 4.0));
        assert_eq!((r[2].pan_delta_x, r[2].pan_delta_y), (3.0, 4.0));
        assert_eq!((r[2].scale, r[2].rotation), (1.0, 0.0));
    }

    #[test]
    fn can_convert_view_id() {
        let r = convert_in(
            &mut Converter::new(),
            &[100, 200],
            &[
                in_view(100, touch(Add, 0, 0.0, 0.0, 0)),
                in_view(200, touch(Hover, 0, 1.0, 0.0, 0)),
            ],
        );
        assert_eq!(r.iter().map(|d| d.view_id).collect::<Vec<_>>(), [100, 200]);
    }

    // Not the engine's: what it asserts, or leaves to its callers.

    #[test]
    fn an_event_for_no_view_or_no_pointer_is_dropped() {
        let mut c = Converter::new();
        assert!(convert_in(&mut c, &[0], &[in_view(7, touch(Down, 0, 1.0, 1.0, 1))]).is_empty());
        for change in [Move, Up, Remove, Cancel, PanZoomUpdate, PanZoomEnd] {
            let r = convert_in(&mut c, &[0], &[touch(change, 3, 1.0, 1.0, 0)]);
            assert!(r.is_empty(), "{change:?}: {r:?}");
        }
    }

    #[test]
    fn a_removed_pointer_that_was_down_is_cancelled_where_it_was() {
        let r = convert(&[touch(Down, 0, 1.0, 2.0, 1), touch(Remove, 0, 5.0, 2.0, 0)]);
        assert_eq!(
            changes(&r),
            [(Add, 1), (Down, 0), (Cancel, 1), (Hover, 1), (Remove, 0)]
        );
        assert_eq!(r[2].pointer_identifier, 1);
        assert_eq!(delta(&r[3]), (4.0, 0.0));
    }

    #[test]
    fn a_touch_is_in_contact_while_down_whatever_its_buttons() {
        use std::time::Duration;
        let at = Duration::from_micros(1_234_567);
        let e = |phase| PointerEvent {
            device: 4,
            buttons: 0x40,
            pressure: 0.5,
            ..PointerEvent::new(phase, PointerDeviceKind::Touch, at, 10.5, 20.25)
        };
        let down = PointerData::from_event(&e(PointerPhase::Down));
        assert_eq!(
            (down.change, down.kind, down.buttons),
            (Down, DeviceKind::Touch, 1)
        );
        assert_eq!(
            (down.time_stamp, down.device, down.pressure),
            (1_234_567, 4, 0.5)
        );
        assert_eq!((down.physical_x, down.physical_y), (10.5, 20.25));
        assert_eq!(PointerData::from_event(&e(PointerPhase::Move)).buttons, 1);
        assert_eq!(PointerData::from_event(&e(PointerPhase::Up)).buttons, 0);
        let mouse = PointerEvent {
            device_kind: PointerDeviceKind::Mouse,
            ..e(PointerPhase::Hover)
        };
        let hover = PointerData::from_event(&mouse);
        assert_eq!((hover.kind, hover.buttons), (DeviceKind::Mouse, 0x40));
    }

    /// The layout `_unpackPointerDataPacket` reads: 36 fields of 8 bytes,
    /// little-endian, in `PointerData`'s order. Each field is given its own
    /// index as its value, so a field out of place reads as another's.
    #[test]
    fn a_record_is_36_fields_in_the_engines_order() {
        let d = PointerData {
            embedder_id: 0,
            time_stamp: 1,
            change: Change::Remove,           // 2
            kind: DeviceKind::InvertedStylus, // 3
            signal_kind: SignalKind::None,
            device: 5,
            pointer_identifier: 6,
            physical_x: 7.0,
            physical_y: 8.0,
            physical_delta_x: 9.0,
            physical_delta_y: 10.0,
            buttons: 11,
            obscured: 12,
            synthesized: 13,
            pressure: 14.0,
            pressure_min: 15.0,
            pressure_max: 16.0,
            distance: 17.0,
            distance_max: 18.0,
            size: 19.0,
            radius_major: 20.0,
            radius_minor: 21.0,
            radius_min: 22.0,
            radius_max: 23.0,
            orientation: 24.0,
            tilt: 25.0,
            platform_data: 26,
            scroll_delta_x: 27.0,
            scroll_delta_y: 28.0,
            pan_x: 29.0,
            pan_y: 30.0,
            pan_delta_x: 31.0,
            pan_delta_y: 32.0,
            scale: 33.0,
            rotation: 34.0,
            view_id: 35,
        };
        let bytes = encode_packet(&[d, d]);
        assert_eq!(bytes.len(), 2 * FIELD_COUNT * 8);
        // Which of the 36 are doubles, from pointer_data.h.
        let doubles = [
            7, 8, 9, 10, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 27, 28, 29, 30, 31, 32,
            33, 34,
        ];
        for (n, field) in bytes.chunks_exact(8).enumerate() {
            let i = n % FIELD_COUNT;
            let raw: [u8; 8] = field.try_into().unwrap_or_default();
            let want = if i == 4 { 0 } else { i as i64 };
            if doubles.contains(&i) {
                assert_eq!(f64::from_le_bytes(raw), want as f64, "field {i}");
            } else {
                assert_eq!(i64::from_le_bytes(raw), want, "field {i}");
            }
        }
    }
}
