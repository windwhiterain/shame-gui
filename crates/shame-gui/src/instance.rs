//! [`GpuStruct`]: the trait bridging CPU structs to their GPU twins.
//!
//! `#[derive(GpuStruct)]` on a `#[repr(C)]` struct generates the GPU twin,
//! WGSL layout constants with compile-time offset checks, serialization,
//! and (for `align(16)` instance structs) a bind group. See the trait doc
//! for the two modes.

use shame_wgpu as sm;

use crate::material::MaterialBindings;
use crate::math::{Vec2, Vec2i, Vec2u, Vec4};

/// A type usable as a field of a `#[derive(GpuStruct)]` struct — and also
/// the target of that derive itself.
///
/// One trait covers everything: GPU twin, WGSL layout, serialization, and
/// bind group construction. `#[derive(GpuStruct)]` on a `#[repr(C)]` struct
/// generates the GPU twin struct, `impl GpuStruct`, `impl CpuLayout`, and
/// compile-time layout checks. `align(16)` enables the instance-level methods
/// (per-field padding in `serialize`, bind group construction); without it,
/// the struct works as a field type only.
///
/// Implemented for primitives (`f32`, `u32`, `Vec2`, `Vec2u`, `Vec2i`,
/// `Vec4`, `[f32; N]`) via macro; field types (`Rect`) derive it.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a valid GPU struct field type",
    note = "`#[derive(GpuStruct)]` field types must implement `GpuStruct` (e.g. f32, u32, Vec2, Vec2u, Vec2i, Vec4, [f32; N], Rect)."
)]
pub trait GpuStruct: bytemuck::Pod + bytemuck::Zeroable + 'static {
    /// The GPU twin type: a shame `GpuType` whose WGSL layout is
    /// wire-compatible with `Self`'s `#[repr(C)]` layout.
    type Gpu: sm::GpuLayout + sm::GpuSized + sm::GpuAligned + sm::GpuStore + sm::NoHandles;
    /// WGSL size of `Gpu` as a struct member.
    const SIZE: u64;
    /// WGSL alignment of `Gpu` as a struct member.
    const ALIGN: u64;

    /// Wire byte size of one instance (includes tail padding for instance
    /// structs; for field types this equals `SIZE`).
    fn wire_size() -> usize;
    /// Appends this instance's wire bytes to `out`. Instance structs pad
    /// between fields and to the wire-size boundary; field types append
    /// raw bytes.
    fn serialize(&self, out: &mut Vec<u8>);
    /// Create the bind group layout + constructor for the instance storage
    /// buffer. Only meaningful for instance structs (with `align(16)`).
    fn make_bindings(gpu: &sm::Gpu) -> MaterialBindings;
}

/// WGSL struct-member offsets for per-field (size, align) pairs, following
/// the WGSL spec's structure-member layout rules
/// (<https://www.w3.org/TR/WGSL/#structure-member-layout>).
pub const fn field_offsets<const N: usize>(sizes: [usize; N], aligns: [usize; N]) -> [usize; N] {
    let mut offsets = [0; N];
    let mut end: usize = 0;
    let mut i: usize = 0;
    while i < N {
        let offset = end.div_ceil(aligns[i]) * aligns[i];
        offsets[i] = offset;
        end = offset + sizes[i];
        i += 1;
    }
    offsets
}

macro_rules! impl_gpu_struct_primitive {
    ($ty: ty, $gpu: ty, $size: literal, $align: literal) => {
        impl GpuStruct for $ty {
            type Gpu = $gpu;
            const SIZE: u64 = $size;
            const ALIGN: u64 = $align;
            fn wire_size() -> usize {
                $size
            }
            fn serialize(&self, out: &mut Vec<u8>) {
                out.extend_from_slice(bytemuck::bytes_of(self));
            }
            fn make_bindings(_gpu: &sm::Gpu) -> MaterialBindings {
                unreachable!("field type, not an instance struct")
            }
        }
    };
}

impl_gpu_struct_primitive!(f32, sm::f32x1, 4, 4);
impl_gpu_struct_primitive!(u32, sm::u32x1, 4, 4);
impl_gpu_struct_primitive!(Vec2, sm::f32x2, 8, 8);
impl_gpu_struct_primitive!(Vec2u, sm::u32x2, 8, 8);
impl_gpu_struct_primitive!(Vec2i, sm::i32x2, 8, 8);
impl_gpu_struct_primitive!(Vec4, sm::f32x4, 16, 16);
impl_gpu_struct_primitive!([f32; 2], sm::f32x2, 8, 8);
impl_gpu_struct_primitive!([f32; 4], sm::f32x4, 16, 16);
impl_gpu_struct_primitive!([f32; 16], sm::f32x4x4, 64, 16);

// () as a zero-sized type. Since shame supports ZST (GpuLayout::IS_ZST),
// the GPU twin is also `()` — no dummy types needed. Works as both
// PushConstant and Instance (for materials with no instance data).
impl GpuStruct for () {
    type Gpu = ();
    const SIZE: u64 = 0;
    const ALIGN: u64 = 1;
    fn wire_size() -> usize {
        0
    }
    fn serialize(&self, _out: &mut Vec<u8>) {}
    fn make_bindings(gpu: &sm::Gpu) -> MaterialBindings {
        // ZST: empty bind group — no storage buffer needed, but the shader
        // still expects a bind group at slot 0.
        let layout = gpu.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("() instance (ZST)"),
            entries: &[],
        });
        MaterialBindings {
            layout,
            make_bind_group: |device, layout, _buffer| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("() instance (ZST)"),
                    layout,
                    entries: &[],
                })
            },
        }
    }
}
