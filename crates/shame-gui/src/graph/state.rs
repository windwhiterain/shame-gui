//! The built-in global app state: a plain `#[derive(DagStruct)]` struct that
//! holds the framework's source ports (framebuffer, mouse, timing) and render
//! output ports (fills, outlines, texts).
//!
//! [`BuiltinState`] is a *state* in exactly the same sense as a map element:
//! both are `#[derive(DagStruct)]` structs accessed through
//! [`Port`](crate::graph::Port) handles behind a
//! [`DagStructRef`](crate::graph::DagStructRef). A custom app state embeds the
//! builtin fields automatically via the [`state`](crate::state) attribute
//! macro, which injects a reserved `__builtin` field and implements
//! [`AppState`].

use crate::graph::DagStruct;
use crate::graph::element::DagStructRef;
use crate::graph::port::Port;
use crate::math::{Vec2, Vec2u};
use crate::shader::RectEntry;
use crate::text::TextObject;

/// Base of the reserved `PortId` range used for built-in source ports of a
/// custom app state. User field ids start at 0; reserving a high range keeps
/// the two from colliding when a custom state embeds `BuiltinState`.
pub const SOURCE_PORT_BASE: u64 = 1 << 20;
/// Base of the reserved `PortId` range for built-in render ports.
pub const RENDER_PORT_BASE: u64 = (1 << 20) + 100;

/// The built-in global state. `App<BuiltinState>` uses this directly; a custom
/// app state gets it injected (as `__builtin: BuiltinState`) by the
/// [`state`](crate::state) attribute macro, which also implements [`AppState`].
#[derive(Clone, Default, shame_gui_derive::DagStruct)]
pub struct BuiltinState {
    /// The window's framebuffer size in physical pixels.
    pub framebuffer_size: Vec2u,
    /// The cursor position in physical pixels, y-down.
    pub mouse_pos: Vec2,
    /// Whether any mouse button is currently pressed.
    pub mouse_down: bool,
    /// Accumulated scroll wheel delta since the last frame.
    pub scroll_delta: f32,
    /// Seconds since the last frame.
    pub delta_time: f32,
    /// Seconds since the app started.
    pub elapsed: f32,
    /// Filled rectangles produced by the DAG (see [`RectEntry`]).
    pub fills: Vec<RectEntry>,
    /// Outlined rectangles (wireframes).
    pub outlines: Vec<RectEntry>,
    /// Text objects queued into the text system.
    pub texts: Vec<TextObject>,
}

/// The framework's built-in source ports, typed over a state `S`.
pub struct SourcePorts<S> {
    /// The window's framebuffer size in physical pixels.
    pub framebuffer_size: Port<Vec2u, S>,
    /// The cursor position in physical pixels, y-down.
    pub mouse_pos: Port<Vec2, S>,
    /// Whether any mouse button is currently pressed.
    pub mouse_down: Port<bool, S>,
    /// Accumulated scroll wheel delta since the last frame.
    pub scroll_delta: Port<f32, S>,
    /// Seconds since the last frame.
    pub delta_time: Port<f32, S>,
    /// Seconds since the app started.
    pub elapsed: Port<f32, S>,
}

/// The framework's built-in render output ports, typed over a state `S`.
pub struct RenderPorts<S> {
    /// Filled rectangles (see [`RectEntry`]).
    pub fills: Port<Vec<RectEntry>, S>,
    /// Outlined rectangles (wireframes).
    pub outlines: Port<Vec<RectEntry>, S>,
    /// Text objects rendered in the text pass.
    pub texts: Port<Vec<TextObject>, S>,
}

/// A state that exposes the framework's built-in source/render fields.
/// `App<S>` requires `S: AppState`; the default `App<BuiltinState>` uses
/// `BuiltinState` directly, while a custom state embeds it via the
/// [`state`](crate::state) attribute macro.
pub trait AppState: DagStruct {
    /// Immutable access to the built-in source/render fields.
    fn builtins(&self) -> &BuiltinState;
    /// Mutable access to the built-in source/render fields.
    fn builtins_mut(&mut self) -> &mut BuiltinState;
    /// The built-in source ports (written by the framework each frame).
    fn source_ports() -> SourcePorts<Self>;
    /// The built-in render output ports (read back after each tick).
    fn render_ports() -> RenderPorts<Self>;
}

impl AppState for BuiltinState {
    fn builtins(&self) -> &BuiltinState {
        self
    }
    fn builtins_mut(&mut self) -> &mut BuiltinState {
        self
    }
    fn source_ports() -> SourcePorts<BuiltinState> {
        let p = BuiltinState::ports();
        SourcePorts {
            framebuffer_size: p.framebuffer_size,
            mouse_pos: p.mouse_pos,
            mouse_down: p.mouse_down,
            scroll_delta: p.scroll_delta,
            delta_time: p.delta_time,
            elapsed: p.elapsed,
        }
    }
    fn render_ports() -> RenderPorts<BuiltinState> {
        let p = BuiltinState::ports();
        RenderPorts {
            fills: p.fills,
            outlines: p.outlines,
            texts: p.texts,
        }
    }
}

/// Writes the framework source fields through a state reference (marking the
/// source ports dirty so nodes reading them re-run). Used by `App::tick_dag`.
pub fn write_source_fields<S: AppState>(
    r: &mut DagStructRef<S>,
    fb: Vec2u,
    cursor: Vec2,
    mouse_down: bool,
    scroll: f32,
    dt: f32,
    elapsed: f32,
) {
    let p = S::source_ports();
    p.framebuffer_size.write(r, fb);
    p.mouse_pos.write(r, cursor);
    p.mouse_down.write(r, mouse_down);
    p.scroll_delta.write(r, scroll);
    p.delta_time.write(r, dt);
    p.elapsed.write(r, elapsed);
}
