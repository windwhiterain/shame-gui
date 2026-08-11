//! Rects snapshot test.

mod common;

#[path = "../examples/rects.rs"]
mod scene;

#[test]
fn snapshot_rects() {
    common::compare_snapshot(
        "rects",
        scene::rects_scene(),
        shame_gui::text::TextSystem::new(),
    );
}
