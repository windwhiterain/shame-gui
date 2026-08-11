/// Calculator demo: state struct → DAG computes outputs → GUI renders and edits.
///
/// - `#[derive(DagStruct, Widget)]` on `CalcState` generates `CalcStatePorts`
///   (typed field accessors) and `into_viewport_nodes()` for widget construction.
/// - The DAG node reads input ports, computes sum and dot, and writes output
///   ports — no rendering primitives.
/// - GUI widgets render the values; editing an input dirties its port →
///   DAG recomputes → output widgets update.
///
/// Run with: cargo run -p shame-gui --example calc
use shame_gui::app::App;
use shame_gui::gui::{Gui, ViewportNode, ViewportTree};
use shame_gui::math::Vec2;
use shame_gui::state;
use shame_gui::text::TextSystem;
use shame_gui::{DagStruct, Widget};

/// Calculator state: built-in fields (injected by `#[state]`) + inputs + outputs.
#[state]
#[derive(Clone, Default, DagStruct, Widget)]
pub struct CalcState {
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
    let mut app = App::<CalcState>::new("Calculator — GUI-driven DAG");
    {
        let s = app.state_mut();
        s.operand1 = 12.0;
        s.operand2 = 34.0;
        s.vec_a = Vec2::new(3.0, 4.0);
        s.vec_b = Vec2::new(5.0, 6.0);
    }

    // ── 1. Build the DAG: reads inputs, computes, writes outputs ──────
    let ports = CalcState::ports();
    app.graph_mut().add_node(
        {
            let p = ports;
            move |gref: &mut shame_gui::graph::DagStructRef<CalcState>,
                  _gpu: Option<&shame_gui::sm::Gpu>| {
                let a = *p.operand1.read(gref);
                let b = *p.operand2.read(gref);
                let va = *p.vec_a.read(gref);
                let vb = *p.vec_b.read(gref);
                p.sum.write(gref, a + b);
                p.dot.write(gref, va.x * vb.x + va.y * vb.y);
            }
        },
        (ports.operand1, ports.operand2, ports.vec_a, ports.vec_b),
        (ports.sum, ports.dot),
        None,
    );

    // ── 2. Build GUI widgets referencing the same fields ──────────────
    let children = CalcState::into_viewport_nodes();
    let tree = ViewportTree::new(ViewportNode::container(children));
    app.add_gui(Gui::new(tree));

    // ── 3. Run ────────────────────────────────────────────────────────
    app.run(TextSystem::new());
}
