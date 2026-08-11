//! Form snapshot test.

mod common;

#[path = "../examples/form.rs"]
mod scene;

#[test]
fn snapshot_form() {
    common::compare_snapshot("form", scene::form_scene(), common::test_text_system());
}
