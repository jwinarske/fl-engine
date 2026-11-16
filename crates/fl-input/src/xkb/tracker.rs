//! Which keys the framework has been told are down (D7): the Linux
//! embedder's `fl_key_embedder_responder`, ported, its unit tests with it.
//!
//! The framework asserts that a key goes down once, comes up once, and
//! that a lock key's presses pair with its lock's state. Native events do
//! not promise that: a key pressed while another window had the keyboard,
//! a release never delivered. Each event carries the modifier and lock
//! state it was made in, and where that disagrees with what has been
//! sent, events are synthesized first to make the two agree.
//!
//! Not ported: the embedder's inference of when GDK's Caps Lock bit turns
//! (`caps_lock_state_logic_inferrence`). A source here reads the state
//! from its keymap, before the event, on every platform alike, which is
//! the embedder's "normal" logic.

use std::collections::BTreeMap;
use std::time::Duration;

use super::{logical_key, physical_key, state, XkbKey};
use crate::{KeyData, KeyEventType};

/// A modifier whose state bit is checked against the keys sent as down
/// (`FlKeyEmbedderCheckedKey`).
struct Checked {
    bit: u32,
    /// The key synthesized as going down when none of them is.
    primary_physical: u64,
    primary_logical: u64,
    /// The other key of the pair, or 0.
    secondary_logical: u64,
}

/// `initialize_modifier_bit_to_checked_keys`.
const MODIFIERS: [Checked; 4] = [
    Checked {
        bit: state::SHIFT,
        primary_physical: 0x0007_00e1,    // shiftLeft
        primary_logical: 0x2_0000_0102,   // shiftLeft
        secondary_logical: 0x2_0000_0103, // shiftRight
    },
    Checked {
        bit: state::CONTROL,
        primary_physical: 0x0007_00e0,    // controlLeft
        primary_logical: 0x2_0000_0100,   // controlLeft
        secondary_logical: 0x2_0000_0101, // controlRight
    },
    Checked {
        bit: state::MOD1,
        primary_physical: 0x0007_00e2,    // altLeft
        primary_logical: 0x2_0000_0104,   // altLeft
        secondary_logical: 0x2_0000_0105, // altRight
    },
    Checked {
        bit: state::META,
        primary_physical: 0x0007_00e3,    // metaLeft
        primary_logical: 0x2_0000_0106,   // metaLeft
        secondary_logical: 0x2_0000_0107, // metaRight
    },
];

/// `initialize_lock_bit_to_checked_keys`.
const LOCKS: [Checked; 2] = [
    Checked {
        bit: state::LOCK,
        primary_physical: 0x0007_0039,  // capsLock
        primary_logical: 0x1_0000_0104, // capsLock
        secondary_logical: 0,
    },
    Checked {
        bit: state::MOD2,
        primary_physical: 0x0007_0053,  // numLock
        primary_logical: 0x1_0000_010a, // numLock
        secondary_logical: 0,
    },
];

/// A lock key cycles through four stages, a press and a release with its
/// lock on and a press and a release with it off.
const STAGES: usize = 4;

/// The key state the framework has been told.
#[derive(Debug, Default)]
pub struct Tracker {
    /// Keys down: physical to the logical key each went down as.
    pressing: BTreeMap<u64, u64>,
    /// Locks on ([`state`] bits), counted from the lock keys' downs.
    lock_records: u32,
    /// The physical key each modifier and lock key last went down on, so
    /// a synthesized event names the key the user has. The embedder keeps
    /// every logical key's and reads only these; kept for these alone, a
    /// source's keysyms cannot grow it.
    mapping: BTreeMap<u64, u64>,
}

impl Tracker {
    /// The keys down, physical to logical
    /// (`fl_key_embedder_responder_get_pressed_state`).
    pub fn pressed(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.pressing.iter().map(|(p, l)| (*p, *l))
    }

    /// One native event: what to send for it, appended to `out`, the
    /// synthesized first. `specified_logical` is the layout's logical key
    /// for it, or 0 for its keysym's
    /// (`fl_key_embedder_responder_handle_event`).
    pub fn handle(&mut self, key: &XkbKey, specified_logical: u64, out: &mut Vec<KeyData>) {
        let sent = out.len();
        self.handle_impl(key, specified_logical, out);
        if out.len() == sent {
            // The framework expects one event at least for each native one.
            out.push(KeyData::empty());
        }
    }

    fn handle_impl(&mut self, key: &XkbKey, specified_logical: u64, out: &mut Vec<KeyData>) {
        let logical = if specified_logical != 0 {
            specified_logical
        } else {
            logical_key(key.keyval)
        };
        let physical = corrected_modifier_physical_key(physical_key(key.keycode), logical);

        self.synchronize_lock_states(key.state, key.time, key.pressed, logical, out);
        self.synchronize_pressed_states(key.state, key.time, out);

        // A key is the logical key it went down as until it comes up.
        let last_logical = self.pressing.get(&physical).copied().unwrap_or(0);
        let (kind, character) = if key.pressed {
            let kind = if last_logical != 0 {
                KeyEventType::Repeat
            } else {
                KeyEventType::Down
            };
            let character = char::from_u32(key.unicode)
                .filter(|c| *c != '\0')
                .map(String::from);
            (kind, character)
        } else if last_logical == 0 {
            // An up for a key not down, a release whose press went
            // elsewhere: nothing to tell.
            return;
        } else {
            (KeyEventType::Up, None)
        };
        if kind != KeyEventType::Repeat {
            self.update_pressing_state(physical, if key.pressed { logical } else { 0 });
        }
        self.possibly_update_lock_bit(logical, key.pressed);
        if key.pressed {
            self.update_mapping_record(physical, logical);
        }
        out.push(KeyData {
            timestamp: key.time,
            kind,
            physical,
            logical: if last_logical != 0 {
                last_logical
            } else {
                logical
            },
            character,
            synthesized: false,
        });
    }

    /// Makes the modifiers sent as down agree with `state`, for an event
    /// that is no key's (`fl_key_embedder_responder_sync_modifiers_if_needed`).
    pub fn sync_modifiers(&mut self, state: u32, time: Duration, out: &mut Vec<KeyData>) {
        self.synchronize_pressed_states(state, time, out);
    }

    /// `update_pressing_state`: `logical` of 0 is the key coming up.
    fn update_pressing_state(&mut self, physical: u64, logical: u64) {
        if logical != 0 {
            // The embedder returns if it is down already.
            self.pressing.entry(physical).or_insert(logical);
        } else {
            self.pressing.remove(&physical);
        }
    }

    /// `update_mapping_record`, for the keys whose record is ever read.
    fn update_mapping_record(&mut self, physical: u64, logical: u64) {
        let read = MODIFIERS
            .iter()
            .chain(&LOCKS)
            .any(|c| c.primary_logical == logical || c.secondary_logical == logical);
        if read {
            self.mapping.insert(logical, physical);
        }
    }

    /// `possibly_update_lock_bit`: a lock key's down turns its lock.
    fn possibly_update_lock_bit(&mut self, logical: u64, is_down: bool) {
        if !is_down {
            return;
        }
        if let Some(lock) = LOCKS.iter().find(|l| l.primary_logical == logical) {
            self.lock_records ^= lock.bit;
        }
    }

    /// Whether `logical` is sent as down on any key.
    fn is_pressing(&self, logical: u64) -> bool {
        self.pressing.values().any(|l| *l == logical)
    }

    fn synthesize(kind: KeyEventType, physical: u64, logical: u64, time: Duration) -> KeyData {
        KeyData {
            timestamp: time,
            kind,
            physical,
            logical,
            character: None,
            synthesized: true,
        }
    }

    /// `synchronize_pressed_states`: a modifier down by the record and up
    /// by the state comes up; one down by the state with neither of its
    /// keys down goes down, as its primary key.
    fn synchronize_pressed_states(&mut self, state: u32, time: Duration, out: &mut Vec<KeyData>) {
        for checked in &MODIFIERS {
            let any_pressed_by_state = state & checked.bit != 0;
            let mut any_pressed_by_record = false;
            for logical in [checked.primary_logical, checked.secondary_logical] {
                if logical == 0 {
                    continue;
                }
                let pressed_before = self.is_pressing(logical);
                any_pressed_by_record |= pressed_before;
                if pressed_before && !any_pressed_by_state {
                    // Every key down as it comes up. The embedder releases
                    // the key the logical key last went down on, which is
                    // another key's, or none, once two keys have been down
                    // as it and the later one has come up: the first then
                    // stays down for good.
                    let down: Vec<u64> = self
                        .pressing
                        .iter()
                        .filter(|(_, l)| **l == logical)
                        .map(|(p, _)| *p)
                        .collect();
                    for physical in down {
                        out.push(Self::synthesize(KeyEventType::Up, physical, logical, time));
                        self.update_pressing_state(physical, 0);
                    }
                }
            }
            if any_pressed_by_state && !any_pressed_by_record {
                let logical = checked.primary_logical;
                let physical = match self.mapping.get(&logical) {
                    Some(recorded) => *recorded,
                    None => {
                        self.mapping.insert(logical, checked.primary_physical);
                        checked.primary_physical
                    }
                };
                out.push(Self::synthesize(
                    KeyEventType::Down,
                    physical,
                    logical,
                    time,
                ));
                self.update_pressing_state(physical, logical);
            }
        }
    }

    /// `synchronize_lock_states`: each lock key is brought, by synthesized
    /// presses and releases, to the stage the event's state says it is at.
    fn synchronize_lock_states(
        &mut self,
        state: u32,
        time: Duration,
        is_down: bool,
        event_logical: u64,
        out: &mut Vec<KeyData>,
    ) {
        for checked in &LOCKS {
            let logical = checked.primary_logical;
            let recorded_physical = self.mapping.get(&logical).copied();
            let physical = recorded_physical.unwrap_or(checked.primary_physical);
            let pressed_logical = recorded_physical
                .and_then(|p| self.pressing.get(&p).copied())
                .unwrap_or(0);
            if pressed_logical != 0 && pressed_logical != logical {
                // The embedder returns: the lock's key is down as another.
                return;
            }
            let stage_by_record =
                stage_by_record(pressed_logical != 0, self.lock_records & checked.bit != 0);
            let enabled_by_state = state & checked.bit != 0;
            let stage_by_event = if logical == event_logical {
                stage_by_self_event(stage_by_record, is_down, enabled_by_state)
            } else {
                stage_by_others_event(stage_by_record, enabled_by_state)
            };
            // Stages only go forward, around the cycle.
            let destination = if stage_by_event >= stage_by_record {
                stage_by_event
            } else {
                stage_by_event + STAGES
            };
            for stage in stage_by_record..destination {
                let is_down_event = stage % STAGES == 0 || stage % STAGES == 2;
                if is_down_event && recorded_physical.is_none() {
                    self.mapping.insert(logical, physical);
                }
                let kind = if is_down_event {
                    KeyEventType::Down
                } else {
                    KeyEventType::Up
                };
                self.update_pressing_state(physical, if is_down_event { logical } else { 0 });
                self.possibly_update_lock_bit(logical, is_down_event);
                out.push(Self::synthesize(kind, physical, logical, time));
            }
        }
    }
}

/// `find_stage_by_record`: the stage a lock key is at, from whether it is
/// down and whether its lock is on.
fn stage_by_record(is_down: bool, is_enabled: bool) -> usize {
    match (is_down, is_enabled) {
        (false, false) => 0,
        (true, true) => 1,
        (false, true) => 2,
        (true, false) => 3,
    }
}

/// `find_stage_by_self_event`, with the state read before the event: the
/// stage the lock key is at as its own event arrives.
fn stage_by_self_event(stage_by_record: usize, is_down_event: bool, is_state_on: bool) -> usize {
    if !is_state_on {
        0
    } else if is_down_event {
        2
    } else {
        stage_by_record
    }
}

/// `find_stage_by_others_event`: the stage another key's event says a lock
/// key is at. Only whether the lock is on can be told.
fn stage_by_others_event(stage_by_record: usize, is_state_on: bool) -> usize {
    if !is_state_on {
        0
    } else if stage_by_record == 0 {
        1
    } else {
        stage_by_record
    }
}

/// `corrected_modifier_physical_key`: a modifier's key that gives another
/// modifier's keysym (Shift and Alt giving Meta, a layout switch) is sent
/// as that modifier's own key, so each logical modifier keeps one physical
/// key.
fn corrected_modifier_physical_key(physical_from_event: u64, logical: u64) -> u64 {
    if !MODIFIERS
        .iter()
        .any(|c| c.primary_physical == physical_from_event)
    {
        return physical_from_event;
    }
    MODIFIERS
        .iter()
        .rev()
        .find(|c| c.primary_logical == logical || c.secondary_logical == logical)
        .map_or(physical_from_event, |c| c.primary_physical)
}

#[cfg(test)]
#[path = "tracker_tests.rs"]
mod tests;
