//! The clipboard (brief M4 D4): text, as the Linux embedder's
//! `GtkClipboard` is used (`fl_platform_handler.cc`).

/// The platform's clipboard. Text only: the framework asks for no other
/// format, and the embedder answers any other with an error.
pub trait Clipboard: Send {
    /// Puts `text` on the clipboard, in place of what was there.
    fn set_text(&mut self, text: &str);

    /// The clipboard's text, if it has any.
    fn text(&mut self) -> Option<String>;
}

/// A clipboard in the process: what there is with no display server.
/// Copy and paste work within the app.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryClipboard {
    text: Option<String>,
}

impl Clipboard for MemoryClipboard {
    fn set_text(&mut self, text: &str) {
        self.text = Some(text.to_owned());
    }

    fn text(&mut self) -> Option<String> {
        self.text.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_set_is_what_is_read() {
        let mut clipboard = MemoryClipboard::default();
        assert_eq!(clipboard.text(), None);
        clipboard.set_text("fl");
        clipboard.set_text("a😄");
        assert_eq!(clipboard.text().as_deref(), Some("a😄"));
        clipboard.set_text("");
        assert_eq!(clipboard.text().as_deref(), Some(""));
    }
}
