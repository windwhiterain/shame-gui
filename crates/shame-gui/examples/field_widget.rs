//! The nested-struct field widget: `#[derive(Widget)]` structs used as plain
//! fields of a bigger state render their own field rows inside the parent's
//! table — no HashMap wrapper needed.
//!
//! - `Audio` / `Video` derive `Widget` — that makes them usable as fields of
//!   `NestedState`, which also derives `Widget`.
//! - A DAG node sums `audio.volume` and `video.brightness` into `master`. It
//!   re-runs when either whole field port is dirty: editing any field of
//!   `audio` (through the field widget's fresh-tracking borrow) wakes the
//!   `audio` reader. Rendering and layout never dirty anything, so clean
//!   ticks reprocess nothing.
//!
//! Run with: cargo run -p shame-gui --example field_widget

use shame_gui::app::App;
use shame_gui::graph::DagStructRef;
use shame_gui::gui::{Gui, ViewportNode, ViewportTree};
use shame_gui::sm;
use shame_gui::state;
use shame_gui::text::TextSystem;
use shame_gui::{DagStruct, Widget};

/// A nested widget struct — rendered as its own labelled table by the field
/// widget when used as a plain field of `NestedState`.
#[derive(Clone, Default, DagStruct, Widget)]
pub struct Audio {
    device: String,
    volume: f32,
    mute: bool,
}

#[derive(Clone, Default, DagStruct, Widget)]
pub struct Video {
    brightness: f32,
    hdr: bool,
}

/// App state: nested widget structs as plain fields + a computed total.
#[state]
#[derive(Clone, Default, DagStruct, Widget)]
pub struct NestedState {
    audio: Audio,
    video: Video,
    master: f32,
}

/// Builds the app: seeded nested structs, a sum DAG node, and the GUI.
pub fn field_widget_scene() -> App<NestedState> {
    let mut app = App::<NestedState>::new("Nested struct widgets — edit fields inline");
    {
        let s = app.state_mut();
        s.audio = Audio {
            device: "Built-in".to_string(),
            volume: 7.0,
            mute: true,
        };
        s.video = Video {
            brightness: 0.5,
            hdr: false,
        };
    }

    let p = NestedState::ports();

    // Sum of the nested structs — re-runs only when a whole field port is
    // dirty (any sub-write through the field widget marks it).
    app.graph_mut().add_node(
        {
            let a = p.audio;
            let v = p.video;
            let out = p.master;
            move |gref: &mut DagStructRef<NestedState>, _gpu: Option<&sm::Gpu>| {
                let m = a.read(gref).volume + v.read(gref).brightness;
                out.write(gref, m);
            }
        },
        (p.audio, p.video),
        p.master,
        None,
    );

    let children = NestedState::into_viewport_nodes();
    let tree = ViewportTree::new(ViewportNode::container(children));
    app.add_gui(Gui::new(tree));
    app
}

fn main() {
    field_widget_scene().run(TextSystem::new());
}
