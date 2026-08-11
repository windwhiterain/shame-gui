//! Text scene — run manually: `cargo run -p shame-gui --example text`
//! Text objects: wrap, z-ordering against rects, overlapping text layers
//! (scene defined in `tests/common/scenes.rs`).

#[path = "../tests/common/scenes/mod.rs"]
mod scenes;

fn main() {
    let (app, text_system) = scenes::text_scene();
    app.run(text_system);
}
