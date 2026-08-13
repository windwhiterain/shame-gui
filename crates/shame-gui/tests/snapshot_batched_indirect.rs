//! Batched render objects snapshot — N objects sharing one material + push
//! constant, drawn with a single `multi_draw_indexed_indirect` dispatch.

#![allow(dead_code)]

mod common;

use shame_gui::DagStruct;
use shame_gui::Vec2;
use shame_gui::Vec4;
use shame_gui::app::App;
use shame_gui::material::{Draw, InstanceBuffer, Material, PipelineData};
use shame_gui::rect::Rect;
use shame_gui::sm;
use shame_gui::state;
use sm::prelude::*;

/// Per-instance data: an NDC-space rect + linear color + depth.
#[derive(shame_gui::GpuStruct, Clone, Copy)]
#[repr(C, align(16))]
struct NdcInstance {
    rect: Rect,
    color: Vec4,
    z: f32,
}

/// Material with a shared NDC offset push constant (identical across the
/// batch) and per-instance rect/color.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
struct NdcMaterial;

impl Material for NdcMaterial {
    type Instance = NdcInstance;
    type PushConstant = Rect;

    fn build(&self, gpu: &sm::Gpu) -> PipelineData {
        let pipeline = {
            let mut encoder: sm::PipelineEncoder<sm::pipeline_kind::Render> =
                gpu.create_pipeline_encoder(Default::default()).unwrap();
            let mut drawcall = encoder.new_render_pipeline(sm::Indexing::BufferU32);

            let offset = Self::push_constant(drawcall.push_constants).pos;
            let instance = Self::get_instance(&mut drawcall.bind_groups.at(0))
                .index(drawcall.vertices.instance_index);
            let pos = instance.rect.pos + offset;
            let size = instance.rect.size;
            let color = instance.color;
            let z = instance.z;

            let positions: sm::Array<sm::vec<f32, x3>, Size<4>> = [
                pos.extend(z),
                (pos + sm::vec!(0.0, size.y)).extend(z),
                (pos + sm::vec!(size.x, 0.0)).extend(z),
                (pos + size).extend(z),
            ]
            .to_gpu();
            let position = positions.index(drawcall.vertices.index);
            let primitive = drawcall
                .vertices
                .assemble(position, sm::Draw::triangle_list(sm::Winding::Cw));
            let frag = primitive.rasterize(sm::Accuracy::default());
            let color_interp = frag.fill(color);
            sm::discard_if(color_interp.w.lt(0.5));
            let mut targets = frag
                .attachments
                .depth_test::<sm::tf::Depth24Plus>(sm::DepthTest::less_equal(true));
            targets.next::<sm::SurfaceFormat>().set(color_interp);
            encoder.finish().unwrap()
        };

        PipelineData {
            pipeline,
            draw: Draw::Primitive {
                indices: Box::new([0, 1, 3, 0, 3, 2]),
            },
            blend: None,
        }
    }
}

#[state]
#[derive(Clone, Default, DagStruct)]
struct BatchedState {
    material: NdcMaterial,
    constant: Rect,
    cpu0: InstanceBuffer<NdcInstance>,
    cpu1: InstanceBuffer<NdcInstance>,
    cpu2: InstanceBuffer<NdcInstance>,
}

fn batched_scene() -> App<BatchedState> {
    let mut app = App::<BatchedState>::new("batched-indirect-test");

    {
        let s = app.state_mut();
        // Three vertical stripes in NDC, one object each (its own InstanceBuffer).
        let mut ib0 = InstanceBuffer::<NdcInstance>::new();
        ib0.push(&NdcInstance {
            rect: Rect::new(Vec2::new(-1.0, -1.0), Vec2::new(2.0 / 3.0, 2.0)),
            color: Vec4::new(1.0, 0.0, 0.0, 1.0),
            z: 0.1,
        });
        let mut ib1 = InstanceBuffer::<NdcInstance>::new();
        ib1.push(&NdcInstance {
            rect: Rect::new(Vec2::new(-1.0 / 3.0, -1.0), Vec2::new(2.0 / 3.0, 2.0)),
            color: Vec4::new(0.0, 1.0, 0.0, 1.0),
            z: 0.1,
        });
        let mut ib2 = InstanceBuffer::<NdcInstance>::new();
        ib2.push(&NdcInstance {
            rect: Rect::new(Vec2::new(1.0 / 3.0, -1.0), Vec2::new(2.0 / 3.0, 2.0)),
            color: Vec4::new(0.0, 0.0, 1.0, 1.0),
            z: 0.1,
        });
        s.cpu0 = ib0;
        s.cpu1 = ib1;
        s.cpu2 = ib2;
    }

    let p = BatchedState::ports();
    app.register_render_objects_batched(p.material, p.constant, [p.cpu0, p.cpu1, p.cpu2]);

    app
}

#[test]
fn snapshot_batched_indirect() {
    common::compare_snapshot(
        "batched_indirect",
        batched_scene(),
        common::test_text_system(),
    );
}
