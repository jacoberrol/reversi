//! The GPU data for one quad instance.

use bytemuck::{Pod, Zeroable};

/// A corner of the shared unit quad. The only per-vertex data; everything else
/// (position, size, color) is per-instance.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub corner: [f32; 2],
}

/// One drawable rectangle-or-disc, in pixel space.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Instance {
    /// Center of the rectangle, in pixels (origin top-left).
    pub center: [f32; 2],
    /// Half width/height, in pixels.
    pub half_size: [f32; 2],
    /// RGBA, straight (non-premultiplied) alpha.
    pub color: [f32; 4],
    /// 1.0 to render as a disc, 0.0 for a full rectangle.
    pub circle: f32,
}

impl Instance {
    /// A filled rectangle centered at `center`.
    pub fn rect(center: [f32; 2], half_size: [f32; 2], color: [f32; 4]) -> Self {
        Self {
            center,
            half_size,
            color,
            circle: 0.0,
        }
    }

    /// A filled disc of the given radius centered at `center`.
    pub fn disc(center: [f32; 2], radius: f32, color: [f32; 4]) -> Self {
        Self {
            center,
            half_size: [radius, radius],
            color,
            circle: 1.0,
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
