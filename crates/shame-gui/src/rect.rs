//! [`Rect`]: a position + size rectangle with the crate's only pixel→NDC
//! conversion.

use crate::math::{Vec2, Vec2u};

/// A rectangle defined by its bottom-left corner and its size. The coordinate
/// space depends on the context: pixel-space (y-down) for instance data,
/// NDC (y-up) for instances produced by [`Rect::to_ndc`].
///
/// Derives [`GpuStruct`](crate::GpuStruct) (field mode), so it can be
/// embedded in instance structs, and [`DagStruct`](crate::DagStruct), so it
/// can flow through DAG ports.
#[derive(crate::GpuStruct, Clone, Copy, Debug, Default, PartialEq, shame_gui_derive::DagStruct)]
#[repr(C)]
pub struct Rect {
    /// The bottom-left corner (or top-left in pixel space, y-down).
    pub pos: Vec2,
    /// The width/height extent.
    pub size: Vec2,
}

impl Rect {
    /// Creates a rect from a corner and an extent.
    pub const fn new(pos: Vec2, size: Vec2) -> Self {
        Self { pos, size }
    }

    /// True when `pos` is inside the rect (half-open: right/bottom edges
    /// excluded), in the rect's own coordinate space (pixel y-down).
    pub fn contains(&self, pos: Vec2) -> bool {
        pos.x >= self.pos.x
            && pos.x < self.pos.x + self.size.x
            && pos.y >= self.pos.y
            && pos.y < self.pos.y + self.size.y
    }

    /// The single pixel→NDC conversion point in the crate.
    /// Returns a `Rect` in NDC (y-up): `pos` is the bottom-left corner
    /// (`y` is the *bottom* edge — tests encode this), `size` is the extent.
    pub fn to_ndc(&self, framebuffer: Vec2u) -> Rect {
        let width = framebuffer.x as f32;
        let height = framebuffer.y as f32;
        Rect::new(
            Vec2::new(
                self.pos.x / width * 2.0 - 1.0,
                1.0 - (self.pos.y + self.size.y) / height * 2.0,
            ),
            Vec2::new(self.size.x / width * 2.0, self.size.y / height * 2.0),
        )
    }
}
