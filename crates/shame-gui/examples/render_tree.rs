//! Render-tree batched rendering: the top map's elements each provide a push
//! constant (a tint color), and the leaves below them provide instance data.
//! One indirect dispatch per top-level element — its constant, drawing the
//! instances of every leaf in its subtree. The descent crosses a plain
//! struct field ([`FieldPath`]) before the leaf map — the struct level
//! between maps is transparent to the batching.
//!
//! Run with: cargo run -p shame-gui --example render_tree

use std::collections::HashMap;

use shame_gui::DagStruct;
use shame_gui::Vec2;
use shame_gui::Vec2u;
use shame_gui::Vec4;
use shame_gui::app::App;
use shame_gui::color::Color;
use shame_gui::graph::{FieldPath, LeafMarker, MapPath};
use shame_gui::material::{Draw, GpuInstanceBuffer, InstanceBuffer, Material, PipelineData};
use shame_gui::rect::Rect;
use shame_gui::shader::RectInstance;
use shame_gui::sm;
use shame_gui::state;
use shame_gui::text::TextSystem;
use sm::prelude::*;

/// Push constant: framebuffer size (canvas-injected, first field) + a tint
/// color — every top-level element tints its subtree with its own color.
#[derive(shame_gui::GpuStruct, Clone, Copy, Default)]
#[repr(C)]
struct GroupParams {
    fb_size: Vec2u,
    color: Vec4,
}

impl shame_gui::graph::PortValue for GroupParams {}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
struct GroupMaterial;

impl Material for GroupMaterial {
    type Instance = RectInstance;
    type PushConstant = GroupParams;
    const HAS_FB_PUSH_CONSTANT: bool = true;

    fn build(&self, gpu: &sm::Gpu) -> PipelineData {
        let pipeline = {
            let mut encoder: sm::PipelineEncoder<sm::pipeline_kind::Render> =
                gpu.create_pipeline_encoder(Default::default()).unwrap();
            let mut drawcall = encoder.new_render_pipeline(sm::Indexing::BufferU32);

            let pc = Self::push_constant(drawcall.push_constants);
            let fb = pc.fb_size.to_f32();
            let color = pc.color;

            let instances = Self::get_instance(&mut drawcall.bind_groups.at(0));
            let instance = instances.index(drawcall.vertices.instance_index);
            let ppos = instance.rect.pos;
            let psize = instance.rect.size;
            let z = instance.z;

            let (pos, size) = shame_gui::dual::pixel_rect_to_ndc_gpu(
                sm::vec!(ppos.x, ppos.y),
                sm::vec!(psize.x, psize.y),
                fb,
            );

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
            let fill: sm::vec<f32, x4> = frag.fill(color);
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

/// One top-level element: a push constant + a struct field holding the
/// instance-leaf map (the tree crosses the field with `FieldPath`).
#[derive(Clone, Default, DagStruct)]
struct Group {
    constant: GroupParams,
    child_set: ChildSet,
}

/// The struct field between `Group` and its leaves — a plain field level in
/// the render-tree path.
#[derive(Clone, Default, DagStruct)]
struct ChildSet {
    children: HashMap<u32, Element>,
}

/// One leaf: user-filled CPU instances + a framework-written GPU handle
/// (RAII — removing the leaf releases its slice automatically).
#[derive(Clone, Default, DagStruct)]
struct Element {
    cpu_buffer: InstanceBuffer<RectInstance>,
    gpu_buffer: Option<GpuInstanceBuffer>,
}

#[state]
#[derive(Clone, Default, DagStruct)]
struct TreeState {
    groups: HashMap<u32, Group>,
    material: GroupMaterial,
}

fn main() {
    let mut app = App::<TreeState>::new("Render tree");

    {
        let s = app.state_mut();
        let colors = [
            Color::rgb(1.0, 0.2, 0.2),
            Color::rgb(0.2, 1.0, 0.2),
            Color::rgb(0.2, 0.4, 1.0),
        ];
        for (i, c) in colors.iter().enumerate() {
            let mut group = Group::default();
            group.constant = GroupParams {
                fb_size: Vec2u::new(0, 0), // patched by the canvas each frame
                color: c.to_linear(),
            };
            // Two leaves per group: one wide + one small instance.
            let mut leaf = Element::default();
            let mut ib = InstanceBuffer::<RectInstance>::new();
            ib.push(&RectInstance {
                rect: Rect::new(
                    Vec2::new(40.0 + i as f32 * 360.0, 60.0),
                    Vec2::new(320.0, 300.0),
                ),
                color: Vec4::new(1.0, 1.0, 1.0, 1.0),
                z: 0.1,
            });
            ib.push(&RectInstance {
                rect: Rect::new(
                    Vec2::new(40.0 + i as f32 * 360.0, 400.0),
                    Vec2::new(320.0, 120.0),
                ),
                color: Vec4::new(1.0, 1.0, 1.0, 1.0),
                z: 0.1,
            });
            leaf.cpu_buffer = ib;
            group.child_set.children.insert(0, leaf);
            s.groups.insert(i as u32, group);
        }
    }

    let p = TreeState::ports();
    let g = Group::ports();
    let cs = ChildSet::ports();
    let el = Element::ports();
    // One batch per group: the group's constant + the slots of its leaves.
    // Nest `FieldPath` to cross plain struct fields and `MapPath` per map
    // level to go deeper.
    app.register_render_tree_batched(
        p.material,
        p.groups,
        g.constant,
        FieldPath {
            field: g.child_set,
            next: MapPath {
                map: cs.children,
                next: LeafMarker::new(),
            },
        },
        el.cpu_buffer,
        el.gpu_buffer,
    );

    app.run(TextSystem::new());
}
