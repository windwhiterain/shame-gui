//! Render-tree batched snapshot: the top map's elements each provide a push
//! constant (a tint color), the leaves below them provide instance data. One
//! indirect dispatch per top-level element. The descent crosses a plain
//! struct field ([`FieldPath`]) before the leaf map.

#![allow(dead_code)]

mod common;

use std::collections::HashMap;

use shame_gui::DagStruct;
use shame_gui::Vec2;
use shame_gui::Vec2u;
use shame_gui::Vec4;
use shame_gui::app::App;
use shame_gui::graph::FieldPath;
use shame_gui::graph::LeafMarker;
use shame_gui::graph::MapPath;
use shame_gui::material::{Draw, GpuInstanceBuffer, InstanceBuffer, Material, PipelineData};
use shame_gui::rect::Rect;
use shame_gui::shader::RectInstance;
use shame_gui::sm;
use shame_gui::state;
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

/// A top-level element: one push constant + a struct field holding the
/// instance-leaf map.
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

/// A leaf: user-filled CPU instances + a framework-written GPU handle.
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

fn render_tree_scene() -> App<TreeState> {
    let mut app = App::<TreeState>::new("render-tree-indirect-test");

    {
        let s = app.state_mut();
        // Three non-overlapping vertical stripes, one group each; the tint
        // color comes from the group's push constant (the instance color is
        // ignored by this material's shader).
        let stripes = [
            (0u32, Vec4::new(1.0, 0.0, 0.0, 1.0), Vec2::new(0.0, 0.0)),
            (1u32, Vec4::new(0.0, 1.0, 0.0, 1.0), Vec2::new(400.0, 0.0)),
            (2u32, Vec4::new(0.0, 0.0, 1.0, 1.0), Vec2::new(800.0, 0.0)),
        ];
        for (i, color, pos) in stripes {
            let mut group = Group::default();
            group.constant = GroupParams {
                fb_size: Vec2u::new(0, 0), // patched by the canvas
                color,
            };
            // Two leaves per group (each one rect) — one group's constant
            // pairs with a group of instances.
            for (j, y) in [0.0f32, 400.0].into_iter().enumerate() {
                let mut ib = InstanceBuffer::<RectInstance>::new();
                ib.push(&RectInstance {
                    rect: Rect::new(Vec2::new(pos.x, y), Vec2::new(400.0, 400.0)),
                    color: Vec4::new(1.0, 1.0, 1.0, 1.0),
                    z: 0.1,
                });
                group.child_set.children.insert(
                    j as u32,
                    Element {
                        cpu_buffer: ib,
                        gpu_buffer: None,
                    },
                );
            }
            s.groups.insert(i, group);
        }
    }

    let p = TreeState::ports();
    let g = Group::ports();
    let cs = ChildSet::ports();
    let el = Element::ports();
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
        None,
    );

    app
}

#[test]
fn snapshot_render_tree_batched_indirect() {
    common::compare_snapshot(
        "render_tree_batched_indirect",
        render_tree_scene(),
        common::test_text_system(),
    );
}

/// Verifies the semantic core of the feature: each group's push constant
/// (tint color) actually reaches its subtree's instances — three vertical
/// stripes, one color per group (sRGB surface, so 1.0 encodes to 255).
#[test]
fn group_constants_tint_their_own_leaves() {
    let img = image::open("tests/snapshots/render_tree_batched_indirect/expected.png")
        .expect("run snapshot_render_tree_batched_indirect first")
        .to_rgba8();
    assert_eq!(img.dimensions(), (1200, 800));
    let px = |x: u32, y: u32| img.get_pixel(x, y).0;
    assert_eq!(px(100, 200), [255, 0, 0, 255], "group 0 tints its top leaf");
    assert_eq!(
        px(100, 600),
        [255, 0, 0, 255],
        "group 0 tints its bottom leaf"
    );
    assert_eq!(px(600, 200), [0, 255, 0, 255], "group 1 tints its top leaf");
    assert_eq!(
        px(600, 600),
        [0, 255, 0, 255],
        "group 1 tints its bottom leaf"
    );
    assert_eq!(
        px(1100, 200),
        [0, 0, 255, 255],
        "group 2 tints its top leaf"
    );
    assert_eq!(
        px(1100, 600),
        [0, 0, 255, 255],
        "group 2 tints its bottom leaf"
    );
}
