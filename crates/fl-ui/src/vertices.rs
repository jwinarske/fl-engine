//! `Vertices`: a mesh for `Canvas.drawVertices`, as the engine's `Vertices`
//! receives it (vertices.cc). Immutable once made; shared by every draw
//! that uses it.

use std::ffi::c_void;
use std::sync::Arc;

use fl_dart_ffi::{args, guard, peer, sys};

/// A mesh: triangles, a strip or a fan of positions, each optionally with a
/// texture coordinate and a color, in an optional index order.
#[derive(Debug, Clone, PartialEq)]
pub struct Vertices {
    /// `VertexMode` index: 0 triangles, 1 triangle strip, 2 triangle fan.
    pub mode: i32,
    pub positions: Vec<[f32; 2]>,
    /// One a position, or none.
    pub texture_coordinates: Vec<[f32; 2]>,
    /// ARGB, one a position, or none.
    pub colors: Vec<u32>,
    /// Into the positions, or none for their own order.
    pub indices: Vec<u16>,
}

type Peer = Option<Arc<Vertices>>;

/// Pairs of floats as points; `None` for an odd count or a value that is
/// not finite.
fn points(raw: &[f32]) -> Option<Vec<[f32; 2]>> {
    if !raw.len().is_multiple_of(2) || raw.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(raw.chunks_exact(2).map(|p| [p[0], p[1]]).collect())
}

impl Vertices {
    /// The mesh, or `None` where the parts do not agree: a mode past the
    /// three, a coordinate or color count other than the positions', an
    /// index past them.
    fn new(
        mode: i32,
        positions: &[f32],
        texture_coordinates: Option<&[f32]>,
        colors: Option<&[i32]>,
        indices: Option<&[u16]>,
    ) -> Option<Vertices> {
        if !(0..=2).contains(&mode) {
            return None;
        }
        let positions = points(positions)?;
        let texture_coordinates = match texture_coordinates {
            Some(t) => points(t).filter(|t| t.len() == positions.len())?,
            None => Vec::new(),
        };
        let colors: Vec<u32> = match colors {
            Some(c) if c.len() == positions.len() => c.iter().map(|&c| c as u32).collect(),
            Some(_) => return None,
            None => Vec::new(),
        };
        let indices = indices.map(<[u16]>::to_vec).unwrap_or_default();
        if indices.iter().any(|&i| usize::from(i) >= positions.len()) {
            return None;
        }
        Some(Vertices {
            mode,
            positions,
            texture_coordinates,
            colors,
            indices,
        })
    }
}

/// `Bool Function(Handle outVertices, Int32 mode, Handle positions, Handle
/// textureCoordinates, Handle colors, Handle indices)`: false for parts
/// that do not agree, which dart:ui throws on.
pub(crate) extern "C" fn init(
    out: sys::Dart_Handle,
    mode: i32,
    positions: sys::Dart_Handle,
    texture_coordinates: sys::Dart_Handle,
    colors: sys::Dart_Handle,
    indices: sys::Dart_Handle,
) -> bool {
    guard(false, || {
        // SAFETY: a Handle-parameter native: isolate current, scope open.
        let mesh = unsafe {
            let positions = args::f32s(positions);
            let texture_coordinates = args::f32s(texture_coordinates);
            let colors = args::i32s(colors);
            let indices = args::u16s(indices);
            positions.and_then(|p| {
                Vertices::new(
                    mode,
                    &p,
                    texture_coordinates.as_deref(),
                    colors.as_deref(),
                    indices.as_deref(),
                )
            })
        };
        let Some(mesh) = mesh else {
            return false;
        };
        // SAFETY: as above.
        match unsafe { peer::attach(out, Some(Arc::new(mesh)) as Peer, 0) } {
            Ok(()) => true,
            Err(e) => {
                eprintln!("fl-ui: Vertices::init: {e}");
                false
            }
        }
    })
}

/// `Void Function(Pointer<Void>)`: lets the mesh go; draws already recorded
/// keep their own reference.
pub(crate) extern "C" fn dispose(v: *mut c_void) {
    guard((), || {
        // SAFETY: v is the receiver's native field.
        let _ = unsafe { peer::with::<Peer, _>(v, |m| *m = None) };
    });
}

/// The mesh a `Vertices` wrapper holds, by its native field.
///
/// # Safety
/// `v` is a native field fl-ui attached, or null.
pub(crate) unsafe fn of_field(v: *mut c_void) -> Option<Arc<Vertices>> {
    // SAFETY: per this function's contract.
    unsafe { peer::with::<Peer, _>(v, |m| m.clone()) }.flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mesh_whose_parts_agree_is_made() {
        let m = Vertices::new(
            0,
            &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            None,
            Some(&[-1, -1, -1]),
            Some(&[0, 1, 2]),
        );
        assert_eq!(m.map(|m| m.positions.len()), Some(3));
    }

    #[test]
    fn parts_that_do_not_agree_make_no_mesh() {
        let tri = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        assert_eq!(Vertices::new(3, &tri, None, None, None), None, "mode");
        assert_eq!(Vertices::new(0, &tri[..5], None, None, None), None, "odd");
        assert_eq!(
            Vertices::new(0, &tri, Some(&[0.0; 4]), None, None),
            None,
            "uv"
        );
        assert_eq!(
            Vertices::new(0, &tri, None, Some(&[0; 2]), None),
            None,
            "colors"
        );
        assert_eq!(
            Vertices::new(0, &tri, None, None, Some(&[3])),
            None,
            "index"
        );
        assert_eq!(
            Vertices::new(0, &[f32::NAN; 6], None, None, None),
            None,
            "finite"
        );
    }
}
