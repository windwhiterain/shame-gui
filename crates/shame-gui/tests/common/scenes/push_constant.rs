//! Minimal push-constant scene: VpRectMaterial with a hardcoded NDC Rect
//! push constant (no ViewportRect, no DAG node).

#![allow(dead_code)]

use super::viewport_rect::{VpRectInstance, VpRectMaterial};
use shame_gui::Vec2;
use shame_gui::Vec4;
use shame_gui::app::App;
use shame_gui::graph::Port;
use shame_gui::material::InstanceBuffer;
use shame_gui::rect::Rect;

pub fn push_constant_scene() -> App {
    let mut app = App::new("push-constant");

    let mut ib = InstanceBuffer::<VpRectInstance>::new();
    ib.push(&VpRectInstance {
        z: 0.1,
        _pad: Vec4::default(),
    });
    let material_port: Port<VpRectMaterial> = Port::new(app.arena_mut().alloc_with(VpRectMaterial));
    let cpu_buffer_port: Port<InstanceBuffer<VpRectInstance>> =
        Port::new(app.arena_mut().alloc_with(ib));
    let constant_port: Port<Rect> = Port::new(
        app.arena_mut()
            .alloc_with(Rect::new(Vec2::new(-1.0, 0.0), Vec2::new(1.0, 1.0))),
    );
    app.register_render_object(material_port, cpu_buffer_port, constant_port);
    app.finalize_graph();

    app
}
