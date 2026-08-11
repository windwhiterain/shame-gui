//! Push constant demo — shows typed push constants in a custom material.
//!
//! The shader reads [`PushParams`] from push constants (per-draw-call)
//! and `RectInstance` from instance data (per-rect). Multiple objects
//! with different push values create separate batches — same pipeline,
//! same shader, different draw calls.
//!
//! Custom render objects use [`App::register_render_object`]:
//! pass a [`Material`] port, a typed [`InstanceBuffer`] port, and a push
//! constant port. Two DAG nodes are created internally to upload the CPU
//! buffer and create the bind group.
//!
//! Run with: cargo run -p shame-gui --example push_demo

use shame_gui::app::App;
use shame_gui::graph::Port;
use shame_gui::material::{Draw, InstanceBuffer, Material, PipelineData};
use shame_gui::math::{Vec2, Vec2u, Vec4};
use shame_gui::rect::Rect;
use shame_gui::shader::{RectEntry, RectInstance};
use shame_gui::text::TextSystem;
use shame_gui::{Color, DagStruct, GpuStruct};

use shame_wgpu as sm;
use sm::prelude::*;

// ── Push constant type ────────────────────────────────────────────────────

#[derive(GpuStruct, DagStruct, Clone, Copy, Default)]
#[repr(C)]
struct PushParams {
    pub fb_size: Vec2u,
    pub offset: Vec2,
    pub tint: Vec4,
}

// ── Custom material ───────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
struct PushDemoMaterial;

impl Material for PushDemoMaterial {
    type Instance = RectInstance;
    type PushConstant = PushParams;
    const HAS_FB_PUSH_CONSTANT: bool = true;

    fn build(&self, gpu: &sm::Gpu) -> PipelineData {
        let pipeline = {
            let mut encoder: sm::PipelineEncoder<sm::pipeline_kind::Render> =
                gpu.create_pipeline_encoder(Default::default()).unwrap();
            let mut drawcall = encoder.new_render_pipeline(sm::Indexing::BufferU32);

            let params = Self::push_constant(drawcall.push_constants);
            let fb = params.fb_size.to_f32();
            let offset = params.offset;
            let tint = params.tint;

            let instances = Self::get_instance(&mut drawcall.bind_groups.at(0));
            let instance = instances.index(drawcall.vertices.instance_index);
            let ppos = instance.rect.pos;
            let psize = instance.rect.size;
            let color = sm::vec!(
                instance.color.x * tint.x,
                instance.color.y * tint.y,
                instance.color.z * tint.z,
                instance.color.w * tint.w
            );
            let z = instance.z;

            let pos = sm::vec!(ppos.x, fb.y - ppos.y - psize.y) * 2.0 / fb - 1.0 + offset;
            let size = psize * 2.0 / fb;

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
            let color_interp: sm::vec<f32, x4> = frag.fill(color);
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

// ── Helper ─────────────────────────────────────────────────────────────────

fn square(x: f32, y: f32, color: Color, z: f32) -> RectEntry {
    RectEntry {
        rect: Rect::new(Vec2::new(x, y), Vec2::new(120.0, 120.0)),
        color: color.to_linear(),
        z,
    }
}

fn make_ib(colors: &[(f32, f32, Color)]) -> InstanceBuffer<RectInstance> {
    let mut ib = InstanceBuffer::new();
    for &(x, y, ref color) in colors {
        ib.push(&RectInstance {
            rect: Rect::new(Vec2::new(x, y), Vec2::new(120.0, 120.0)),
            color: color.to_linear(),
            z: 0.0,
        });
    }
    ib
}

// ── Scene ─────────────────────────────────────────────────────────────────

fn main() {
    let mut app = App::new("Push Constant Demo");

    let colors = [
        (150.0, 80.0, Color::rgb(255.0, 80.0, 80.0)),
        (300.0, 80.0, Color::rgb(80.0, 255.0, 80.0)),
        (450.0, 80.0, Color::rgb(80.0, 80.0, 255.0)),
    ];

    let reg = |app: &mut App, offset: Vec2, tint: Vec4| {
        let material_port: Port<PushDemoMaterial> =
            Port::new(app.arena_mut().alloc_with(PushDemoMaterial));
        let ib = make_ib(&colors);
        let cpu_buffer_port: Port<InstanceBuffer<RectInstance>> =
            Port::new(app.arena_mut().alloc_with(ib));
        let const_port: Port<PushParams> = Port::new(app.arena_mut().alloc_with(PushParams {
            fb_size: Vec2u::new(0, 0),
            offset,
            tint,
        }));
        app.register_render_object(material_port, cpu_buffer_port, const_port);
    };

    reg(
        &mut app,
        Vec2::new(0.25, -0.33),
        Vec4::new(0.5, 1.0, 0.5, 1.0),
    );
    reg(
        &mut app,
        Vec2::new(0.0, -0.67),
        Vec4::new(1.0, 1.0, 1.0, 1.0),
    );
    reg(
        &mut app,
        Vec2::new(0.33, -0.125),
        Vec4::new(0.8, 0.8, 2.0, 1.0),
    );
    app.finalize_graph();

    app.run(TextSystem::new());
}
