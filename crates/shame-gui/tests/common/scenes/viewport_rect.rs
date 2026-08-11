//! ViewportRect scene: captures layout rect and feeds it through the DAG to
//! a custom material's push constant. Uses frame 2 to allow the DAG node to
//! react to the first frame's widget write.

#![allow(dead_code)]

use shame_gui::Vec4;
use shame_gui::app::App;
use shame_gui::graph::StateArena;
use shame_gui::graph::{Port, PortGroup};
use shame_gui::gui::WidgetNode;
use shame_gui::gui::primitives::viewport_rect::{
    ViewportRect, ViewportRectData, ViewportRectPorts,
};
use shame_gui::gui::{Gui, SplitDir, ViewportNode, ViewportTree};
use shame_gui::material::{Draw, InstanceBuffer, Material, PipelineData};
use shame_gui::rect::Rect;
use shame_gui::sm;
use sm::prelude::*;

#[derive(shame_gui::GpuStruct, Clone, Copy)]
#[repr(C, align(16))]
pub struct VpRectInstance {
    pub z: f32,
    pub _pad: Vec4,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct VpRectMaterial;

impl Material for VpRectMaterial {
    type Instance = VpRectInstance;
    type PushConstant = Rect;

    fn build(&self, gpu: &sm::Gpu) -> PipelineData {
        let pipeline = {
            let mut encoder: sm::PipelineEncoder<sm::pipeline_kind::Render> =
                gpu.create_pipeline_encoder(Default::default()).unwrap();
            let mut drawcall = encoder.new_render_pipeline(sm::Indexing::BufferU32);

            let instances = Self::get_instance(&mut drawcall.bind_groups.at(0));
            let instance = instances.index(drawcall.vertices.instance_index);
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
            let index = drawcall.vertices.index;
            let position = positions.index(index);
            let primitive = drawcall
                .vertices
                .assemble(position, sm::Draw::triangle_list(sm::Winding::Cw));
            let frag = primitive.rasterize(sm::Accuracy::default());
            let color: sm::vec<f32, x4> = sm::vec!(0.6, 0.1, 0.1, 1.0);
            let fill = frag.fill(color);
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

pub fn viewport_rect_scene() -> App {
    let mut app = App::new("viewport-rect");

    let constant_port: Port<Rect> = Port::new(app.arena_mut().alloc::<Rect>());

    {
        let vp_value = ViewportRect::default();
        let vp_ports = ViewportRectPorts::alloc_slots(app.arena_mut());
        vp_ports.write(app.arena_mut(), vp_value);
        let input_ids = vp_ports.port_ids();

        {
            let mut b = app.graph_builder();
            let fb_port = b.source().framebuffer_size;
            b.add_node(
                {
                    move |arena: &mut StateArena, _gpu: Option<&sm::Gpu>| {
                        let rect = vp_ports.rect.read(arena);
                        let fb = *fb_port.read(arena);
                        constant_port.write(arena, rect.to_ndc(fb));
                    }
                },
                (vp_ports, fb_port),
                constant_port,
            );
        }

        let widget_node =
            WidgetNode::new(vp_ports, ViewportRectData::default()).with_port_ids(input_ids);
        let tree = ViewportTree::new(ViewportNode::split(
            SplitDir::Vertical,
            0.4,
            ViewportNode::container(vec![("captured".into(), ViewportNode::Widget(widget_node))]),
            ViewportNode::container(vec![]),
        ));
        app.add_gui(Gui::new(tree));
    }

    let mut ib = InstanceBuffer::<VpRectInstance>::new();
    ib.push(&VpRectInstance {
        z: 0.1,
        _pad: Vec4::default(),
    });
    let material_port: Port<VpRectMaterial> = Port::new(app.arena_mut().alloc_with(VpRectMaterial));
    let cpu_buffer_port: Port<InstanceBuffer<VpRectInstance>> =
        Port::new(app.arena_mut().alloc_with(ib));
    app.register_render_object(material_port, cpu_buffer_port, constant_port);
    app.finalize_graph();

    app
}
