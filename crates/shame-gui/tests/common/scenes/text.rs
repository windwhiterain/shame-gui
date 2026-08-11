//! Text scene: wrapping, z-ordering against rects, overlapping text layers.

#![allow(dead_code)]

use shame_gui::Vec2;
use shame_gui::app::App;
use shame_gui::color::Color;
use shame_gui::graph::StateArena;
use shame_gui::rect::Rect;
use shame_gui::shader::RectEntry;
use shame_gui::sm;
use shame_gui::text::{TextObject, TextSystem};

pub fn text_scene() -> (App, TextSystem) {
    let text_system = TextSystem::new();

    let wrapped = "This is a long paragraph that wraps at the given width. \
                   The height is computed from the wrapped line count.";

    let mut app = App::new("shame-gui text");

    let fills: Vec<RectEntry> = vec![
        RectEntry {
            rect: Rect::new(Vec2::new(40.0, 40.0), Vec2::new(360.0, 180.0)),
            color: Color::rgb(0.4845, 0.4845, 0.6652).to_linear(),
            z: 0.5,
        },
        RectEntry {
            rect: Rect::new(Vec2::new(440.0, 40.0), Vec2::new(360.0, 180.0)),
            color: Color::rgb(0.6652, 0.4845, 0.4845).to_linear(),
            z: 0.5,
        },
    ];

    let texts: Vec<TextObject> = vec![
        TextObject::new("Text above the left panel")
            .at_position(Vec2::new(60.0, 60.0))
            .with_font_size(20.0)
            .with_color(Color::WHITE)
            .with_z(0.1),
        TextObject::new(wrapped)
            .at_position(Vec2::new(460.0, 60.0))
            .with_font_size(16.0)
            .with_color(Color::rgb(1.0, 1.0, 0.8))
            .with_z(0.1)
            .with_wrap_width(320.0),
        TextObject::new("Hidden behind the panel (z = 0.9)")
            .at_position(Vec2::new(60.0, 120.0))
            .with_font_size(18.0)
            .with_color(Color::rgb(0.8, 1.0, 0.8))
            .with_z(0.9),
        TextObject::new("Overlapping text — bottom layer")
            .at_position(Vec2::new(460.0, 320.0))
            .with_font_size(22.0)
            .with_color(Color::WHITE)
            .with_z(0.3),
        TextObject::new("top layer")
            .at_position(Vec2::new(500.0, 344.0))
            .with_font_size(22.0)
            .with_color(Color::rgb(1.0, 0.6, 0.2))
            .with_z(0.2),
    ];

    {
        let mut b = app.graph_builder();
        let fills_port = b.render().fills;
        let texts_port = b.render().texts;
        b.add_node(
            {
                let f = fills;
                let fp = fills_port;
                move |arena: &mut StateArena, _gpu: Option<&sm::Gpu>| {
                    fp.write(arena, f.clone());
                }
            },
            (),
            fills_port,
        );
        b.add_node(
            {
                let t = texts;
                let tp = texts_port;
                move |arena: &mut StateArena, _gpu: Option<&sm::Gpu>| {
                    tp.write(arena, t.clone());
                }
            },
            (),
            texts_port,
        );
    }
    app.finalize_graph();

    (app, text_system)
}

pub fn text_scene_with(text_system: TextSystem) -> (App, TextSystem) {
    let (app, _) = text_scene();
    (app, text_system)
}
