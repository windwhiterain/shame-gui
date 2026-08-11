//! Rects snapshot test. One scene per process: winit allows a single
//! `EventLoop` per process, so each scene runs in its own test binary.

mod common;

#[test]
fn snapshot_rects() {
    common::compare_snapshot(
        "rects",
        common::scenes::rects_scene(),
        shame_gui::text::TextSystem::new(),
    );
}
