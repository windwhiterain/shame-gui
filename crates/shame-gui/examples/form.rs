//! Form scene — run manually: `cargo run -p shame-gui --example form`
//! Interactive widgets: checkbox, button, tabs, split divider (scene
//! defined in `tests/common/scenes.rs`).

#[path = "../tests/common/scenes/mod.rs"]
mod scenes;

fn main() {
    scenes::form_scene().run(shame_gui::text::TextSystem::new());
}
