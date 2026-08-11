//! Blend ordering snapshot test. Verifies that transparent materials
//! (blend: Some) render after opaque materials (blend: None).

mod common;

#[test]
fn snapshot_blend() {
    common::compare_snapshot(
        "blend",
        common::scenes::blend_scene(),
        shame_gui::text::TextSystem::new(),
    );
}
