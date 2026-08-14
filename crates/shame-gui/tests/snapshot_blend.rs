//! Blend ordering snapshot test.

#![allow(dead_code)]

mod common;

#[path = "../examples/blend.rs"]
mod scene;

#[test]
fn snapshot_blend() {
    common::compare_snapshot("blend", scene::blend_scene(), common::test_text_system());
}
