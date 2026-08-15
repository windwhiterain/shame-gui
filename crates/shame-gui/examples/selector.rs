//! The `MapSelector` derive attribute: `#[widget(selector = "teams")]` on a
//! `String` field turns it into a dropdown constrained to the keys of the
//! named `HashMap<String, T>` field of the same struct.
//!
//! - `teams` is edited with the map widget (add/remove/expand).
//! - `team` is a selector over `teams` — only existing keys can be picked,
//!   and the empty value renders as "(none)".
//! - A DAG node renders the selected team's info; it re-runs when the
//!   selection or the map changes. Deleting the selected key in the map
//!   widget makes the selector reset it to empty, so the info line falls
//!   back to "(none)".
//!
//! Run with: cargo run -p shame-gui --example selector

use std::collections::HashMap;

use shame_gui::app::App;
use shame_gui::graph::DagStructRef;
use shame_gui::gui::{Gui, ViewportNode, ViewportTree};
use shame_gui::sm;
use shame_gui::state;
use shame_gui::text::TextSystem;
use shame_gui::{DagStruct, Widget};

/// One team: the map widget's element type (derives `Widget` so entries
/// expand into their field form).
#[derive(Clone, Default, DagStruct, Widget)]
pub struct Team {
    desc: String,
    size: u32,
}

/// App state: the team map (map widget), the selected key (selector — the
/// derive attribute wires it to `teams`), and a DAG-computed description of
/// the selection.
#[state]
#[derive(Clone, Default, DagStruct, Widget)]
pub struct SelectorState {
    teams: HashMap<String, Team>,
    #[widget(selector = "teams")]
    team: String,
    team_info: String,
}

/// Builds the app: a seeded team map, a selection-info DAG node, and the
/// derived GUI tree (the `team` row is a selector, not a text input).
pub fn selector_scene() -> App<SelectorState> {
    let mut app = App::<SelectorState>::new("Key selector — HashMap constraint");
    {
        let s = app.state_mut();
        s.teams.insert(
            "core".to_string(),
            Team {
                desc: "Core engine".to_string(),
                size: 3,
            },
        );
        s.teams.insert(
            "ui".to_string(),
            Team {
                desc: "GUI framework".to_string(),
                size: 2,
            },
        );
        s.team = "core".to_string();
    }

    let p = SelectorState::ports();

    // The selected team's info — re-runs when the selection or the map
    // changes (a selection deleted from the map is reset to empty by the
    // selector's render, so the DAG sees the empty value one tick later).
    app.graph_mut().add_node(
        {
            let teams = p.teams;
            let team = p.team;
            let out = p.team_info;
            move |gref: &mut DagStructRef<SelectorState>, _gpu: Option<&sm::Gpu>| {
                let current = team.read(gref).clone();
                let info = if current.is_empty() {
                    "(none)".to_string()
                } else {
                    match teams.read(gref).get(&current) {
                        Some(t) => format!("{current} — {} ({} people)", t.desc, t.size),
                        None => format!("{current} — missing"),
                    }
                };
                out.write(gref, info);
            }
        },
        (p.teams, p.team),
        p.team_info,
        None,
    );

    let children = SelectorState::into_viewport_nodes();
    let tree = ViewportTree::new(ViewportNode::container(children));
    app.add_gui(Gui::new(tree));
    app
}

fn main() {
    selector_scene().run(TextSystem::new());
}
