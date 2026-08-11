//! ViewportRect snapshot — ViewportRect + DAG + custom material, tested in isolation.

#![allow(dead_code)]

mod common;

use shame_gui::DagStruct;
use shame_gui::Vec4;
use shame_gui::app::App;
use shame_gui::gui::primitives::viewport_rect::{ViewportRect, ViewportRectData};
use shame_gui::gui::{Gui, SplitDir, ViewportNode, ViewportTree, WidgetNode};
use shame_gui::material::{Draw, InstanceBuffer, Material, PipelineData};
use shame_gui::rect::Rect;
use shame_gui::sm;
use shame_gui::state;
use sm::prelude::*;

#[derive(shame_gui::GpuStruct, Clone, Copy)]
#[repr(C, align(16))]
struct VpTestInstance {
    z: f32,
    _pad: Vec4,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
struct VpTestMaterial;

impl Material for VpTestMaterial {
    type Instance = VpTestInstance;
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
            let fill = frag.fill(sm::vec!(0.1, 0.7, 0.2, 1.0));
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
struct VpState {
    viewport: ViewportRect,
    material: VpTestMaterial,
    cpu_buffer: InstanceBuffer<VpTestInstance>,
    constant: Rect,
    gpu_buffer: Option<std::sync::Arc<shame_gui::material::GpuBufferSlot>>,
    bind_group: Option<std::sync::Arc<wgpu::BindGroup>>,
}

fn viewport_rect_scene() -> App<VpState> {
    let mut app = App::<VpState>::new("viewport-rect-test");

    {
        let s = app.state_mut();
        let mut ib = InstanceBuffer::<VpTestInstance>::new();
        ib.push(&VpTestInstance {
            z: 0.1,
            _pad: Vec4::default(),
        });
        s.cpu_buffer = ib;
    }

    let p = VpState::ports();
    let src = <VpState as shame_gui::graph::AppState>::source_ports();

    {
        let fb_port = src.framebuffer_size;
        let vp = p.viewport;
        let constant = p.constant;
        app.graph_mut().add_node(
            {
                move |gref: &mut shame_gui::graph::DagStructRef<VpState>, _gpu: Option<&sm::Gpu>| {
                    let rect = vp.read(gref).rect;
                    let fb = *fb_port.read(gref);
                    constant.write(gref, rect.to_ndc(fb));
                }
            },
            (p.viewport, fb_port),
            constant,
            None,
        );
    }

    let vp_widget = WidgetNode::new(p.viewport, ViewportRectData::default());
    let tree = ViewportTree::new(ViewportNode::split(
        SplitDir::Vertical,
        0.4,
        ViewportNode::container(vec![("captured".into(), ViewportNode::Widget(vp_widget))]),
        ViewportNode::container(vec![]),
    ));
    app.add_gui(Gui::new(tree));

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
fn snapshot_viewport_rect() {
    common::compare_snapshot_frame2(
        "viewport_rect",
        viewport_rect_scene(),
        common::test_text_system(),
    );
}
