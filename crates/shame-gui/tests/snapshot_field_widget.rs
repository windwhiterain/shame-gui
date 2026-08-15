//! GPU snapshot of the nested-struct field widget scene
//! (`examples/field_widget.rs`): `Audio` and `Video` render their own field
//! rows inside the root table — no interaction needed, the nesting is
//! visible on the first frame.

mod common;

#[path = "../examples/field_widget.rs"]
mod scene;

#[test]
fn snapshot_field_widget() {
    let app = scene::field_widget_scene();
    common::compare_snapshot("field_widget", app, common::test_text_system());
}
