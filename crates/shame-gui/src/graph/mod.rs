//! The DAG engine: typed ports over a state struct, with incremental
//! dirty-triggered evaluation.
//!
//! A **state** is any `#[derive(DagStruct)]` struct — the built-in
//! [`BuiltinState`] (source + render fields) or a user state, and each element of a
//! [`HashMap`] collection is a state too. [`Port<D, S>`] reads/writes one
//! field `D` of state `S`; nodes receive a guarded
//! [`DagStructRef<S>`] so they can only touch fields through ports.
//!
//! [`Graph<S>`] runs nodes when their input ports are dirty. Dirty and fired
//! tracking live entirely on the graph — user code never sees them.
//! [`Graph::add_map_node`] fans one per-element node over a `HashMap<K, E>`,
//! reprocessing only the elements that changed.

pub mod element;
pub mod port;
pub mod state;

mod graph;

pub use element::DagStructRef;
pub use graph::Graph;
pub use port::{DagStruct, Port, PortGroup, PortId, PortValue};
pub use state::{
    AppState, BuiltinState, RENDER_PORT_BASE, RenderPorts, SOURCE_PORT_BASE, SourcePorts,
    write_source_fields,
};
