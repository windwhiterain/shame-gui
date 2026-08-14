//! The filled-rectangle material: [`RectMaterial`] (shader) and the
//! [`RectEntry`] source data.

use shame_wgpu as sm;
use sm::prelude::*;

use crate::material::{Draw, Material, PipelineData};

/// One filled rect. `rect` is in pixel-space (y-down). The vertex shader
/// converts to NDC using `fb_size` from push constants.
#[derive(crate::GpuStruct, Clone, Copy)]
#[repr(C, align(16))]
pub struct RectInstance {
    /// The rect in physical pixels, y-down.
    pub rect: crate::rect::Rect,
    /// The fill color in linear space.
    pub color: crate::Vec4,
    /// Depth layer; smaller z is closer.
    pub z: f32,
}

/// Solid rects with `discard` for alpha < 0.5. No blending.
/// Pixel→NDC conversion is done in the vertex shader via
/// [`ViewportParams`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RectMaterial;

impl Material for RectMaterial {
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

            // Shared pixel→NDC formula (see `dual::pixel_rect_to_ndc_gpu`).
            let (pos, size) = crate::dual::pixel_rect_to_ndc_gpu(
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
            let color_interp: sm::vec<f32, x4> = frag.fill(color);
            sm::discard_if(color_interp.w.lt(crate::shader::ALPHA_DISCARD));
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

/// Source data for one filled rect instance: pixel-space rect (y-down) plus
/// the linear instance color. `color` is already linear — build it with
/// `Color::rgb(...).to_linear()` when authoring.
#[derive(Clone, Copy)]
pub struct RectEntry {
    /// The rect in physical pixels, y-down (converted to NDC per frame).
    pub rect: crate::rect::Rect,
    /// The fill color in linear space.
    pub color: crate::Vec4,
    /// Depth layer; smaller z is closer.
    pub z: f32,
}
