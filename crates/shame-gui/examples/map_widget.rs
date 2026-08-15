//! The `HashMap<String, T>` map widget: a collapsible key-value list with
//! add/remove, each entry expanding into its element form.
//!
//! - `Member` derives `Widget` — that is what makes it usable as the map
//!   widget's element type (the derive also implements `WidgetElement`,
//!   which supplies the per-field child-widget template).
//! - A DAG node sums every member's `score` into `total_score`. It re-runs
//!   only when the map port is dirty: adding/removing a key, or editing an
//!   element through the widget's per-key borrow. Rendering and layout
//!   never dirty the map, so clean ticks reprocess nothing.
//!
//! Run with: cargo run -p shame-gui --example map_widget

use std::collections::HashMap;

use shame_gui::app::App;
use shame_gui::graph::DagStructRef;
use shame_gui::gui::{Gui, ViewportNode, ViewportTree};
use shame_gui::sm;
use shame_gui::state;
use shame_gui::text::TextSystem;
use shame_gui::{DagStruct, Widget};

/// One map element: derives `Widget` so the map widget can render its
/// fields when the entry is expanded.
#[derive(Clone, Default, DagStruct, Widget)]
pub struct Member {
    name: String,
    score: u32,
    active: bool,
}

/// App state: the member map + a computed total.
#[state]
#[derive(Clone, Default, DagStruct, Widget)]
pub struct MemberState {
    members: HashMap<String, Member>,
    total_score: u32,
}

/// Builds the app: a seeded member map, a sum DAG node, and the GUI.
pub fn map_widget_scene() -> App<MemberState> {
    let mut app = App::<MemberState>::new("HashMap widget — add/remove/expand");
    {
        let s = app.state_mut();
        s.members.insert(
            "alice".to_string(),
            Member {
                name: "Alice A".to_string(),
                score: 3,
                active: true,
            },
        );
        s.members.insert(
            "bob".to_string(),
            Member {
                name: "Bob B".to_string(),
                score: 5,
                active: false,
            },
        );
    }

    let p = MemberState::ports();

    // Sum of all member scores — re-runs only when the map port is dirty.
    app.graph_mut().add_node(
        {
            let m = p.members;
            let out = p.total_score;
            move |gref: &mut DagStructRef<MemberState>, _gpu: Option<&sm::Gpu>| {
                let total = m.read(gref).values().map(|member| member.score).sum();
                out.write(gref, total);
            }
        },
        p.members,
        p.total_score,
        None,
    );

    let children = MemberState::into_viewport_nodes();
    let tree = ViewportTree::new(ViewportNode::container(children));
    app.add_gui(Gui::new(tree));
    app
}

fn main() {
    map_widget_scene().run(TextSystem::new());
}
