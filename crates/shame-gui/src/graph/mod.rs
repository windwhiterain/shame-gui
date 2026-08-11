//! The DAG engine: typed ports over a value arena, with incremental
//! dirty-triggered evaluation.
//!
//! The core idea is that every piece of state — widget values, computed
//! outputs, render targets — lives in a [`StateArena`] slot, addressed by
//! [`PortId`]. [`Port`] handles (possibly several for the same slot, which
//! is what "connecting" means) read and write through the arena.
//!
//! [`Graph`] organizes [`Port`]s into a DAG: each [`GraphBuilder::add_node`]
//! registers an `eval` closure whose inputs are declared by [`PortId`]. When
//! an input port is marked dirty (a widget edit, or a source port write),
//! `Graph::tick` re-runs the affected nodes and propagates dirtiness to
//! their outputs.
//!
//! ## The three port families
//!
//! - [`SourcePorts`] — built-in framework ports written every frame
//!   (framebuffer size, mouse position, timing).
//! - [`RenderPorts`] — built-in output ports the framework reads after a
//!   tick (fills, outlines, texts).
//! - User ports — allocated via [`StateArena::alloc`] /
//!   [`build_state_ports`], wired to nodes and widgets.
//!
//! ## Widget bridge
//!
//! A [`WidgetNode`](crate::gui::WidgetNode) carries the `PortId`s of the
//! value it edits. When a widget changes a value, `Gui::on_event` returns
//! the dirty ids and the app marks them on the graph before the next tick.
//!
//! [`PortValue`] defines which types can flow through ports; [`DagStruct`]
//! maps a value type to its port group (`Port<T>` for primitives,
//! `{Name}Ports` for `#[derive(DagStruct)]` structs); [`PortGroup`] is the
//! common interface over single and composite port groups.

pub mod arena;
pub mod port;
pub mod source;

mod graph;

pub use arena::StateArena;
pub use graph::{Graph, GraphBuilder, IdGroup, RenderPorts};
pub use port::{DagStruct, Port, PortGroup, PortId, PortValue, build_state_ports, port_ids};
pub use source::SourcePorts;
