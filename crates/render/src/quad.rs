//! The GPU data for one quad instance.
//!
//! Every drawable is a unit quad; `shape` + `param` tell the fragment shader how
//! to paint it:
//! - `shape = 0` — plain rectangle.
//! - `shape = 1` — rounded rectangle; `param` = corner radius as a fraction of
//!   the half-extent (`0.0..=1.0`).
//! - `shape = 2` — disc; `param` = gloss (`0` matte, `1` glossy highlight + rim).

use bytemuck::{Pod, Zeroable};

/// A corner of the shared unit quad. The only per-vertex data.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub corner: [f32; 2],
}

/// One drawable rectangle-or-disc, in pixel space.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Instance {
    pub center: [f32; 2],
    pub half_size: [f32; 2],
    pub color: [f32; 4],
    pub shape: f32,
    pub param: f32,
}

const SHAPE_RECT: f32 = 0.0;
const SHAPE_ROUNDED: f32 = 1.0;
const SHAPE_DISC: f32 = 2.0;

impl Instance {
    /// A filled rectangle.
    pub fn rect(center: [f32; 2], half_size: [f32; 2], color: [f32; 4]) -> Self {
        Self {
            center,
            half_size,
            color,
            shape: SHAPE_RECT,
            param: 0.0,
        }
    }

    /// A filled rectangle with rounded corners (`corner` = fraction of the
    /// half-extent, `0.0..=1.0`).
    pub fn rounded(center: [f32; 2], half_size: [f32; 2], color: [f32; 4], corner: f32) -> Self {
        Self {
            center,
            half_size,
            color,
            shape: SHAPE_ROUNDED,
            param: corner,
        }
    }

    /// A matte disc of the given radius (used for hints and shadows).
    pub fn disc(center: [f32; 2], radius: f32, color: [f32; 4]) -> Self {
        Self {
            center,
            half_size: [radius, radius],
            color,
            shape: SHAPE_DISC,
            param: 0.0,
        }
    }

    /// A glossy game piece (specular highlight + rim shadow).
    pub fn piece(center: [f32; 2], radius: f32, color: [f32; 4]) -> Self {
        Self::piece_xy(center, [radius, radius], color)
    }

    /// A glossy piece with independent half-extents, for the edge-on flip squash.
    pub fn piece_xy(center: [f32; 2], half_size: [f32; 2], color: [f32; 4]) -> Self {
        Self {
            center,
            half_size,
            color,
            shape: SHAPE_DISC,
            param: 1.0,
        }
    }
}

/// The four corners of the unit quad (two triangles via [`QUAD_INDICES`]).
pub const QUAD_VERTICES: [Vertex; 4] = [
    Vertex { corner: [0.0, 0.0] },
    Vertex { corner: [1.0, 0.0] },
    Vertex { corner: [0.0, 1.0] },
    Vertex { corner: [1.0, 1.0] },
];

/// Index buffer for the unit quad.
pub const QUAD_INDICES: [u16; 6] = [0, 1, 2, 2, 1, 3];
