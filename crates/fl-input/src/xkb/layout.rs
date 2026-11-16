//! Logical keys a layout does not give by itself: `fl_keyboard_manager`'s
//! `guarantee_layout` and `fl_keyboard_layout`, ported.
//!
//! A shortcut names a logical key, Control and `keyC`. On a layout whose
//! keys type no Latin letters (Russian), or type them elsewhere (French),
//! the embedder still gives every letter and digit to some key: to the key
//! that types it if one does, else to the key that has it on a US layout.

use std::collections::BTreeMap;

use super::tables::LAYOUT_GOALS;

/// Keycodes a layout is kept for (`kLayoutSize`).
const LAYOUT_SIZE: u16 = 128;
/// Past this a keysym is no character of the Basic Multilingual Plane
/// the embedder reads (`kBmpMax`).
const BMP_MAX: u32 = 0xd7ff;

/// The derived logical keys, by layout group and keycode.
#[derive(Debug, Default)]
pub struct Layout {
    groups: BTreeMap<u8, BTreeMap<u16, u64>>,
}

impl Layout {
    /// The logical key the embedder specifies for `keycode` in `group`, or
    /// 0 for a key whose keysym is to decide. The group is derived the
    /// first time one of its goal keys is asked for; `lookup` gives the
    /// keysym of a keycode at a group and a shift level.
    pub fn logical_key(
        &mut self,
        group: u8,
        keycode: u16,
        lookup: impl Fn(u16, u8, u8) -> u32,
    ) -> u64 {
        self.guarantee(group, keycode, lookup);
        self.get(group, keycode)
    }

    /// `fl_keyboard_layout_get_logical_key`.
    fn get(&self, group: u8, keycode: u16) -> u64 {
        if keycode >= LAYOUT_SIZE {
            return 0;
        }
        self.groups
            .get(&group)
            .and_then(|g| g.get(&keycode))
            .copied()
            .unwrap_or(0)
    }

    fn set(&mut self, group: u8, keycode: u16, logical: u64) {
        self.groups
            .entry(group)
            .or_default()
            .insert(keycode, logical);
    }

    /// `guarantee_layout`.
    fn guarantee(&mut self, group: u8, keycode: u16, lookup: impl Fn(u16, u8, u8) -> u32) {
        if self.groups.contains_key(&group) {
            return;
        }
        if !LAYOUT_GOALS.iter().any(|(code, _, _)| *code == keycode) {
            return;
        }
        let clue = |keycode: u16, level: u8| -> u64 {
            let origin = lookup(keycode, group, level);
            u64::from(if origin < BMP_MAX { origin } else { 0xffff })
        };
        // Mandatory goals no key has been found for yet, by logical key.
        let mut remaining: BTreeMap<u64, u16> = LAYOUT_GOALS
            .iter()
            .filter(|(_, _, mandatory)| *mandatory)
            .map(|(code, logical, _)| (*logical, *code))
            .collect();
        for (keycode, us_logical, _) in LAYOUT_GOALS {
            let clues = [clue(*keycode, 0), clue(*keycode, 1)];
            // A key that types a mandatory character is that key.
            for c in clues {
                if remaining.remove(&c).is_some() {
                    self.set(group, *keycode, c);
                    break;
                }
            }
            // A key that types nothing in Latin-1 is what it is on a US
            // layout.
            let any_eascii = clues.iter().any(|c| *c < 256);
            if self.get(group, *keycode) == 0 && !any_eascii {
                self.set(group, *keycode, *us_logical);
            }
        }
        // Whatever is still wanted goes to its US key.
        for (logical, keycode) in remaining {
            self.set(group, keycode, logical);
        }
    }
}
