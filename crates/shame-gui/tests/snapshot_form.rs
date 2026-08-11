//! Form snapshot test. One scene per process: winit allows a single
//! `EventLoop` per process, so each scene runs in its own test binary.

mod common;

#[test]
fn snapshot_form() {
    common::compare_snapshot(
        "form",
        common::scenes::form_scene(),
        common::test_text_system(),
    );
}
