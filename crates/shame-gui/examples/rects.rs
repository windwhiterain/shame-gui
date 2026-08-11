//! Rects scene — run manually: `cargo run -p shame-gui --example rects`
//! Batched rects, wireframes, z-ordering, and a custom gradient material
//! (scene defined in `tests/common/scenes.rs`).

#[path = "../tests/common/scenes/mod.rs"]
mod scenes;

fn main() {
    scenes::rects_scene().run(shame_gui::text::TextSystem::new());
}
