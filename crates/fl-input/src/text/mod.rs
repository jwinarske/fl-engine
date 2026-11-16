//! Text input (brief M4 §2, D4, D6): the text an input method edits, what
//! becomes of a key the framework did not handle, and the clipboard. The
//! Linux embedder's, ported, with the platform's parts behind traits:
//! [`InputMethod`] for GTK's `GtkIMContext`, [`Clipboard`] for its
//! `GtkClipboard`.

mod clipboard;
mod handler;
mod model;

pub use clipboard::{Clipboard, MemoryClipboard};
pub use handler::{
    typed_character, Direct, EditingDelta, EditingState, ImEvent, InputMethod, InputType,
    TextInput, Update, CLIENT_UNSET,
};
pub use model::{TextInputModel, TextRange};
