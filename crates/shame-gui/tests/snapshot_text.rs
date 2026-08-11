//! Text snapshot test.

mod common;

#[path = "../examples/text.rs"]
mod scene;

#[test]
fn snapshot_text() {
    let (app, text_system) = scene::text_scene_with(common::test_text_system());
    common::compare_snapshot("text", app, text_system);
}
