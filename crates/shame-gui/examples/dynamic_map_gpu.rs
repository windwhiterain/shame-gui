//! Dynamic HashMap GPU batching: each map element carries its own
//! `InstanceBuffer` (CPU, user-filled) + `GpuInstanceBuffer` (GPU, framework-
//! written) and every element is drawn in a single indirect dispatch.
//!
//! The element's `gpu_buffer` is a RAII handle: when a key is removed from the
//! map, the element is dropped and its GPU buffer is released automatically.
//!
//! Run with: cargo run -p shame-gui --example dynamic_map_gpu

use std::collections::HashMap;

use shame_gui::DagStruct;
use shame_gui::Vec2;
use shame_gui::app::App;
use shame_gui::color::Color;
use shame_gui::material::{GpuInstanceBuffer, InstanceBuffer};
use shame_gui::rect::Rect;
use shame_gui::shader::{RectInstance, RectMaterial, ViewportParams};
use shame_gui::state;
use shame_gui::text::TextSystem;

/// One element = one colored quad. `cpu_buffer` is filled by the user;
/// `gpu_buffer` is written by the framework and releases itself on removal.
#[derive(Clone, Default, DagStruct)]
struct Element {
    cpu_buffer: InstanceBuffer<RectInstance>,
    gpu_buffer: Option<GpuInstanceBuffer>,
}

/// App state: the dynamic map + the shared material/push constant.
#[state]
#[derive(Clone, Default, DagStruct)]
struct MapState {
    elements: HashMap<u32, Element>,
    material: RectMaterial,
    constant: ViewportParams,
}

fn main() {
    let mut app = App::<MapState>::new("Dynamic HashMap GPU batching");

    {
        let s = app.state_mut();
        let colors = [
            Color::rgb(1.0, 0.2, 0.2),
            Color::rgb(0.2, 1.0, 0.2),
            Color::rgb(0.2, 0.4, 1.0),
            Color::rgb(1.0, 0.8, 0.2),
        ];
        for (i, c) in colors.iter().enumerate() {
            let mut ib = InstanceBuffer::<RectInstance>::new();
            ib.push(&RectInstance {
                rect: Rect::new(
                    Vec2::new(40.0 + i as f32 * 160.0, 60.0),
                    Vec2::new(120.0, 120.0),
                ),
                color: c.to_linear(),
                z: 0.1,
            });
            s.elements.insert(
                i as u32,
                Element {
                    cpu_buffer: ib,
                    gpu_buffer: None,
                },
            );
        }
    }

    let p = MapState::ports();
    let el = Element::ports();
    app.register_map_render_objects_batched(
        p.elements,
        p.material,
        p.constant,
        el.cpu_buffer,
        el.gpu_buffer,
    );

    app.run(TextSystem::new());
}
