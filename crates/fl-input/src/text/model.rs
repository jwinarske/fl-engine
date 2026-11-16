//! The text an input method edits (brief M4 §2): the engine's
//! `TextInputModel` (`shell/platform/common/text_input_model.cc`) and its
//! `TextRange`, ported. Offsets are UTF-16 code units, as the framework
//! counts them.
//!
//! Where the original trusts its ranges to be inside the text, this clamps
//! them: a range the framework sent that is not inside the text is refused
//! as there, and nothing here indexes with one.

/// `TextRange`: a base and an extent, the extent where the caret is. A
/// range with the two the same is a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct TextRange {
    base: usize,
    extent: usize,
}

impl TextRange {
    pub const fn new(base: usize, extent: usize) -> TextRange {
        TextRange { base, extent }
    }

    /// A collapsed range: a position.
    pub const fn at(position: usize) -> TextRange {
        TextRange {
            base: position,
            extent: position,
        }
    }

    pub fn base(&self) -> usize {
        self.base
    }

    pub fn extent(&self) -> usize {
        self.extent
    }

    pub fn start(&self) -> usize {
        self.base.min(self.extent)
    }

    pub fn end(&self) -> usize {
        self.base.max(self.extent)
    }

    /// Moves the end, whichever of the two it is.
    pub fn set_end(&mut self, pos: usize) {
        if self.base <= self.extent {
            self.extent = pos;
        } else {
            self.base = pos;
        }
    }

    /// Where a collapsed range is: its extent.
    pub fn position(&self) -> usize {
        self.extent
    }

    pub fn length(&self) -> usize {
        self.end() - self.start()
    }

    pub fn collapsed(&self) -> bool {
        self.base == self.extent
    }

    pub fn reversed(&self) -> bool {
        self.base > self.extent
    }

    pub fn contains(&self, range: &TextRange) -> bool {
        range.start() >= self.start() && range.end() <= self.end()
    }
}

fn is_leading_surrogate(unit: u16) -> bool {
    unit & 0xfc00 == 0xd800
}

fn is_trailing_surrogate(unit: u16) -> bool {
    unit & 0xfc00 == 0xdc00
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `TextInputModel`: a text, its selection, and the range an input method
/// is composing in, if one is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInputModel {
    text: Vec<u16>,
    selection: TextRange,
    composing_range: TextRange,
    composing: bool,
}

impl TextInputModel {
    pub fn new() -> TextInputModel {
        TextInputModel::default()
    }

    /// Replaces the text; the selection and the composing range go to its
    /// start.
    pub fn set_text(&mut self, text: &str) {
        self.set_text_with(text, TextRange::at(0), TextRange::at(0));
    }

    /// Replaces the text, the selection and the composing range. `false`
    /// if either range is not inside the text; both are then at its start.
    pub fn set_text_with(
        &mut self,
        text: &str,
        selection: TextRange,
        composing_range: TextRange,
    ) -> bool {
        self.text = utf16(text);
        if !self.text_range().contains(&selection) || !self.text_range().contains(&composing_range)
        {
            // The original leaves the ranges it had, which may be past the
            // new text's end.
            self.selection = TextRange::at(0);
            self.composing_range = TextRange::at(0);
            self.composing = false;
            return false;
        }
        self.selection = selection;
        self.composing_range = composing_range;
        self.composing = !composing_range.collapsed();
        true
    }

    /// Sets the selection. `false` if it is not inside what can be edited:
    /// the composing range while composing, where it must also be
    /// collapsed, or else the text.
    pub fn set_selection(&mut self, range: TextRange) -> bool {
        if self.composing && !range.collapsed() {
            return false;
        }
        if !self.editable_range().contains(&range) {
            return false;
        }
        self.selection = range;
        true
    }

    /// Sets the composing range, and the caret `cursor_offset` from its
    /// start. `false` if not composing, or the range is not inside the
    /// text.
    pub fn set_composing_range(&mut self, range: TextRange, cursor_offset: usize) -> bool {
        if !self.composing || !self.text_range().contains(&range) {
            return false;
        }
        self.composing_range = range;
        // The original's sum is unchecked; a caret past the text is held
        // to its end.
        let caret = range.start().saturating_add(cursor_offset);
        self.selection = TextRange::at(caret.min(self.text.len()));
        true
    }

    /// Starts composing, at the selection's start.
    pub fn begin_composing(&mut self) {
        self.composing = true;
        self.composing_range = TextRange::at(self.selection.start());
    }

    /// Replaces the composing text, or the selection if there is none,
    /// with `text`; `selection` is within the new composing text.
    pub fn update_composing_text_with(&mut self, text: &[u16], selection: TextRange) {
        if text.is_empty() && self.composing_range.collapsed() {
            return;
        }
        let delete = if self.composing_range.collapsed() {
            self.selection
        } else {
            self.composing_range
        };
        let (start, end) = self.clamped(delete);
        self.text.splice(start..end, text.iter().copied());
        // The original keeps a collapsed composing range where it was and
        // writes at the selection's start; they are the same place while
        // composing began here.
        let start = self.composing_range.start().min(self.text.len());
        self.composing_range = if self.composing_range.reversed() {
            TextRange::new(start + text.len(), start)
        } else {
            TextRange::new(start, start + text.len())
        };
        self.composing_range = self.held(self.composing_range);
        // The original's base is the selection's start, not its base.
        self.selection = self.held(TextRange::new(
            selection.start().saturating_add(start),
            selection.extent().saturating_add(start),
        ));
    }

    /// Replaces the composing text with `text`, the caret after it.
    pub fn update_composing_text(&mut self, text: &str) {
        let text = utf16(text);
        let end = TextRange::at(text.len());
        self.update_composing_text_with(&text, end);
    }

    /// Makes the composing text the text's own: the caret after it, and
    /// composing goes on from there.
    pub fn commit_composing(&mut self) {
        if self.composing_range.collapsed() {
            return;
        }
        self.composing_range = TextRange::at(self.composing_range.end());
        self.selection = self.composing_range;
    }

    /// Ends composing; the text stays as it is.
    pub fn end_composing(&mut self) {
        self.composing = false;
        self.composing_range = TextRange::at(0);
    }

    fn delete_selected(&mut self) -> bool {
        if self.selection.collapsed() {
            return false;
        }
        let (start, end) = self.clamped(self.selection);
        self.text.drain(start..end);
        self.selection = TextRange::at(start);
        if self.composing {
            self.composing_range = self.selection;
        }
        true
    }

    /// Adds a character at the caret, in place of the selection.
    pub fn add_code_point(&mut self, c: u32) {
        match char::from_u32(c) {
            Some(c) => {
                let mut units = [0; 2];
                let units = c.encode_utf16(&mut units);
                self.add_units(units);
            }
            // A lone surrogate, as the original passes one through.
            None if c <= 0xffff => self.add_units(&[c as u16]),
            None => {}
        }
    }

    /// Adds `text` at the caret, in place of the selection and of any
    /// composing text.
    pub fn add_text(&mut self, text: &str) {
        self.add_units(&utf16(text));
    }

    fn add_units(&mut self, text: &[u16]) {
        self.delete_selected();
        if self.composing {
            let (start, end) = self.clamped(self.composing_range);
            self.text.drain(start..end);
            self.selection = TextRange::at(start);
            self.composing_range = TextRange::new(start, start + text.len());
        }
        let position = self.selection.position().min(self.text.len());
        self.text.splice(position..position, text.iter().copied());
        self.selection = TextRange::at(position + text.len());
    }

    /// The units of the character before `position`, which is past
    /// `floor`: 2 for a surrogate pair.
    fn units_before(&self, position: usize) -> usize {
        match position.checked_sub(1).and_then(|i| self.text.get(i)) {
            Some(u) if is_trailing_surrogate(*u) && position >= 2 => 2,
            _ => 1,
        }
    }

    /// The units of the character at `position`, which is before the end.
    fn units_at(&self, position: usize) -> usize {
        match self.text.get(position) {
            Some(u) if is_leading_surrogate(*u) && position + 2 <= self.text.len() => 2,
            _ => 1,
        }
    }

    /// Deletes the selection, or the character before the caret. `false`
    /// if there was nothing to delete.
    pub fn backspace(&mut self) -> bool {
        if self.delete_selected() {
            return true;
        }
        let position = self.selection.position().min(self.text.len());
        if position > self.editable_range().start() {
            let count = self.units_before(position);
            self.text.drain(position - count..position);
            self.selection = TextRange::at(position - count);
            if self.composing {
                let end = self.composing_range.end().saturating_sub(count);
                self.composing_range.set_end(end);
            }
            return true;
        }
        false
    }

    /// Deletes the selection, or the character after the caret. `false`
    /// if there was nothing to delete.
    pub fn delete(&mut self) -> bool {
        if self.delete_selected() {
            return true;
        }
        let position = self.selection.position();
        if position < self.editable_range().end().min(self.text.len()) {
            let count = self.units_at(position);
            self.text.drain(position..position + count);
            if self.composing {
                let end = self.composing_range.end().saturating_sub(count);
                self.composing_range.set_end(end);
            }
            return true;
        }
        false
    }

    /// Deletes `count` characters starting `offset_from_cursor` characters
    /// from the caret, before it if negative, within what can be edited.
    /// `false` if nothing was deleted.
    pub fn delete_surrounding(&mut self, offset_from_cursor: i32, count: i32) -> bool {
        let editable = self.editable_range();
        let max_pos = editable.end().min(self.text.len());
        let mut start = self.selection.extent().min(self.text.len());
        let mut count = count;
        if offset_from_cursor < 0 {
            for i in 0..offset_from_cursor.unsigned_abs() {
                if start <= editable.start() {
                    count = i32::try_from(i).unwrap_or(i32::MAX);
                    break;
                }
                start -= self.units_before(start);
            }
        } else {
            for _ in 0..offset_from_cursor {
                if start >= max_pos {
                    break;
                }
                start += self.units_at(start);
            }
        }

        let mut end = start;
        for _ in 0..count.max(0) {
            if end >= max_pos {
                break;
            }
            // As the original, each step is as wide as the first character.
            end += self.units_at(start);
        }
        let end = end.min(self.text.len());
        if start >= end {
            return false;
        }

        let deleted = end - start;
        self.text.drain(start..end);
        self.selection = TextRange::at(if offset_from_cursor <= 0 {
            start
        } else {
            self.selection.start().min(self.text.len())
        });
        if self.composing {
            let end = self.composing_range.end().saturating_sub(deleted);
            self.composing_range.set_end(end);
        }
        true
    }

    /// Moves the caret to the start of what can be edited. `false` if it
    /// was there.
    pub fn move_cursor_to_beginning(&mut self) -> bool {
        let min_pos = self.editable_range().start();
        if self.selection.collapsed() && self.selection.position() == min_pos {
            return false;
        }
        self.selection = TextRange::at(min_pos);
        true
    }

    /// Moves the caret to the end of what can be edited. `false` if it was
    /// there.
    pub fn move_cursor_to_end(&mut self) -> bool {
        let max_pos = self.editable_range().end();
        if self.selection.collapsed() && self.selection.position() == max_pos {
            return false;
        }
        self.selection = TextRange::at(max_pos);
        true
    }

    /// Extends the selection to the start of what can be edited.
    pub fn select_to_beginning(&mut self) -> bool {
        let min_pos = self.editable_range().start();
        if self.selection.collapsed() && self.selection.position() == min_pos {
            return false;
        }
        self.selection = TextRange::new(self.selection.base(), min_pos);
        true
    }

    /// Extends the selection to the end of what can be edited.
    pub fn select_to_end(&mut self) -> bool {
        let max_pos = self.editable_range().end();
        if self.selection.collapsed() && self.selection.position() == max_pos {
            return false;
        }
        self.selection = TextRange::new(self.selection.base(), max_pos);
        true
    }

    /// Moves the caret a character on, or to the selection's end.
    pub fn move_cursor_forward(&mut self) -> bool {
        if !self.selection.collapsed() {
            self.selection = TextRange::at(self.selection.end());
            return true;
        }
        let position = self.selection.position();
        if position < self.editable_range().end().min(self.text.len()) {
            self.selection = TextRange::at(position + self.units_at(position));
            return true;
        }
        false
    }

    /// Moves the caret a character back, or to the selection's start.
    pub fn move_cursor_back(&mut self) -> bool {
        if !self.selection.collapsed() {
            self.selection = TextRange::at(self.selection.start());
            return true;
        }
        let position = self.selection.position().min(self.text.len());
        if position > self.editable_range().start() {
            self.selection = TextRange::at(position - self.units_before(position));
            return true;
        }
        false
    }

    /// The text, as UTF-8. A surrogate with no pair comes out U+FFFD.
    pub fn text(&self) -> String {
        String::from_utf16_lossy(&self.text)
    }

    /// The caret's offset in the UTF-8 text, in bytes.
    pub fn cursor_offset(&self) -> usize {
        let end = self.selection.extent().min(self.text.len());
        String::from_utf16_lossy(&self.text[..end]).len()
    }

    /// The whole text's range, in UTF-16 code units.
    pub fn text_range(&self) -> TextRange {
        TextRange::new(0, self.text.len())
    }

    pub fn selection(&self) -> TextRange {
        self.selection
    }

    pub fn composing_range(&self) -> TextRange {
        self.composing_range
    }

    pub fn composing(&self) -> bool {
        self.composing
    }

    /// What can be edited: the composing range while composing, or the
    /// text.
    fn editable_range(&self) -> TextRange {
        if self.composing {
            self.composing_range
        } else {
            self.text_range()
        }
    }

    /// A range's start and end, held inside the text.
    fn clamped(&self, range: TextRange) -> (usize, usize) {
        let len = self.text.len();
        (range.start().min(len), range.end().min(len))
    }

    /// A range with both ends held inside the text.
    fn held(&self, range: TextRange) -> TextRange {
        let len = self.text.len();
        TextRange::new(range.base().min(len), range.extent().min(len))
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
