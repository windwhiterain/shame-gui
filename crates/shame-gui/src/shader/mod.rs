//! Built-in materials: filled rectangles and wireframe outlines.
//!
//! For custom shading, implement [`Material`](crate::material::Material) —
//! the [`rect`] module is the reference implementation.

pub mod rect;
pub mod wireframe;

pub use rect::{RectEntry, RectInstance, RectInstanceGpu, RectMaterial};
pub use wireframe::WireframeMaterial;

/// Viewport uniform passed as push constant to GUI materials that do
/// pixel→NDC conversion in the vertex shader.
#[derive(crate::GpuStruct, Clone, Copy, Default)]
#[repr(C)]
pub struct ViewportParams {
    /// Framebuffer size in physical pixels.
    pub fb_size: crate::Vec2u,
}
