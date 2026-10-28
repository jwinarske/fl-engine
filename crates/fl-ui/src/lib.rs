//! dart:ui's natives (brief M2 §3): the `@Native` functions the Flutter
//! framework calls, resolved through [`resolve`].
//!
//! The platform natives that need nothing are stubs, listed in [`STUBS`]
//! with what each returns. The drawing ones keep what they are given, a
//! picture's draws and a scene's layers, as plain data; fl-raster turns
//! that into pixels.
//!
//! `PlatformConfigurationNativeApi::Render` hands each frame to the sink set
//! with [`set_frame_sink`], on the UI thread.

use std::ffi::c_void;

use fl_dart_ffi::{lookup, Symbol};

mod compositing;
mod painting;
mod path;
mod platform;
mod semantics;

pub use compositing::{Clip, LayerOp, Scene};
pub use painting::{Cap, ClipOp, Draw, Join, Paint, Picture, Style};
pub use path::{Affine, FillType, Path, Point, RRect, Seg};
pub use platform::{
    deliver_replies, discard_replies, has_pending_replies, set_frame_scheduler, set_frame_sink,
    Frame,
};

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
        "PlatformConfigurationNativeApi::DefaultRouteName",
        0,
        platform::default_route_name
    ),
    fl_dart_ffi::symbol!(
        "PlatformConfigurationNativeApi::SetApplicationLocale",
        1,
        platform::set_application_locale
    ),
    fl_dart_ffi::symbol!(
        "PlatformConfigurationNativeApi::RequestViewFocusChange",
        3,
        platform::request_view_focus_change
    ),
    fl_dart_ffi::symbol!(
        "PlatformConfigurationNativeApi::Render",
        4,
        platform::render
    ),
    // painting.dart
    fl_dart_ffi::symbol!("PictureRecorder::Create", 1, painting::recorder_create),
    fl_dart_ffi::symbol!("PictureRecorder::endRecording", 2, painting::recorder_end),
    fl_dart_ffi::symbol!("Picture::dispose", 1, painting::picture_dispose),
    fl_dart_ffi::symbol!("Canvas::Create", 6, painting::canvas_create),
    fl_dart_ffi::symbol!("Canvas::save", 1, painting::save),
    fl_dart_ffi::symbol!(
        "Canvas::saveLayerWithoutBounds",
        3,
        painting::save_layer_without_bounds
    ),
    fl_dart_ffi::symbol!("Canvas::saveLayer", 7, painting::save_layer),
    fl_dart_ffi::symbol!("Canvas::restore", 1, painting::restore),
    fl_dart_ffi::symbol!("Canvas::restoreToCount", 2, painting::restore_to_count),
    fl_dart_ffi::symbol!("Canvas::getSaveCount", 1, painting::get_save_count),
    fl_dart_ffi::symbol!("Canvas::translate", 3, painting::translate),
    fl_dart_ffi::symbol!("Canvas::scale", 3, painting::scale),
    fl_dart_ffi::symbol!("Canvas::rotate", 2, painting::rotate),
    fl_dart_ffi::symbol!("Canvas::skew", 3, painting::skew),
    fl_dart_ffi::symbol!("Canvas::transform", 2, painting::transform),
    fl_dart_ffi::symbol!("Canvas::clipRect", 7, painting::clip_rect),
    fl_dart_ffi::symbol!("Canvas::clipRRect", 3, painting::clip_rrect),
    fl_dart_ffi::symbol!("Canvas::clipPath", 3, painting::clip_path),
    fl_dart_ffi::symbol!("Canvas::drawColor", 3, painting::draw_color),
    fl_dart_ffi::symbol!("Canvas::drawLine", 7, painting::draw_line),
    fl_dart_ffi::symbol!("Canvas::drawPaint", 3, painting::draw_paint),
    fl_dart_ffi::symbol!("Canvas::drawRect", 7, painting::draw_rect),
    fl_dart_ffi::symbol!("Canvas::drawRRect", 4, painting::draw_rrect),
    fl_dart_ffi::symbol!("Canvas::drawDRRect", 5, painting::draw_drrect),
    fl_dart_ffi::symbol!("Canvas::drawOval", 7, painting::draw_oval),
    fl_dart_ffi::symbol!("Canvas::drawCircle", 6, painting::draw_circle),
    fl_dart_ffi::symbol!("Canvas::drawArc", 10, painting::draw_arc),
    fl_dart_ffi::symbol!("Canvas::drawPath", 4, painting::draw_path),
    fl_dart_ffi::symbol!("Canvas::drawPicture", 2, painting::draw_picture),
    fl_dart_ffi::symbol!("Path::Create", 1, path::create),
    fl_dart_ffi::symbol!("Path::clone", 2, path::clone),
    fl_dart_ffi::symbol!("Path::getFillType", 1, path::get_fill_type),
    fl_dart_ffi::symbol!("Path::setFillType", 2, path::set_fill_type),
    fl_dart_ffi::symbol!("Path::moveTo", 3, path::move_to),
    fl_dart_ffi::symbol!("Path::relativeMoveTo", 3, path::relative_move_to),
    fl_dart_ffi::symbol!("Path::lineTo", 3, path::line_to),
    fl_dart_ffi::symbol!("Path::relativeLineTo", 3, path::relative_line_to),
    fl_dart_ffi::symbol!("Path::quadraticBezierTo", 5, path::quad_to),
    fl_dart_ffi::symbol!("Path::relativeQuadraticBezierTo", 5, path::relative_quad_to),
    fl_dart_ffi::symbol!("Path::cubicTo", 7, path::cubic_to),
    fl_dart_ffi::symbol!("Path::relativeCubicTo", 7, path::relative_cubic_to),
    fl_dart_ffi::symbol!("Path::conicTo", 6, path::conic_to),
    fl_dart_ffi::symbol!("Path::relativeConicTo", 6, path::relative_conic_to),
    fl_dart_ffi::symbol!("Path::arcTo", 8, path::arc_to),
    fl_dart_ffi::symbol!("Path::arcToPoint", 8, path::arc_to_point),
    fl_dart_ffi::symbol!("Path::relativeArcToPoint", 8, path::relative_arc_to_point),
    fl_dart_ffi::symbol!("Path::addRect", 5, path::add_rect),
    fl_dart_ffi::symbol!("Path::addOval", 5, path::add_oval),
    fl_dart_ffi::symbol!("Path::addArc", 7, path::add_arc),
    fl_dart_ffi::symbol!("Path::addPolygon", 3, path::add_polygon),
    fl_dart_ffi::symbol!("Path::addRRect", 2, path::add_rrect),
    fl_dart_ffi::symbol!("Path::addPath", 4, path::add_path),
    fl_dart_ffi::symbol!("Path::addPathWithMatrix", 5, path::add_path_with_matrix),
    fl_dart_ffi::symbol!("Path::close", 1, path::close),
    fl_dart_ffi::symbol!("Path::reset", 1, path::reset),
    fl_dart_ffi::symbol!("Path::shift", 4, path::shift),
    fl_dart_ffi::symbol!("Path::transform", 3, path::transform),
    fl_dart_ffi::symbol!("Path::getBounds", 1, path::get_bounds),
    // compositing.dart
    fl_dart_ffi::symbol!("SceneBuilder::Create", 1, compositing::builder_create),
    fl_dart_ffi::symbol!(
        "SceneBuilder::pushTransformHandle",
        4,
        compositing::push_transform
    ),
    fl_dart_ffi::symbol!("SceneBuilder::pushOffset", 5, compositing::push_offset),
    fl_dart_ffi::symbol!("SceneBuilder::pushClipRect", 8, compositing::push_clip_rect),
    fl_dart_ffi::symbol!(
        "SceneBuilder::pushClipRRect",
        5,
        compositing::push_clip_rrect
    ),
    fl_dart_ffi::symbol!("SceneBuilder::pushClipPath", 5, compositing::push_clip_path),
    fl_dart_ffi::symbol!("SceneBuilder::pushOpacity", 6, compositing::push_opacity),
    fl_dart_ffi::symbol!("SceneBuilder::addPicture", 5, compositing::add_picture),
    fl_dart_ffi::symbol!("SceneBuilder::pop", 1, compositing::pop),
    fl_dart_ffi::symbol!("SceneBuilder::addRetained", 2, compositing::add_retained),
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
        "null (success); flutter/restoration `get` is answered with a null result, every other message dropped and never answered"
    ),
    (
        "PlatformConfigurationNativeApi::SetSemanticsTreeEnabled",
        "nothing",
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
    (
        "PlatformConfigurationNativeApi::SetApplicationLocale",
        "nothing: no platform here wants the resolved locale",
    ),
    (
        "PlatformConfigurationNativeApi::RequestViewFocusChange",
        "nothing: one view, no platform focus to move",
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
