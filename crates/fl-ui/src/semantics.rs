//! `SemanticsFlags`. Semantics are out of M2 (brief §1): the flags object
//! gets a peer so later natives taking it see a live object, and nothing
//! is kept.

use fl_dart_ffi::{guard, peer, sys};

/// The `SemanticsFlags` peer.
struct SemanticsFlags;

/// `Void Function(Handle, Int x7, Bool x17)`: the flags object, then its
/// tristates and booleans in declaration order (semantics.dart:1280-1330).
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn init_semantics_flags(
    wrapper: sys::Dart_Handle,
    _is_checked: i32,
    _is_selected: i32,
    _is_enabled: i32,
    _is_toggled: i32,
    _is_expanded: i32,
    _is_required: i32,
    _is_focused: i32,
    _is_button: bool,
    _is_text_field: bool,
    _is_in_mutually_exclusive_group: bool,
    _is_header: bool,
    _is_obscured: bool,
    _scopes_route: bool,
    _names_route: bool,
    _is_hidden: bool,
    _is_image: bool,
    _is_live_region: bool,
    _has_implicit_scrolling: bool,
    _is_multiline: bool,
    _is_read_only: bool,
    _is_link: bool,
    _is_slider: bool,
    _is_keyboard_key: bool,
    _is_accessibility_focus_blocked: bool,
) {
    guard((), || {
        // SAFETY: a Handle-parameter native: isolate current, scope open.
        if let Err(e) = unsafe { peer::attach(wrapper, SemanticsFlags, 0) } {
            eprintln!("fl-ui: NativeSemanticsFlags::initSemanticsFlags: {e}");
        }
    });
}
