/// Calculator demo: state struct → DAG computes outputs → GUI renders and edits.
///
/// - `#[derive(DagStruct, Widget)]` on `CalcState` generates `CalcStatePorts`
///   and `into_viewport_nodes(self, arena, ports)` for widget construction.
/// - Ports are allocated via `build_state_ports` so the same arena slots
///   are shared between the DAG node and the GUI widgets.
/// - The DAG node reads input ports, computes sum and dot, and writes output
///   ports — no rendering primitives.
/// - GUI widgets render the values; editing an input dirties its port →
///   DAG recomputes → output widgets update.
///
/// Run with: cargo run -p shame-gui --example calc
use shame_gui::app::App;
use shame_gui::graph::{StateArena, build_state_ports};
use shame_gui::gui::{Gui, ViewportNode, ViewportTree};
use shame_gui::math::Vec2;
use shame_gui::text::TextSystem;
use shame_gui::{DagStruct, Widget};

/// Calculator state: inputs + outputs.
#[derive(Clone, Default, DagStruct, Widget)]
struct CalcState {
    // ── Inputs (editable by the user) ──
    operand1: f32,
    operand2: f32,
    vec_a: Vec2,
    vec_b: Vec2,
    // ── Outputs (computed by the DAG, displayed) ──
    sum: f32,
    dot: f32,
}

fn main() {
    let state = CalcState {
        operand1: 12.0,
        operand2: 34.0,
        vec_a: Vec2::new(3.0, 4.0),
        vec_b: Vec2::new(5.0, 6.0),
        ..Default::default()
    };

    let mut app = App::new("Calculator — GUI-driven DAG");
    let text_system = TextSystem::new();

    // ── 1. Allocate arena ports and seed with initial values ──────────
    // The arena borrow is released after this statement, so subsequent
    // calls to `graph_builder()` / `finalize_graph()` / `arena_mut()` are
    // independent.
    let ports = build_state_ports(&state, app.arena_mut());

    // ── 2. Build the DAG: reads inputs, computes, writes outputs ──────
    // Wrap in a block so the `GraphBuilder` borrow is dropped before
    // `finalize_graph()` is called.
    {
        let mut b = app.graph_builder();
        b.add_node(
            {
                let p = ports.clone();
                move |arena: &mut StateArena, _gpu: Option<&shame_gui::sm::Gpu>| {
                    let a = *p.operand1.read(arena);
                    let b = *p.operand2.read(arena);
                    let va = Vec2::new(*p.vec_a.x.read(arena), *p.vec_a.y.read(arena));
                    let vb = Vec2::new(*p.vec_b.x.read(arena), *p.vec_b.y.read(arena));
                    p.sum.write(arena, a + b);
                    p.dot.write(arena, va.x * vb.x + va.y * vb.y);
                }
            },
            (ports.operand1, ports.operand2, ports.vec_a, ports.vec_b),
            (ports.sum, ports.dot),
        );
    }
    app.finalize_graph();

    // ── 3. Build GUI widgets referencing the same arena slots ─────────
    // `into_viewport_nodes(arena, Some(ports))` wires the existing port
    // IDs into each `WidgetNode` so dirty tracking (Enter key → widget
    // write → dirty port → DAG tick) works correctly.
    let children = state.into_viewport_nodes(app.arena_mut(), Some(ports));
    let tree = ViewportTree::new(ViewportNode::container(children));
    app.add_gui(Gui::new(tree));

    // ── 4. Run ────────────────────────────────────────────────────────
    app.run(text_system);
}
