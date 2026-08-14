//! Push constant demo — time-varying push constants animate multiple rects.
//!
//! One material, one render object, one instance buffer with many rects, and
//! **one push constant** that changes every frame. A single DAG node reads
//! [`elapsed`] from the framework's built-in source ports and writes a new
//! [`PushParams`] each tick.
//!
//! Run with: cargo run -p shame-gui --example push_constant

use shame_gui::app::App;
use shame_gui::material::{Draw, InstanceBuffer, Material, PipelineData};
use shame_gui::math::{Vec2, Vec2u, Vec4};
use shame_gui::rect::Rect;
use shame_gui::shader::RectInstance;
use shame_gui::sm;
use shame_gui::state;
use shame_gui::text::TextSystem;
use shame_gui::{Color, DagStruct, GpuStruct};
use sm::prelude::*;

// ── Push constant: one struct drives every instance in the draw call ────────

#[derive(GpuStruct, DagStruct, Clone, Copy, Default)]
#[repr(C)]
struct PushParams {
    /// The framework auto-fills this field each frame (HAS_FB_PUSH_CONSTANT).
    pub fb_size: Vec2u,
    /// NDC offset applied to every instance.
    pub offset: Vec2,
    /// Multiplied against each instance's own colour.
    pub tint: Vec4,
}

// ── Custom material ─────────────────────────────────────────────────────────

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

            // Shared pixel→NDC formula, then the per-instance NDC offset.
            let (pos, size) = shame_gui::dual::pixel_rect_to_ndc_gpu(
                sm::vec!(ppos.x, ppos.y),
                sm::vec!(psize.x, psize.y),
                fb,
            );
            let pos = pos + offset;

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
            sm::discard_if(color_interp.w.lt(shame_gui::shader::ALPHA_DISCARD));
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

// ── Helpers ─────────────────────────────────────────────────────────────────

fn build_instances() -> InstanceBuffer<RectInstance> {
    let size = 80.0;
    let gap = 20.0;
    let cols: [(f32, Color); 3] = [
        (120.0, Color::rgb(255.0, 80.0, 80.0)),
        (120.0 + size + gap, Color::rgb(80.0, 255.0, 80.0)),
        (120.0 + 2.0 * (size + gap), Color::rgb(80.0, 80.0, 255.0)),
    ];
    let rows: [f32; 3] = [80.0, 80.0 + size + gap, 80.0 + 2.0 * (size + gap)];

    let mut ib = InstanceBuffer::new();
    for &y in &rows {
        for &(x, ref color) in &cols {
            ib.push(&RectInstance {
                rect: Rect::new(Vec2::new(x, y), Vec2::new(size, size)),
                color: color.to_linear(),
                z: 0.0,
            });
        }
    }
    ib
}

// ── State ───────────────────────────────────────────────────────────────────

#[state]
#[derive(Clone, Default, DagStruct)]
pub struct PushState {
    material: PushDemoMaterial,
    cpu_buffer: InstanceBuffer<RectInstance>,
    constant: PushParams,
    gpu_buffer: Option<std::sync::Arc<shame_gui::material::GpuBufferSlot>>,
    bind_group: Option<std::sync::Arc<wgpu::BindGroup>>,
}

fn main() {
    let mut app = App::<PushState>::new("Push Constant Demo — time-varying");

    {
        let s = app.state_mut();
        s.cpu_buffer = build_instances();
        s.constant = PushParams {
            fb_size: Vec2u::new(0, 0),
            offset: Vec2::new(0.0, -0.33),
            tint: Vec4::new(1.0, 1.0, 1.0, 1.0),
        };
    }

    let p = PushState::ports();
    let src = <PushState as shame_gui::graph::AppState>::source_ports();
    let elapsed = src.elapsed;
    let constant = p.constant;
    app.graph_mut().add_node(
        {
            move |gref: &mut shame_gui::graph::DagStructRef<PushState>, _gpu: Option<&sm::Gpu>| {
                let t = *elapsed.read(gref);
                let offset = Vec2::new((t * 0.8).sin() * 0.15, (t * 1.1).cos() * 0.15);
                let tint = Vec4::new(
                    (t * 0.7).sin() * 0.4 + 0.6,
                    (t * 0.9 + 1.0).sin() * 0.4 + 0.6,
                    (t * 1.1 + 2.0).sin() * 0.4 + 0.6,
                    1.0,
                );
                constant.write(
                    gref,
                    PushParams {
                        fb_size: Default::default(),
                        offset,
                        tint,
                    },
                );
            }
        },
        elapsed,
        constant,
        None,
    );

    app.register_render_object(
        p.material,
        p.cpu_buffer,
        p.constant,
        p.gpu_buffer,
        p.bind_group,
    );

    app.run(TextSystem::new());
}
