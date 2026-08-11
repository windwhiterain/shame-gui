//! The built-in source ports the framework writes every frame.

use crate::graph::port::Port;

/// Framework-provided built-in ports. These are the DAG's source
/// nodes — written by the framework each frame, their dirty flags
/// trigger propagation to downstream plugin nodes.
///
/// Access via [`GraphBuilder::source`](crate::graph::GraphBuilder::source)
/// while building the graph, or via `graph.source` on a
/// [`Graph`](crate::graph::Graph).
pub struct SourcePorts {
    /// The window's framebuffer size in physical pixels.
    pub framebuffer_size: Port<crate::math::Vec2u>,
    /// The cursor position in physical pixels, y-down.
    pub mouse_pos: Port<crate::math::Vec2>,
    /// Whether any mouse button is currently pressed.
    pub mouse_down: Port<bool>,
    /// Accumulated scroll wheel delta since the last frame.
    pub scroll_delta: Port<f32>,
    /// Seconds since the last frame.
    pub delta_time: Port<f32>,
    /// Seconds since the app started.
    pub elapsed: Port<f32>,
}
