//! Dynamic-map batched rendering snapshot — a `HashMap` of elements, each with
//! its own `InstanceBuffer` + `GpuInstanceBuffer`, drawn in one indirect
//! dispatch.

#![allow(dead_code)]

mod common;

use std::collections::HashMap;

use shame_gui::DagStruct;
use shame_gui::Vec2;
use shame_gui::Vec4;
use shame_gui::app::App;
use shame_gui::material::{GpuInstanceBuffer, InstanceBuffer};
use shame_gui::rect::Rect;
use shame_gui::shader::{RectInstance, RectMaterial, ViewportParams};
use shame_gui::state;

#[derive(Clone, Default, DagStruct)]
struct Element {
    cpu_buffer: InstanceBuffer<RectInstance>,
    gpu_buffer: Option<GpuInstanceBuffer>,
}

#[state]
#[derive(Clone, Default, DagStruct)]
struct MapState {
    elements: HashMap<u32, Element>,
    material: RectMaterial,
    constant: ViewportParams,
}

fn map_batched_scene() -> App<MapState> {
    let mut app = App::<MapState>::new("map-batched-indirect-test");

    {
        let s = app.state_mut();
        // Three non-overlapping vertical stripes (deterministic regardless of
        // HashMap iteration order).
        let stripes = [
            (
                0u32,
                Vec2::new(0.0, 0.0),
                Vec2::new(400.0, 800.0),
                Vec4::new(1.0, 0.0, 0.0, 1.0),
            ),
            (
                1u32,
                Vec2::new(400.0, 0.0),
                Vec2::new(400.0, 800.0),
                Vec4::new(0.0, 1.0, 0.0, 1.0),
            ),
            (
                2u32,
                Vec2::new(800.0, 0.0),
                Vec2::new(400.0, 800.0),
                Vec4::new(0.0, 0.0, 1.0, 1.0),
            ),
        ];
        for (i, pos, size, color) in stripes {
            let mut ib = InstanceBuffer::<RectInstance>::new();
            ib.push(&RectInstance {
                rect: Rect::new(pos, size),
                color,
                z: 0.1,
            });
            s.elements.insert(
                i,
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

    app
}

#[test]
fn snapshot_map_batched_indirect() {
    common::compare_snapshot(
        "map_batched_indirect",
        map_batched_scene(),
        common::test_text_system(),
    );
}
