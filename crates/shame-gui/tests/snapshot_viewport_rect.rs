//! ViewportRect snapshot test — verifies that a ViewportRect widget captures
//! its layout rect and feeds it through the DAG to a custom render object's
//! push constant. Uses frame 2 to allow the DAG node to react to the first
//! frame's widget write.

mod common;

#[test]
fn snapshot_viewport_rect() {
    common::compare_snapshot_frame2(
        "viewport_rect",
        common::scenes::viewport_rect_scene(),
        common::test_text_system(),
    );
}
