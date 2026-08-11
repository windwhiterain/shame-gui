//! Rects scene: batched rects, wireframes, z-ordering, and a custom gradient material.

use shame_gui::DagStruct;
use shame_gui::Vec2;
use shame_gui::Vec4;
use shame_gui::app::App;
use shame_gui::color::Color;
use shame_gui::material::{Draw, InstanceBuffer, Material, PipelineData};
use shame_gui::rect::Rect;
use shame_gui::shader::RectEntry;
use shame_gui::sm;
use shame_gui::state;
use shame_gui::text::TextSystem;
use sm::prelude::*;

#[derive(shame_gui::GpuStruct, Clone, Copy)]
#[repr(C, align(16))]
pub struct GradientRectInstance {
    pub rect: Rect,
    pub color_a: Vec4,
    pub color_b: Vec4,
    pub z: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct GradientRectMaterial;

impl Material for GradientRectMaterial {
    type Instance = GradientRectInstance;
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

            let positions: sm::Array<sm::vec<f32, x3>, Size<4>> = [
                pos.extend(z),
                (pos + sm::vec!(0.0, size.y)).extend(z),
                (pos + sm::vec!(size.x, 0.0)).extend(z),
                (pos + size).extend(z),
            ]
            .to_gpu();
            let corner_colors: sm::Array<sm::vec<f32, x4>, Size<4>> =
                [color_a, color_a, color_b, color_b].to_gpu();
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

/// The custom state: built-in fields (injected by `#[state]`) + the render object's declared ports.
#[state]
#[derive(Clone, Default, DagStruct)]
pub struct RectsState {
    material: GradientRectMaterial,
    cpu_buffer: InstanceBuffer<GradientRectInstance>,
    constant: (),
    gpu_buffer: Option<std::sync::Arc<shame_gui::material::GpuBufferSlot>>,
    bind_group: Option<std::sync::Arc<wgpu::BindGroup>>,
}

pub fn rects_scene() -> App<RectsState> {
    let mut app = App::<RectsState>::new("shame-gui rects");

    let fills: Vec<RectEntry> = vec![
        RectEntry {
            rect: Rect::new(Vec2::new(100.0, 100.0), Vec2::new(600.0, 400.0)),
            color: Color::rgb(0.4845, 0.4845, 0.6652).to_linear(),
            z: 0.3,
        },
        RectEntry {
            rect: Rect::new(Vec2::new(200.0, 150.0), Vec2::new(400.0, 300.0)),
            color: Color::rgb(0.7977, 0.5838, 0.4845).to_linear(),
            z: 0.2,
        },
        RectEntry {
            rect: Rect::new(Vec2::new(300.0, 200.0), Vec2::new(200.0, 150.0)),
            color: Color::rgb(0.9547, 0.9063, 0.4845).to_linear(),
            z: 0.1,
        },
    ];

    let outlines: Vec<RectEntry> = vec![RectEntry {
        rect: Rect::new(Vec2::new(100.0, 100.0), Vec2::new(600.0, 400.0)),
        color: Color::WHITE.to_linear(),
        z: 0.05,
    }];

    let mut ib = InstanceBuffer::<GradientRectInstance>::new();
    ib.push(&GradientRectInstance {
        rect: Rect::new(Vec2::new(0.0833, -0.25), Vec2::new(0.3333, 1.0)),
        color_a: Color::rgb(0.3492, 0.7977, 0.9547).to_linear(),
        color_b: Color::rgb(0.9547, 0.4845, 0.7977).to_linear(),
        z: 0.05,
    });

    let p = RectsState::ports();
    let render = <RectsState as shame_gui::graph::AppState>::render_ports();
    {
        let g = app.graph_mut();
        let fp = render.fills;
        let op = render.outlines;
        g.add_node(
            {
                let f = fills.clone();
                move |gref: &mut shame_gui::graph::DagStructRef<RectsState>,
                      _gpu: Option<&sm::Gpu>| {
                    fp.write(gref, f.clone());
                }
            },
            (),
            fp,
            None,
        );
        g.add_node(
            {
                let o = outlines.clone();
                move |gref: &mut shame_gui::graph::DagStructRef<RectsState>,
                      _gpu: Option<&sm::Gpu>| {
                    op.write(gref, o.clone());
                }
            },
            (),
            op,
            None,
        );
    }

    {
        let s = app.state_mut();
        s.cpu_buffer = ib;
    }

    app.register_render_object(
        p.material,
        p.cpu_buffer,
        p.constant,
        p.gpu_buffer,
        p.bind_group,
    );

    app
}

fn main() {
    rects_scene().run(TextSystem::new());
}
