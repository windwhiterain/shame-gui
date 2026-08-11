//! Text snapshot test. One scene per process: winit allows a single
//! `EventLoop` per process, so each scene runs in its own test binary.

mod common;

#[test]
fn snapshot_text() {
    let (app, text_system) = common::scenes::text_scene_with(common::test_text_system());
    common::compare_snapshot("text", app, text_system);
}
