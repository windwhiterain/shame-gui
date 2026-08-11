use crate::graph::StateArena;
use crate::graph::port::{DagStruct, PortGroup};

/// Builds a port group for the given state struct:
/// allocates arena slots for every leaf port and writes initial values.
///
/// State structs come from library crates that provide `#[derive(DagStruct)]`
/// types and subgraph-builder functions. The app composes them into one
/// master struct and wires the DAG manually:
///
/// ```ignore
/// // Library crate `calc_lib` provides CalcState + fn calc_graph(ports: &CalcStatePorts, b: &mut GraphBuilder)
/// // Library crate `log_lib`  provides LogState  + fn log_graph(ports: &LogStatePorts, calc: &CalcStatePorts, b: &mut GraphBuilder)
///
/// #[derive(DagStruct, Widget)]
/// struct AppState { calc: CalcState, log: LogState }
///
/// let state = AppState { calc: CalcState::default(), log: LogState::default() };
/// let ports = build_state_ports(&state, app.arena_mut());
/// let mut b = app.graph_builder();
/// calc_graph(&ports.calc, &mut b);
/// log_graph(&ports.log, &ports.calc, &mut b);
/// app.finalize_graph();
/// ```
pub fn build_state_ports<S: DagStruct>(state: &S, arena: &mut StateArena) -> S::Ports {
    let ports = S::Ports::alloc_slots(arena);
    S::write_ports(state, arena, &ports);
    ports
}
