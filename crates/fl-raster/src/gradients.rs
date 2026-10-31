//! dart:ui's gradients as emblema's: linear, radial, two-point conical and
//! sweep, each with its stops, its tile mode, its matrix as the shader's
//! local transform, and the paint's alpha as its opacity.

use emblema::{Color, GradientStop, Paint as EPaint, Transform2D, Vec2};
use fl_ui::{Gradient, GradientKind};

use crate::{tile_of, RasterError};

/// A dart:ui `matrix4`, column-major, as emblema's shader transform: the
/// 2D affine part and the perspective row, as upstream's `To3x3` keeps.
pub(crate) fn shader_transform_of(m: &[f64; 16]) -> Transform2D {
    Transform2D::from_column_major_4x4(&m.map(|v| v as f32))
}

/// `g` as emblema's paint, at the paint's `opacity`, its stops as upstream
/// gives a gradient its stops.
pub(crate) fn gradient_paint(g: &Gradient, opacity: f32) -> Result<EPaint, RasterError> {
    let stops: Vec<GradientStop> = convert_stops(&g.colors, &g.stops)
        .into_iter()
        .map(|(c, o)| GradientStop::new(Color::srgb(c[0], c[1], c[2], c[3]), o))
        .collect();
    let v = |[x, y]: [f32; 2]| Vec2::new(x, y);
    let paint = match g.kind {
        GradientKind::Linear { start, end } => EPaint::linear_gradient(v(start), v(end), stops),
        GradientKind::Radial { center, radius } => {
            EPaint::radial_gradient(v(center), radius, stops)
        }
        GradientKind::Conical {
            start,
            start_radius,
            end,
            end_radius,
        } => EPaint::conical_gradient(v(start), start_radius, v(end), end_radius, stops),
        GradientKind::Sweep {
            center,
            start_angle,
            end_angle,
        } => EPaint::sweep_gradient(v(center), start_angle, end_angle, stops),
    };
    let paint = paint.with_tile_mode(tile_of(g.tile)?).with_opacity(opacity);
    Ok(match &g.matrix {
        Some(m) => paint.with_shader_transform(shader_transform_of(m)),
        None => paint,
    })
}

/// Upstream's `Paint::ConvertStops` (display_list/paint.cc): the first
/// color added at 0 if the stops start past it, every stop clamped to 0 to
/// 1, the last color added at 1 if they end before it, and the stops made
/// non-decreasing. As upstream's, that last pass runs over the stops as
/// given in number, from the second, on the list after the one at 0 was
/// added.
fn convert_stops(colors: &[[f32; 4]], given: &[f32]) -> Vec<([f32; 4], f32)> {
    let (Some(first), Some(last)) = (given.first(), given.last()) else {
        return Vec::new();
    };
    let mut out: Vec<([f32; 4], f32)> = Vec::with_capacity(given.len() + 2);
    if *first != 0.0 {
        out.push((colors[0], 0.0));
    }
    for (c, s) in colors.iter().zip(given) {
        out.push((*c, s.clamp(0.0, 1.0)));
    }
    if *last != 1.0 {
        let c = out[out.len() - 1].0;
        out.push((c, 1.0));
    }
    for i in 1..given.len() {
        let floor = out[i - 1].1;
        out[i].1 = out[i].1.max(floor);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const G: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
    const B: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    fn offsets(v: &[([f32; 4], f32)]) -> Vec<f32> {
        v.iter().map(|(_, s)| *s).collect()
    }

    #[test]
    fn stops_from_zero_to_one_are_kept() {
        let v = convert_stops(&[R, G, B], &[0.0, 0.4, 1.0]);
        assert_eq!(offsets(&v), [0.0, 0.4, 1.0]);
        assert_eq!(v[1].0, G);
    }

    #[test]
    fn stops_inside_the_ramp_get_its_ends_from_their_own_colors() {
        let v = convert_stops(&[R, B], &[0.25, 0.75]);
        assert_eq!(offsets(&v), [0.0, 0.25, 0.75, 1.0]);
        assert_eq!([v[0].0, v[3].0], [R, B]);
    }

    #[test]
    fn stops_out_of_range_or_order_are_clamped_as_upstream_clamps_them() {
        // Clamped to 0 to 1, then each held to the one before, from the
        // second, over as many as were given: with one added at 0 the pass
        // stops one short, and the 0.3 after 0.8 stays, as upstream's does.
        let v = convert_stops(&[R, G, B], &[-0.5, 0.8, 0.3]);
        assert_eq!(offsets(&v), [0.0, 0.0, 0.8, 0.3, 1.0]);
        let v = convert_stops(&[R, G, B], &[0.0, 1.5, 0.2]);
        assert_eq!(offsets(&v), [0.0, 1.0, 1.0, 1.0]);
    }
}
