//! `text_input_model_unittests.cc`, converted case for case
//! (scratch script; the engine's names kept, in snake case).

use super::*;

#[test]
fn set_text() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_text_wide_characters() {
    let mut model = TextInputModel::new();
    model.set_text("😄🙃🤪🧐");
    assert_eq!(model.text(), "😄🙃🤪🧐");
}

#[test]
fn set_text_empty() {
    let mut model = TextInputModel::new();
    model.set_text("");
    assert_eq!(model.text(), "");
}

#[test]
fn set_text_replace_text() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert_eq!(model.text(), "ABCDE");
    model.set_text("");
    assert_eq!(model.text(), "");
}

#[test]
fn set_text_resets_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(3)));
    assert_eq!(model.selection(), TextRange::at(3));
    model.set_text("FGHJI");
    assert_eq!(model.selection(), TextRange::at(0));
}

#[test]
fn set_selection_start() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(0)));
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_selection_composing_start() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(model.set_selection(TextRange::at(1)));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_selection_middle() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_selection_composing_middle() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(model.set_selection(TextRange::at(2)));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_selection_end() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(5)));
    assert_eq!(model.selection(), TextRange::at(5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_selection_composing_end() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(model.set_selection(TextRange::at(4)));
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_selection_wth_extent() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    assert_eq!(model.selection(), TextRange::new(1, 4));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_selection_wth_extent_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(!model.set_selection(TextRange::new(1, 4)));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_selection_reverse_extent() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    assert_eq!(model.selection(), TextRange::new(4, 1));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_selection_reverse_extent_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(!model.set_selection(TextRange::new(4, 1)));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_selection_outside_string() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(!model.set_selection(TextRange::new(4, 6)));
    assert!(!model.set_selection(TextRange::new(5, 6)));
    assert!(!model.set_selection(TextRange::at(6)));
}

#[test]
fn set_selection_outside_composing_range() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(!model.set_selection(TextRange::at(0)));
    assert_eq!(model.selection(), TextRange::at(1));
    assert!(!model.set_selection(TextRange::at(5)));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
}

#[test]
fn set_composing_range_start() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(0, 0), 0));
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_composing_range_middle() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(2, 2), 0));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(2));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_composing_range_end() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(5, 5), 0));
    assert_eq!(model.selection(), TextRange::at(5));
    assert_eq!(model.composing_range(), TextRange::at(5));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_composing_range_with_extent() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 3));
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_composing_range_reverse_extent() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 3));
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn set_composing_range_outside_string() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(!model.set_composing_range(TextRange::new(4, 6), 0));
    assert!(!model.set_composing_range(TextRange::new(5, 6), 0));
    assert!(!model.set_composing_range(TextRange::new(6, 6), 0));
}
// Composing sequence with no initial selection and no text input.

#[test]
fn commit_composing_no_text_with_no_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.set_selection(TextRange::at(0));
    // Verify no changes on BeginComposing.
    model.begin_composing();
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
    // Verify no changes on CommitComposing.
    model.commit_composing();
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
    // Verify no changes on CommitComposing.
    model.end_composing();
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}
// Composing sequence with an initial selection and no text input.

#[test]
fn commit_composing_no_text_with_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.set_selection(TextRange::new(1, 3));
    // Verify no changes on BeginComposing.
    model.begin_composing();
    assert_eq!(model.selection(), TextRange::new(1, 3));
    assert_eq!(model.composing_range(), TextRange::at(1));
    assert_eq!(model.text(), "ABCDE");
    // Verify no changes on CommitComposing.
    model.commit_composing();
    assert_eq!(model.selection(), TextRange::new(1, 3));
    assert_eq!(model.composing_range(), TextRange::at(1));
    assert_eq!(model.text(), "ABCDE");
    // Verify no changes on CommitComposing.
    model.end_composing();
    assert_eq!(model.selection(), TextRange::new(1, 3));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}
// Composing sequence with no initial selection.

#[test]
fn commit_composing_text_with_no_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.set_selection(TextRange::at(1));
    // Verify no changes on BeginComposing.
    model.begin_composing();
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::at(1));
    assert_eq!(model.text(), "ABCDE");
    // Verify selection base, extent and composing extent increment as text is
    // entered. Verify composing base does not change.
    model.update_composing_text("つ");
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::new(1, 2));
    assert_eq!(model.text(), "AつBCDE");
    model.update_composing_text("つる");
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::new(1, 3));
    assert_eq!(model.text(), "AつるBCDE");
    // Verify that cursor position is set to correct offset from composing base.
    model.update_composing_text("鶴");
    assert!(model.set_selection(TextRange::at(1)));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 2));
    assert_eq!(model.text(), "A鶴BCDE");
    // Verify composing base is set to composing extent on commit.
    model.commit_composing();
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(2));
    assert_eq!(model.text(), "A鶴BCDE");
    // Verify that further text entry increments the selection base, extent and
    // the composing extent. Verify that composing base does not change.
    model.update_composing_text("が");
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::new(2, 3));
    assert_eq!(model.text(), "A鶴がBCDE");
    // Verify composing base is set to composing extent on commit.
    model.commit_composing();
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::at(3));
    assert_eq!(model.text(), "A鶴がBCDE");
    // Verify no changes on EndComposing.
    model.end_composing();
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "A鶴がBCDE");
}
// Composing sequence with an initial selection.

#[test]
fn commit_composing_text_with_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.set_selection(TextRange::new(1, 3));
    // Verify no changes on BeginComposing.
    model.begin_composing();
    assert_eq!(model.selection(), TextRange::new(1, 3));
    assert_eq!(model.composing_range(), TextRange::at(1));
    assert_eq!(model.text(), "ABCDE");
    // Verify selection is replaced and selection base, extent and composing
    // extent increment to the position immediately after the composing text.
    // Verify composing base does not change.
    model.update_composing_text("つ");
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::new(1, 2));
    assert_eq!(model.text(), "AつDE");
    // Verify that further text entry increments the selection base, extent and
    // the composing extent. Verify that composing base does not change.
    model.update_composing_text("つる");
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::new(1, 3));
    assert_eq!(model.text(), "AつるDE");
    // Verify that cursor position is set to correct offset from composing base.
    model.update_composing_text("鶴");
    assert!(model.set_selection(TextRange::at(1)));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 2));
    assert_eq!(model.text(), "A鶴DE");
    // Verify composing base is set to composing extent on commit.
    model.commit_composing();
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(2));
    assert_eq!(model.text(), "A鶴DE");
    // Verify that further text entry increments the selection base, extent and
    // the composing extent. Verify that composing base does not change.
    model.update_composing_text("が");
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::new(2, 3));
    assert_eq!(model.text(), "A鶴がDE");
    // Verify composing base is set to composing extent on commit.
    model.commit_composing();
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::at(3));
    assert_eq!(model.text(), "A鶴がDE");
    // Verify no changes on EndComposing.
    model.end_composing();
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "A鶴がDE");
}

#[test]
fn update_composing_removes_last_composing_character() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    model.set_composing_range(TextRange::new(1, 2), 1);
    model.update_composing_text("");
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::at(1));
    model.set_text("ACDE");
}

#[test]
fn update_selection_while_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    model.set_composing_range(TextRange::new(4, 5), 1);
    model.update_composing_text_with(&utf16("ぴょんぴょん"), TextRange::new(3, 6));
    assert_eq!(model.text(), "ABCDぴょんぴょん");
    assert_eq!(model.selection(), TextRange::new(7, 10));
    assert_eq!(model.composing_range(), TextRange::new(4, 10));
}

#[test]
fn add_code_point() {
    let mut model = TextInputModel::new();
    model.add_code_point('A' as u32);
    model.add_code_point('B' as u32);
    model.add_code_point(0x1f604);
    model.add_code_point('D' as u32);
    model.add_code_point('E' as u32);
    assert_eq!(model.selection(), TextRange::at(6));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "AB😄DE");
}

#[test]
fn add_code_point_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    model.add_code_point('x' as u32);
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "AxE");
}

#[test]
fn add_code_point_reverse_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    model.add_code_point('x' as u32);
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "AxE");
}

#[test]
fn add_code_point_selection_wide_character() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    model.add_code_point(0x1f604);
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "A😄E");
}

#[test]
fn add_code_point_reverse_selection_wide_character() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    model.add_code_point(0x1f604);
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "A😄E");
}

#[test]
fn add_text() {
    let mut model = TextInputModel::new();
    model.add_text("ABCDE");
    model.add_text("😄");
    model.add_text("FGHIJ");
    assert_eq!(model.selection(), TextRange::at(12));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE😄FGHIJ");
}

#[test]
fn add_text_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    model.add_text("xy");
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "AxyE");
}

#[test]
fn add_text_reverse_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    model.add_text("xy");
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "AxyE");
}

#[test]
fn add_text_selection_wide_character() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    model.add_text("😄🙃");
    assert_eq!(model.selection(), TextRange::at(5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "A😄🙃E");
}

#[test]
fn add_text_reverse_selection_wide_character() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    model.add_text("😄🙃");
    assert_eq!(model.selection(), TextRange::at(5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "A😄🙃E");
}

#[test]
fn delete_start() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(0)));
    assert!(model.delete());
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "BCDE");
}

#[test]
fn delete_middle() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.delete());
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABDE");
}

#[test]
fn delete_end() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(5)));
    assert!(!model.delete());
    assert_eq!(model.selection(), TextRange::at(5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn delete_wide_characters() {
    let mut model = TextInputModel::new();
    model.set_text("😄🙃🤪🧐");
    assert!(model.set_selection(TextRange::at(4)));
    assert!(model.delete());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "😄🙃🧐");
}

#[test]
fn delete_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    assert!(model.delete());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "AE");
}

#[test]
fn delete_reverse_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    assert!(model.delete());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "AE");
}

#[test]
fn delete_start_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert!(model.delete());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 3));
    assert_eq!(model.text(), "ACDE");
}

#[test]
fn delete_start_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 0));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert!(model.delete());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(3, 1));
    assert_eq!(model.text(), "ACDE");
}

#[test]
fn delete_middle_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 1));
    assert!(model.delete());
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::new(1, 3));
    assert_eq!(model.text(), "ABDE");
}

#[test]
fn delete_middle_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 1));
    assert!(model.delete());
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::new(3, 1));
    assert_eq!(model.text(), "ABDE");
}

#[test]
fn delete_end_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 3));
    assert!(!model.delete());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn delete_end_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 3));
    assert!(!model.delete());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn delete_surrounding_at_cursor() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.delete_surrounding(0, 1));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABDE");
}

#[test]
fn delete_surrounding_at_cursor_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 1));
    assert!(model.delete_surrounding(0, 1));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::new(1, 3));
    assert_eq!(model.text(), "ABDE");
}

#[test]
fn delete_surrounding_at_cursor_all() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.delete_surrounding(0, 3));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "AB");
}

#[test]
fn delete_surrounding_at_cursor_all_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 1));
    assert!(model.delete_surrounding(0, 2));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::new(1, 2));
    assert_eq!(model.text(), "ABE");
}

#[test]
fn delete_surrounding_at_cursor_greedy() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.delete_surrounding(0, 4));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "AB");
}

#[test]
fn delete_surrounding_at_cursor_greedy_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 1));
    assert!(model.delete_surrounding(0, 4));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::new(1, 2));
    assert_eq!(model.text(), "ABE");
}

#[test]
fn delete_surrounding_before_cursor() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.delete_surrounding(-1, 1));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ACDE");
}

#[test]
fn delete_surrounding_before_cursor_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 2));
    assert!(model.delete_surrounding(-1, 1));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::new(1, 3));
    assert_eq!(model.text(), "ABDE");
}

#[test]
fn delete_surrounding_before_cursor_all() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.delete_surrounding(-2, 2));
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "CDE");
}

#[test]
fn delete_surrounding_before_cursor_all_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 2));
    assert!(model.delete_surrounding(-2, 2));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 2));
    assert_eq!(model.text(), "ADE");
}

#[test]
fn delete_surrounding_before_cursor_greedy() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.delete_surrounding(-3, 3));
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "CDE");
}

#[test]
fn delete_surrounding_before_cursor_greedy_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 2));
    assert!(model.delete_surrounding(-3, 3));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 2));
    assert_eq!(model.text(), "ADE");
}

#[test]
fn delete_surrounding_after_cursor() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.delete_surrounding(1, 1));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCE");
}

#[test]
fn delete_surrounding_after_cursor_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(model.delete_surrounding(1, 1));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 3));
    assert_eq!(model.text(), "ABDE");
}

#[test]
fn delete_surrounding_after_cursor_all() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.delete_surrounding(1, 2));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABC");
}

#[test]
fn delete_surrounding_after_cursor_all_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(model.delete_surrounding(1, 2));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 2));
    assert_eq!(model.text(), "ABE");
}

#[test]
fn delete_surrounding_after_cursor_greedy() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.delete_surrounding(1, 3));
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABC");
}

#[test]
fn delete_surrounding_after_cursor_greedy_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(model.delete_surrounding(1, 3));
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 2));
    assert_eq!(model.text(), "ABE");
}

#[test]
fn delete_surrounding_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(2, 3)));
    assert!(model.delete_surrounding(0, 1));
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCE");
}

#[test]
fn delete_surrounding_reverse_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 3)));
    assert!(model.delete_surrounding(0, 1));
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCE");
}

#[test]
fn backspace_start() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(0)));
    assert!(!model.backspace());
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn backspace_middle() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.backspace());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ACDE");
}

#[test]
fn backspace_end() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(5)));
    assert!(model.backspace());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCD");
}

#[test]
fn backspace_wide_characters() {
    let mut model = TextInputModel::new();
    model.set_text("😄🙃🤪🧐");
    assert!(model.set_selection(TextRange::at(4)));
    assert!(model.backspace());
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "😄🤪🧐");
}

#[test]
fn backspace_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    assert!(model.delete());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "AE");
}

#[test]
fn backspace_reverse_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    assert!(model.delete());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "AE");
}

#[test]
fn backspace_start_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(!model.backspace());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn backspace_start_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 0));
    assert!(!model.backspace());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn backspace_middle_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 1));
    assert!(model.backspace());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 3));
    assert_eq!(model.text(), "ACDE");
}

#[test]
fn backspace_middle_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 1));
    assert!(model.backspace());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(3, 1));
    assert_eq!(model.text(), "ACDE");
}

#[test]
fn backspace_end_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 3));
    assert!(model.backspace());
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::new(1, 3));
    assert_eq!(model.text(), "ABCE");
}

#[test]
fn backspace_end_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 3));
    assert!(model.backspace());
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::new(3, 1));
    assert_eq!(model.text(), "ABCE");
}

#[test]
fn move_cursor_forward_start() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(0)));
    assert!(model.move_cursor_forward());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_forward_middle() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.move_cursor_forward());
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_forward_end() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(5)));
    assert!(!model.move_cursor_forward());
    assert_eq!(model.selection(), TextRange::at(5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_forward_wide_characters() {
    let mut model = TextInputModel::new();
    model.set_text("😄🙃🤪🧐");
    assert!(model.set_selection(TextRange::at(4)));
    assert!(model.move_cursor_forward());
    assert_eq!(model.selection(), TextRange::at(6));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "😄🙃🤪🧐");
}

#[test]
fn move_cursor_forward_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    assert!(model.move_cursor_forward());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_forward_reverse_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    assert!(model.move_cursor_forward());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_forward_start_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(model.move_cursor_forward());
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_forward_start_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 0));
    assert!(model.move_cursor_forward());
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_forward_middle_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 1));
    assert!(model.move_cursor_forward());
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_forward_middle_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 1));
    assert!(model.move_cursor_forward());
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_forward_end_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 3));
    assert!(!model.move_cursor_forward());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_forward_end_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 3));
    assert!(!model.move_cursor_forward());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_back_start() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(0)));
    assert!(!model.move_cursor_back());
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_back_middle() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.move_cursor_back());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_back_end() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(5)));
    assert!(model.move_cursor_back());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_back_wide_characters() {
    let mut model = TextInputModel::new();
    model.set_text("😄🙃🤪🧐");
    assert!(model.set_selection(TextRange::at(4)));
    assert!(model.move_cursor_back());
    assert_eq!(model.selection(), TextRange::at(2));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "😄🙃🤪🧐");
}

#[test]
fn move_cursor_back_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    assert!(model.move_cursor_back());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_back_reverse_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    assert!(model.move_cursor_back());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_back_start_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(model.set_selection(TextRange::at(1)));
    assert!(!model.move_cursor_back());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_back_start_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 0));
    assert!(model.set_selection(TextRange::at(1)));
    assert!(!model.move_cursor_back());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_back_middle_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 1));
    assert!(model.move_cursor_back());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_back_middle_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 1));
    assert!(model.move_cursor_back());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_back_end_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 3));
    assert!(model.move_cursor_back());
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_back_end_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 3));
    assert!(model.move_cursor_back());
    assert_eq!(model.selection(), TextRange::at(3));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_beginning_start() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(0)));
    assert!(!model.move_cursor_to_beginning());
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_beginning_start() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(0)));
    assert!(!model.select_to_beginning());
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_beginning_middle() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.move_cursor_to_beginning());
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_beginning_middle() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.select_to_beginning());
    assert_eq!(model.selection(), TextRange::new(2, 0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_beginning_end() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(5)));
    assert!(model.move_cursor_to_beginning());
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_beginning_end() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(5)));
    assert!(model.select_to_beginning());
    assert_eq!(model.selection(), TextRange::new(5, 0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_beginning_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    assert!(model.move_cursor_to_beginning());
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_beginning_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    assert!(model.select_to_beginning());
    assert_eq!(model.selection(), TextRange::new(1, 0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_beginning_reverse_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    assert!(model.move_cursor_to_beginning());
    assert_eq!(model.selection(), TextRange::at(0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_beginning_reverse_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    assert!(model.select_to_beginning());
    assert_eq!(model.selection(), TextRange::new(4, 0));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_beginning_start_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(!model.move_cursor_to_beginning());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_beginning_start_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(!model.select_to_beginning());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_beginning_start_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 0));
    assert!(!model.move_cursor_to_beginning());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_beginning_start_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 0));
    assert!(!model.select_to_beginning());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_beginning_middle_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 1));
    assert!(model.move_cursor_to_beginning());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_beginning_middle_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 1));
    assert!(model.select_to_beginning());
    assert_eq!(model.selection(), TextRange::new(2, 1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_beginning_middle_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 1));
    assert!(model.move_cursor_to_beginning());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_beginning_middle_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 1));
    assert!(model.select_to_beginning());
    assert_eq!(model.selection(), TextRange::new(2, 1));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_beginning_end_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 3));
    assert!(model.move_cursor_to_beginning());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_beginning_end_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 3));
    assert!(model.move_cursor_to_beginning());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_beginning_end_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 3));
    assert!(model.move_cursor_to_beginning());
    assert_eq!(model.selection(), TextRange::at(1));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_beginning_end_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 3));
    assert!(model.select_to_beginning());
    assert_eq!(model.selection(), TextRange::new(4, 1));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_end_start() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(0)));
    assert!(model.move_cursor_to_end());
    assert_eq!(model.selection(), TextRange::at(5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_end_start() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(0)));
    assert!(model.select_to_end());
    assert_eq!(model.selection(), TextRange::new(0, 5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_end_middle() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.move_cursor_to_end());
    assert_eq!(model.selection(), TextRange::at(5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_end_middle() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(2)));
    assert!(model.select_to_end());
    assert_eq!(model.selection(), TextRange::new(2, 5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_end_end() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(5)));
    assert!(!model.move_cursor_to_end());
    assert_eq!(model.selection(), TextRange::at(5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_end_end() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::at(5)));
    assert!(!model.select_to_end());
    assert_eq!(model.selection(), TextRange::at(5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_end_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    assert!(model.move_cursor_to_end());
    assert_eq!(model.selection(), TextRange::at(5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_end_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    assert!(model.select_to_end());
    assert_eq!(model.selection(), TextRange::new(1, 5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_end_reverse_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    assert!(model.move_cursor_to_end());
    assert_eq!(model.selection(), TextRange::at(5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_end_reverse_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    assert!(model.select_to_end());
    assert_eq!(model.selection(), TextRange::new(4, 5));
    assert_eq!(model.composing_range(), TextRange::at(0));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_end_start_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(model.move_cursor_to_end());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_end_start_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(model.select_to_end());
    assert_eq!(model.selection(), TextRange::new(1, 4));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_end_start_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(model.move_cursor_to_end());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_end_start_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 0));
    assert!(model.select_to_end());
    assert_eq!(model.selection(), TextRange::new(1, 4));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_end_middle_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 1));
    assert!(model.move_cursor_to_end());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_end_middle_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 1));
    assert!(model.select_to_end());
    assert_eq!(model.selection(), TextRange::new(2, 4));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_end_middle_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 1));
    assert!(model.move_cursor_to_end());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_end_middle_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 1));
    assert!(model.select_to_end());
    assert_eq!(model.selection(), TextRange::new(2, 4));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_end_end_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 3));
    assert!(!model.move_cursor_to_end());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_end_end_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(1, 4), 3));
    assert!(!model.select_to_end());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(1, 4));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn move_cursor_to_end_end_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 3));
    assert!(!model.move_cursor_to_end());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn select_to_end_end_reverse_composing() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    model.begin_composing();
    assert!(model.set_composing_range(TextRange::new(4, 1), 3));
    assert!(!model.select_to_end());
    assert_eq!(model.selection(), TextRange::at(4));
    assert_eq!(model.composing_range(), TextRange::new(4, 1));
    assert_eq!(model.text(), "ABCDE");
}

#[test]
fn get_cursor_offset() {
    let mut model = TextInputModel::new();
    // These characters take 1, 2, 3 and 4 bytes in UTF-8.
    model.set_text("$¢€𐍈");
    assert!(model.set_selection(TextRange::at(0)));
    assert_eq!(model.cursor_offset(), 0);
    assert!(model.move_cursor_forward());
    assert_eq!(model.cursor_offset(), 1);
    assert!(model.move_cursor_forward());
    assert_eq!(model.cursor_offset(), 3);
    assert!(model.move_cursor_forward());
    assert_eq!(model.cursor_offset(), 6);
    assert!(model.move_cursor_forward());
    assert_eq!(model.cursor_offset(), 10);
}

#[test]
fn get_cursor_offset_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(1, 4)));
    assert_eq!(model.cursor_offset(), 4);
}

#[test]
fn get_cursor_offset_reverse_selection() {
    let mut model = TextInputModel::new();
    model.set_text("ABCDE");
    assert!(model.set_selection(TextRange::new(4, 1)));
    assert_eq!(model.cursor_offset(), 1);
}
