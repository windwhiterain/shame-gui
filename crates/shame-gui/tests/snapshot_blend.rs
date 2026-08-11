//! Blend ordering snapshot test.

mod common;

#[path = "../examples/blend.rs"]
mod scene;

#[test]
fn snapshot_blend() {
    common::compare_snapshot(
        "blend",
        scene::blend_scene(),
        shame_gui::text::TextSystem::new(),
    );
}
