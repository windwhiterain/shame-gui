//! The rect-outline material: [`WireframeMaterial`] (shader).
//! Shares the instance layout with the filled-rect material.

use shame_wgpu as sm;
use sm::prelude::*;

use crate::material::{Draw, Material, PipelineData};
use crate::shader::rect::RectInstance;

/// Rect outlines as line lists. Same instance type as `RectMaterial`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct WireframeMaterial;

impl Material for WireframeMaterial {
    type Instance = RectInstance;
    type PushConstant = crate::shader::ViewportParams;
    const HAS_FB_PUSH_CONSTANT: bool = true;

    fn build(&self, gpu: &sm::Gpu) -> PipelineData {
        let pipeline = {
            let mut encoder: sm::PipelineEncoder<sm::pipeline_kind::Render> =
                gpu.create_pipeline_encoder(Default::default()).unwrap();
            let mut drawcall = encoder.new_render_pipeline(sm::Indexing::BufferU32);

            let vp = Self::push_constant(drawcall.push_constants);
            let fb = vp.fb_size.to_f32();

            let instances = Self::get_instance(&mut drawcall.bind_groups.at(0));
            let instance = instances.index(drawcall.vertices.instance_index);
            let ppos = instance.rect.pos;
            let psize = instance.rect.size;
            let color = instance.color;
            let z = instance.z;

            let pos = sm::vec!(ppos.x, fb.y - ppos.y - psize.y) * 2.0 / fb - 1.0;
            let size = psize * 2.0 / fb;

            let positions: sm::Array<sm::vec<f32, x3>, Size<4>> = [
                pos.extend(z),
                (pos + sm::vec!(size.x, 0.0)).extend(z),
                (pos + size).extend(z),
                (pos + sm::vec!(0.0, size.y)).extend(z),
            ]
            .to_gpu();
            let position = positions.index(drawcall.vertices.index);
            let primitive = drawcall.vertices.assemble(position, sm::Draw::line_list());
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
                indices: Box::new([0, 1, 1, 2, 2, 3, 3, 0]),
            },
            blend: None,
        }
    }
}
