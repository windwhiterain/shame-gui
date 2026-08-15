//! GPU snapshot of the selector scene (`examples/selector.rs`): the team
//! map widget, the `MapSelector` with its sorted key list open ("core"
//! highlighted as the current selection), and the DAG-computed info line.
//!
//! The open click is injected through `App::step` before the window runs
//! (runner-emitted events are only consumed by `step`, not the winit loop),
//! so the first rendered frame already shows the expanded list.

mod common;

#[path = "../examples/selector.rs"]
mod scene;

use shame_gui::gui::event::{InputEvent, MouseButton};
use shame_gui::math::{Vec2, Vec2u};

#[test]
fn snapshot_selector() {
    let mut app = scene::selector_scene();

    // The selector is the second container row (1200×800 window):
    // row 1 = teams map (70 tall, editor at (110, 6, 1084, 70)); gap 8 →
    // row 2 = selector editor at (110, 84, 1084, 22); header centre (600, 95).
    app.step(
        &[InputEvent::MouseDown {
            pos: Vec2::new(600.0, 95.0),
            button: MouseButton::Left,
            pressure: None,
        }],
        1.0 / 60.0,
        0.0,
        Vec2::new(600.0, 95.0),
        Vec2u::new(1200, 800),
    );

    common::compare_snapshot("selector", app, common::test_text_system());
}
