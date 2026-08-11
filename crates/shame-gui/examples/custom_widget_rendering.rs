//! Custom widget rendering: ViewportRect + state-driven DAG + custom material.
//!
//! 1. **State struct** with editable `hue` / `saturation` fields drives
//!    interactive slider widgets on the left side of the window.
//! 2. A **ViewportRect** (passive, invisible widget) captures the pixel-space
//!    [`Rect`] assigned to the right pane during layout.
//! 3. A **DAG node** reads the state values, the captured pixel rect, and the
//!    framebuffer size, then converts the pixel rect to NDC and writes a
//!    `CustomPushConstant` into the state.
//! 4. A **custom material** registered with [`App::register_render_object`]
//!    reads that push constant each frame and draws a tinted quad filling
//!    exactly the captured area.
//!
//! Run with: cargo run -p shame-gui --example custom_widget_rendering

use shame_gui::Vec4;
use shame_gui::app::App;
use shame_gui::gui::primitives::viewport_rect::{ViewportRect, ViewportRectData};
use shame_gui::gui::{Gui, SplitDir, ViewportNode, ViewportTree, WidgetNode};
use shame_gui::material::{Draw, InstanceBuffer, Material, PipelineData};
use shame_gui::rect::Rect;
use shame_gui::sm;
use shame_gui::state;
use shame_gui::text::TextSystem;
use shame_gui::{DagStruct, Widget};
use sm::prelude::*;

// ── Push constant: NDC rect + computed tint, fed to the GPU each frame ───────

#[derive(shame_gui::GpuStruct, DagStruct, Clone, Copy, Default)]
#[repr(C)]
struct CustomPushConstant {
    ndc_rect: Rect,
    tint: Vec4,
}

// ── Instance: one quad per registered render object ─────────────────────────

#[derive(shame_gui::GpuStruct, Clone, Copy)]
#[repr(C, align(16))]
struct CustomInstance {
    z: f32,
    _pad: Vec4,
}

// ── Material: draws a tinted quad at the push constant's NDC rect ───────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
struct CustomMaterial;

impl Material for CustomMaterial {
    type Instance = CustomInstance;
    type PushConstant = CustomPushConstant;

    fn build(&self, gpu: &sm::Gpu) -> PipelineData {
        let pipeline = {
            let mut encoder: sm::PipelineEncoder<sm::pipeline_kind::Render> =
                gpu.create_pipeline_encoder(Default::default()).unwrap();
            let mut drawcall = encoder.new_render_pipeline(sm::Indexing::BufferU32);

            let instance = Self::get_instance(&mut drawcall.bind_groups.at(0))
                .index(drawcall.vertices.instance_index);
            let z = instance.z;

            let pc = Self::push_constant(drawcall.push_constants);
            let pos = pc.ndc_rect.pos;
            let size = pc.ndc_rect.size;
            let tint = pc.tint;

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
            let fill = frag.fill(tint);
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

// ── Helpers ─────────────────────────────────────────────────────────────────

/// HSV → linear RGB (CPU-side, for the DAG node).
fn hue_sat_to_rgb(hue: f32, saturation: f32) -> Vec4 {
    let h = hue.rem_euclid(1.0);
    let s = saturation.clamp(0.0, 1.0);
    let v = 1.0;
    let c = v * s;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h * 6.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Vec4::new(r + m, g + m, b + m, 1.0)
}

// ── State ───────────────────────────────────────────────────────────────────

#[state]
#[derive(Clone, Default, DagStruct, Widget)]
pub struct CustomRenderState {
    hue: f32,
    saturation: f32,
    #[widget(skip)]
    viewport: ViewportRect,
    #[widget(skip)]
    material: CustomMaterial,
    #[widget(skip)]
    cpu_buffer: InstanceBuffer<CustomInstance>,
    #[widget(skip)]
    constant: CustomPushConstant,
    #[widget(skip)]
    gpu_buffer: Option<std::sync::Arc<shame_gui::material::GpuBufferSlot>>,
    #[widget(skip)]
    bind_group: Option<std::sync::Arc<wgpu::BindGroup>>,
}

fn main() {
    let mut app = App::<CustomRenderState>::new("Custom Widget Rendering");

    {
        let s = app.state_mut();
        s.hue = 0.6;
        s.saturation = 0.8;
        let mut ib = InstanceBuffer::<CustomInstance>::new();
        ib.push(&CustomInstance {
            z: 0.1,
            _pad: Vec4::default(),
        });
        s.cpu_buffer = ib;
    }

    let p = CustomRenderState::ports();
    let src = <CustomRenderState as shame_gui::graph::AppState>::source_ports();

    // ── DAG node: combine state + ViewportRect → render parameters ────
    let hue = p.hue;
    let sat = p.saturation;
    let vp = p.viewport;
    let fb = src.framebuffer_size;
    let constant = p.constant;
    app.graph_mut().add_node(
        {
            move |gref: &mut shame_gui::graph::DagStructRef<CustomRenderState>,
                  _gpu: Option<&sm::Gpu>| {
                let hue = *hue.read(gref);
                let sat = *sat.read(gref);
                let rect = vp.read(gref).rect;
                let fb = *fb.read(gref);
                let tint = hue_sat_to_rgb(hue, sat);
                constant.write(
                    gref,
                    CustomPushConstant {
                        ndc_rect: rect.to_ndc(fb),
                        tint,
                    },
                );
            }
        },
        (hue, sat, vp, fb),
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

    // ── GUI: controls (left) | rendered area (right) ────────────────────
    let controls = CustomRenderState::into_viewport_nodes();
    let vp_widget = WidgetNode::new(p.viewport, ViewportRectData::default());
    let tree = ViewportTree::new(ViewportNode::split(
        SplitDir::Vertical,
        0.4,
        ViewportNode::container(controls),
        ViewportNode::container(vec![("viewport".into(), ViewportNode::Widget(vp_widget))]),
    ));
    app.add_gui(Gui::new(tree));

    app.run(TextSystem::new());
}
