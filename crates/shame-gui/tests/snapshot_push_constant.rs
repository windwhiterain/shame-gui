//! Minimal push constant snapshot — verifies VpRectMaterial with a hardcoded
//! Rect push constant (no ViewportRect, no DAG node).

mod common;

#[test]
fn snapshot_push_constant() {
    common::compare_snapshot(
        "push_constant",
        common::scenes::push_constant_scene(),
        common::test_text_system(),
    );
}
