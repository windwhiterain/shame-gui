//! Rects snapshot test.

#![allow(dead_code)]

mod common;

#[path = "../examples/rects.rs"]
mod scene;

#[test]
fn snapshot_rects() {
    common::compare_snapshot("rects", scene::rects_scene(), common::test_text_system());
}
