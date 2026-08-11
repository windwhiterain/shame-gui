//! Push constant snapshot — custom material with Rect push constant, no widgets or DAG.

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

#[derive(shame_gui::GpuStruct, Clone, Copy)]
#[repr(C, align(16))]
struct PcInstance {
    z: f32,
    _pad: Vec4,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
struct PcMaterial;

impl Material for PcMaterial {
    type Instance = PcInstance;
    type PushConstant = Rect;

    fn build(&self, gpu: &sm::Gpu) -> PipelineData {
        let pipeline = {
            let mut encoder: sm::PipelineEncoder<sm::pipeline_kind::Render> =
                gpu.create_pipeline_encoder(Default::default()).unwrap();
            let mut drawcall = encoder.new_render_pipeline(sm::Indexing::BufferU32);

            let instance = Self::get_instance(&mut drawcall.bind_groups.at(0))
                .index(drawcall.vertices.instance_index);
            let z = instance.z;

            let pc = Self::push_constant(drawcall.push_constants);
            let pos = pc.pos;
            let size = pc.size;

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
            let fill = frag.fill(sm::vec!(0.2, 0.2, 0.9, 1.0));
            let mut targets = frag
                .attachments
                .depth_test::<sm::tf::Depth24Plus>(sm::DepthTest::less_equal(true));
            targets.next::<sm::SurfaceFormat>().set(fill);
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
struct PcState {
    material: PcMaterial,
    cpu_buffer: InstanceBuffer<PcInstance>,
    constant: Rect,
    gpu_buffer: Option<std::sync::Arc<shame_gui::material::GpuBufferSlot>>,
    bind_group: Option<std::sync::Arc<wgpu::BindGroup>>,
}

fn push_constant_scene() -> App<PcState> {
    let mut app = App::<PcState>::new("push-constant");

    {
        let s = app.state_mut();
        let mut ib = InstanceBuffer::<PcInstance>::new();
        ib.push(&PcInstance {
            z: 0.1,
            _pad: Vec4::default(),
        });
        s.cpu_buffer = ib;
        s.constant = Rect::new(Vec2::new(-1.0, 0.0), Vec2::new(1.0, 1.0));
    }

    let p = PcState::ports();
    app.register_render_object(
        p.material,
        p.cpu_buffer,
        p.constant,
        p.gpu_buffer,
        p.bind_group,
    );

    app
}

#[test]
fn snapshot_push_constant() {
    common::compare_snapshot(
        "push_constant",
        push_constant_scene(),
        common::test_text_system(),
    );
}
