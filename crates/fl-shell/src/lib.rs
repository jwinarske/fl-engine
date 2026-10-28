//! The fl-engine embedder (brief M2 §2, §6): what a Flutter framework isolate
//! needs from the engine besides dart:ui's natives.
//!
//! [`isolate_config`] gives an isolate dart:ui's natives; [`prepare`] makes
//! the calls the engine makes between creating the root isolate and running
//! `main` (`RuntimeController`, runtime_controller.cc:594-600, and its
//! `FlushRuntimeStateToIsolate`). Without them `runApp` has no view to draw
//! into.

use fl_dart_vm::{DartError, HandleScope, IsolateConfig};

/// The one view, `implicitView` in the framework's terms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewConfig {
    /// Physical pixels.
    pub width: f64,
    /// Physical pixels.
    pub height: f64,
    pub device_pixel_ratio: f64,
    /// Hz, reported to the framework through `_updateDisplays`.
    pub refresh_rate: f64,
}

impl Default for ViewConfig {
    fn default() -> Self {
        ViewConfig {
            width: 800.0,
            height: 600.0,
            device_pixel_ratio: 1.0,
            refresh_rate: 60.0,
        }
    }
}

/// The implicit view's id, as the engine assigns it.
pub const IMPLICIT_VIEW_ID: i64 = 0;
/// The display the view is on.
pub const DISPLAY_ID: i64 = 0;

/// An isolate configuration with dart:ui's natives.
pub fn isolate_config(script_uri: &str) -> IsolateConfig {
    IsolateConfig {
        script_uri: script_uri.to_owned(),
        natives: Some(fl_ui::resolve),
        ..IsolateConfig::default()
    }
}

/// The engine's calls into `dart:ui` before `main`. Run inside the root
/// isolate's scope, before [`HandleScope::invoke_main`].
pub fn prepare(s: &mut HandleScope<'_>, view: &ViewConfig) -> Result<(), DartError> {
    let ui = s.lookup_library("dart:ui")?;

    // Without it `PlatformDispatcher.implicitView` is null and `runApp`
    // throws (dart_ui.cc:381; widgets/binding.dart:1628-1650).
    let id = s.new_int(IMPLICIT_VIEW_ID);
    s.set_field(ui, "_implicitViewId", id)?;

    add_view(s, view)?;

    // One locale: language, country, script, variant.
    let locales = s.new_string_list(&["en", "US", "", ""])?;
    s.invoke(ui, "_updateLocales", &[locales])?;
    let off = s.new_bool(false)?;
    s.invoke(ui, "_updateSemanticsEnabled", &[off])?;
    let none = s.new_int(0);
    s.invoke(ui, "_updateAccessibilityFeatures", &[none])?;
    // Valid JSON is required; empty means every setting at its default.
    let settings = s.new_string("{}")?;
    s.invoke(ui, "_updateUserSettingsData", &[settings])?;
    // Paused, hidden or detached would stop frames.
    let lifecycle = s.new_string("AppLifecycleState.resumed")?;
    s.invoke(ui, "_updateInitialLifecycleState", &[lifecycle])?;

    let ids = s.new_i64_list(&[DISPLAY_ID])?;
    let widths = s.new_f64_list(&[view.width])?;
    let heights = s.new_f64_list(&[view.height])?;
    let dprs = s.new_f64_list(&[view.device_pixel_ratio])?;
    let rates = s.new_f64_list(&[view.refresh_rate])?;
    s.invoke(ui, "_updateDisplays", &[ids, widths, heights, dprs, rates])?;
    Ok(())
}

/// `_addView`, all 29 arguments (hooks.dart:8-38; the engine's order is
/// platform_configuration.cc:107-140).
fn add_view(s: &mut HandleScope<'_>, view: &ViewConfig) -> Result<(), DartError> {
    let ui = s.lookup_library("dart:ui")?;
    let mut args = vec![s.new_int(IMPLICIT_VIEW_ID)];
    args.push(s.new_double(view.device_pixel_ratio)?);
    args.push(s.new_double(view.width)?);
    args.push(s.new_double(view.height)?);
    // View padding, view insets, system gesture insets: 4 each, all zero.
    for _ in 0..12 {
        args.push(s.new_double(0.0)?);
    }
    // physicalTouchSlop: unset.
    args.push(s.new_double(-1.0)?);
    // Display features: bounds, types, states. None.
    args.push(s.new_f64_list(&[])?);
    args.push(s.new_i64_list(&[])?);
    args.push(s.new_i64_list(&[])?);
    args.push(s.new_int(DISPLAY_ID));
    // Tight constraints. Zero would lay out a 0x0 root that paints nothing
    // (rendering/view.dart:40-47).
    args.push(s.new_double(view.width)?);
    args.push(s.new_double(view.width)?);
    args.push(s.new_double(view.height)?);
    args.push(s.new_double(view.height)?);
    // Display corner radii: none.
    for _ in 0..4 {
        args.push(s.new_double(0.0)?);
    }
    debug_assert_eq!(args.len(), 29);
    s.invoke(ui, "_addView", &args).map(drop)
}
