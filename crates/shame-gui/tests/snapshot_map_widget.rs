//! GPU snapshot of the map widget scene (`examples/map_widget.rs`):
//! the members list with "alice" expanded into its element form.
//!
//! The expand click is injected through `App::step` before the window runs
//! (runner-emitted events are only consumed by `step`, not the winit loop),
//! so the first rendered frame already shows the expanded entry.

mod common;

#[path = "../examples/map_widget.rs"]
mod scene;

use shame_gui::gui::event::{InputEvent, MouseButton};
use shame_gui::math::{Vec2, Vec2u};

#[test]
fn snapshot_map_widget() {
    let mut app = scene::map_widget_scene();

    // The map widget is the first container row (1200×800 window):
    // root padding 6 + label column 96 + gap 8 → editor at (110, 6, 1084, 70);
    // "alice" is the first (sorted) header at y=6; expander centre (119, 17).
    app.step(
        &[InputEvent::MouseDown {
            pos: Vec2::new(119.0, 17.0),
            button: MouseButton::Left,
            pressure: None,
        }],
        1.0 / 60.0,
        0.0,
        Vec2::new(119.0, 17.0),
        Vec2u::new(1200, 800),
    );

    common::compare_snapshot("map_widget", app, common::test_text_system());
}
