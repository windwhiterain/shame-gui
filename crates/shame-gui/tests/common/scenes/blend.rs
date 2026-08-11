//! Blend-ordering scene: transparent material renders after opaque.

#![allow(dead_code)]

use shame_gui::Vec2;
use shame_gui::Vec4;
use shame_gui::app::App;
use shame_gui::color::Color;
use shame_gui::graph::Port;
use shame_gui::graph::StateArena;
use shame_gui::material::{Draw, InstanceBuffer, Material, PipelineData};
use shame_gui::rect::Rect;
use shame_gui::shader::RectEntry;
use shame_gui::sm;
use sm::prelude::*;

#[derive(shame_gui::GpuStruct, Clone, Copy)]
#[repr(C, align(16))]
pub struct TransparentRectInstance {
    pub rect: Rect,
    pub color_a: Vec4,
    pub color_b: Vec4,
    pub z: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TransparentRectMaterial;

impl Material for TransparentRectMaterial {
    type Instance = TransparentRectInstance;
    type PushConstant = ();

    fn build(&self, gpu: &sm::Gpu) -> PipelineData {
        let pipeline = {
            let mut encoder: sm::PipelineEncoder<sm::pipeline_kind::Render> =
                gpu.create_pipeline_encoder(Default::default()).unwrap();
            let mut drawcall = encoder.new_render_pipeline(sm::Indexing::BufferU32);

            let instances = Self::get_instance(&mut drawcall.bind_groups.at(0));
            let instance = instances.index(drawcall.vertices.instance_index);
            let pos = instance.rect.pos;
            let size = instance.rect.size;
            let color_a = instance.color_a;
            let color_b = instance.color_b;
            let z = instance.z;

            let corner_colors: sm::Array<sm::vec<f32, x4>, Size<4>> =
                [color_a, color_b, color_b, color_a].to_gpu();

            let positions: sm::Array<sm::vec<f32, x3>, Size<4>> = [
                pos.extend(z),
                (pos + sm::vec!(0.0, size.y)).extend(z),
                (pos + sm::vec!(size.x, 0.0)).extend(z),
                (pos + size).extend(z),
            ]
            .to_gpu();
            let index = drawcall.vertices.index;
            let position = positions.index(index);
            let corner_color = corner_colors.at(index);
            let primitive = drawcall
                .vertices
                .assemble(position, sm::Draw::triangle_list(sm::Winding::Cw));
            let frag = primitive.rasterize(sm::Accuracy::default());
            let color_interp: sm::vec<f32, x4> = frag.fill(corner_color);
            let mut targets = frag
                .attachments
                .depth_test::<sm::tf::Depth24Plus>(sm::DepthTest::less_equal(false));
            targets
                .next::<sm::SurfaceFormat>()
                .blend(sm::Blend::alpha(), color_interp);
            encoder.finish().unwrap()
        };

        PipelineData {
            pipeline,
            draw: Draw::Primitive {
                indices: Box::new([0, 1, 3, 0, 3, 2]),
            },
            blend: Some(sm::conversion::blend(sm::Blend::alpha())),
        }
    }
}

pub fn blend_scene() -> App {
    let mut app = App::new("shame-gui blend");

    let fills: Vec<RectEntry> = vec![RectEntry {
        rect: Rect::new(Vec2::new(100.0, 100.0), Vec2::new(600.0, 400.0)),
        color: Color::rgb(0.2, 0.3, 0.6).to_linear(),
        z: 0.5,
    }];

    let red = Color::new(1.0, 0.2, 0.2, 1.0).to_linear();
    let mut ib = InstanceBuffer::<TransparentRectInstance>::new();
    ib.push(&TransparentRectInstance {
        rect: Rect::new(Vec2::new(-0.5833, -0.05), Vec2::new(0.5, 0.6)),
        color_a: red,
        color_b: Vec4::new(red.x, red.y, red.z, 0.1),
        z: 0.4,
    });

    {
        let mut b = app.graph_builder();
        let fills_port = b.render().fills;
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
    }

    let material_port: Port<TransparentRectMaterial> =
        Port::new(app.arena_mut().alloc_with(TransparentRectMaterial));
    let cpu_buffer_port: Port<InstanceBuffer<TransparentRectInstance>> =
        Port::new(app.arena_mut().alloc_with(ib));
    let constant_port: Port<()> = Port::new(app.arena_mut().alloc::<()>());
    app.register_render_object(material_port, cpu_buffer_port, constant_port);
    app.finalize_graph();

    app
}
