//! The mouse cursor (brief M4 §2, D8): what the framework asks for on
//! `flutter/mousecursor`, and the platform that shows it.

/// The platform's mouse cursor.
pub trait MouseCursor: Send {
    /// Shows the cursor of this name from now: a cursor theme's name
    /// ([`cursor_name`]), or `none` for no cursor at all.
    fn activate(&mut self, name: &str);
}

/// The cursor shown for a kind the table has not.
pub const FALLBACK_CURSOR: &str = "default";

/// The cursor theme's name for a `SystemMouseCursor`'s kind: the Linux
/// embedder's table (`fl_mouse_cursor_handler.cc`), the CSS names GDK and
/// the cursor themes share.
pub fn cursor_name(kind: &str) -> &'static str {
    match kind {
        "alias" => "alias",
        "allScroll" => "all-scroll",
        "basic" => "default",
        "cell" => "cell",
        "click" => "pointer",
        "contextMenu" => "context-menu",
        "copy" => "copy",
        "forbidden" => "not-allowed",
        "grab" => "grab",
        "grabbing" => "grabbing",
        "help" => "help",
        "move" => "move",
        "none" => "none",
        "noDrop" => "no-drop",
        "precise" => "crosshair",
        "progress" => "progress",
        "text" => "text",
        "resizeColumn" => "col-resize",
        "resizeDown" => "s-resize",
        "resizeDownLeft" => "sw-resize",
        "resizeDownRight" => "se-resize",
        "resizeLeft" => "w-resize",
        "resizeLeftRight" => "ew-resize",
        "resizeRight" => "e-resize",
        "resizeRow" => "row-resize",
        "resizeUp" => "n-resize",
        "resizeUpDown" => "ns-resize",
        "resizeUpLeft" => "nw-resize",
        "resizeUpRight" => "ne-resize",
        "resizeUpLeftDownRight" => "nwse-resize",
        "resizeUpRightDownLeft" => "nesw-resize",
        "verticalText" => "vertical-text",
        "wait" => "wait",
        "zoomIn" => "zoom-in",
        "zoomOut" => "zoom-out",
        _ => FALLBACK_CURSOR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_is_its_theme_name_and_one_unknown_is_the_default() {
        assert_eq!(cursor_name("basic"), "default");
        assert_eq!(cursor_name("click"), "pointer");
        assert_eq!(cursor_name("resizeUpLeftDownRight"), "nwse-resize");
        assert_eq!(cursor_name("none"), "none");
        assert_eq!(cursor_name("a kind of a later release"), "default");
        assert_eq!(cursor_name(""), "default");
    }
}
