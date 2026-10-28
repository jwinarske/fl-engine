//! dart:ui's natives (brief M2 §3): the `@Native` functions the Flutter
//! framework calls, resolved through [`resolve`].
//!
//! Stage 1 of the brief's §12: every native on a framework app's path to its
//! first frame is present. The platform ones that need nothing are stubs,
//! listed in [`STUBS`] with what each returns. The drawing ones keep the
//! structure of what they are given (a picture's draws, a scene's layers) so
//! the path can be checked end to end; turning that into pixels is stage 3.
//!
//! `PlatformConfigurationNativeApi::Render` hands each frame to the sink set
//! with [`set_frame_sink`], on the UI thread.

use std::ffi::c_void;

use fl_dart_ffi::{lookup, Symbol};

mod compositing;
mod painting;
mod platform;
mod semantics;

pub use compositing::{LayerOp, Scene};
pub use painting::{Draw, Paint, Picture, Style};
pub use platform::{set_frame_sink, Frame};

/// Every dart:ui native this crate implements.
pub static NATIVES: &[Symbol] = &[
    // PlatformConfigurationNativeApi (platform_dispatcher.dart, window.dart)
    fl_dart_ffi::symbol!(
        "PlatformConfigurationNativeApi::GetRootIsolateToken",
        0,
        platform::get_root_isolate_token
    ),
    fl_dart_ffi::symbol!(
        "PlatformConfigurationNativeApi::SendChannelUpdate",
        2,
        platform::send_channel_update
    ),
    fl_dart_ffi::symbol!(
        "PlatformConfigurationNativeApi::SendPlatformMessage",
        3,
        platform::send_platform_message
    ),
    fl_dart_ffi::symbol!(
        "PlatformConfigurationNativeApi::SetSemanticsTreeEnabled",
        1,
        platform::set_semantics_tree_enabled
    ),
    fl_dart_ffi::symbol!(
        "PlatformConfigurationNativeApi::ScheduleFrame",
        0,
        platform::schedule_frame
    ),
    fl_dart_ffi::symbol!(
        "PlatformConfigurationNativeApi::SetNeedsReportTimings",
        1,
        platform::set_needs_report_timings
    ),
    fl_dart_ffi::symbol!(
        "PlatformConfigurationNativeApi::EndWarmUpFrame",
        0,
        platform::end_warm_up_frame
    ),
    fl_dart_ffi::symbol!(
        "PlatformConfigurationNativeApi::Render",
        4,
        platform::render
    ),
    // painting.dart
    fl_dart_ffi::symbol!("PictureRecorder::Create", 1, painting::recorder_create),
    fl_dart_ffi::symbol!("PictureRecorder::endRecording", 2, painting::recorder_end),
    fl_dart_ffi::symbol!("Canvas::Create", 6, painting::canvas_create),
    fl_dart_ffi::symbol!("Canvas::drawRect", 7, painting::canvas_draw_rect),
    fl_dart_ffi::symbol!("Picture::dispose", 1, painting::picture_dispose),
    // compositing.dart
    fl_dart_ffi::symbol!("SceneBuilder::Create", 1, compositing::builder_create),
    fl_dart_ffi::symbol!(
        "SceneBuilder::pushTransformHandle",
        4,
        compositing::push_transform
    ),
    fl_dart_ffi::symbol!("SceneBuilder::pushOffset", 5, compositing::push_offset),
    fl_dart_ffi::symbol!("SceneBuilder::addPicture", 5, compositing::add_picture),
    fl_dart_ffi::symbol!("SceneBuilder::pop", 1, compositing::pop),
    fl_dart_ffi::symbol!("SceneBuilder::build", 2, compositing::build),
    fl_dart_ffi::symbol!("Scene::dispose", 1, compositing::scene_dispose),
    fl_dart_ffi::symbol!("EngineLayer::dispose", 1, compositing::layer_dispose),
    // semantics.dart
    fl_dart_ffi::symbol!(
        "NativeSemanticsFlags::initSemanticsFlags",
        25,
        semantics::init_semantics_flags
    ),
];

/// The natives that do nothing yet, and what each returns. The brief's exit
/// criterion 5: every stub is listed with its result.
pub static STUBS: &[(&str, &str)] = &[
    (
        "PlatformConfigurationNativeApi::GetRootIsolateToken",
        "a non-zero token, unique per call (0 selects the background-isolate path)",
    ),
    (
        "PlatformConfigurationNativeApi::SendChannelUpdate",
        "nothing",
    ),
    (
        "PlatformConfigurationNativeApi::SendPlatformMessage",
        "null (success); the message is dropped and never answered",
    ),
    (
        "PlatformConfigurationNativeApi::SetSemanticsTreeEnabled",
        "nothing",
    ),
    (
        "PlatformConfigurationNativeApi::ScheduleFrame",
        "nothing: no frame after the warm-up frame yet (stage 4)",
    ),
    (
        "PlatformConfigurationNativeApi::SetNeedsReportTimings",
        "nothing",
    ),
    ("PlatformConfigurationNativeApi::EndWarmUpFrame", "nothing"),
    (
        "NativeSemanticsFlags::initSemanticsFlags",
        "an empty peer; the flags are not kept (semantics are out of M2)",
    ),
];

/// A resolver for dart:ui, for fl-dart-vm's `IsolateConfig::natives`.
pub fn resolve(name: &str, args: usize) -> Option<*mut c_void> {
    lookup(NATIVES, name, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_symbol_is_listed_once() {
        let mut names: Vec<_> = NATIVES.iter().map(|s| s.name).collect();
        names.sort_unstable();
        let n = names.len();
        names.dedup();
        assert_eq!(names.len(), n, "a symbol is listed twice");
    }

    #[test]
    fn every_stub_is_a_native() {
        for (name, _) in STUBS {
            assert!(NATIVES.iter().any(|s| s.name == *name), "{name}");
        }
    }
}
