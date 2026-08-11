use shame_gui::GpuStruct;
use shame_gui::Vec2;
use shame_gui::Vec2i;
use shame_gui::Vec2u;
use shame_gui::Vec4;
use shame_gui::rect::Rect;

#[derive(shame_gui::GpuStruct, Clone, Copy)]
#[repr(C, align(16))]
pub struct RectInstance {
    pub rect: Rect,
    pub color: Vec4,
    pub z: f32,
}

/// A field-level `GpuStruct` (no `align(16)`): used as a nested field inside
/// an instance struct. Its GPU twin appears as `sm::Struct<NestedFieldGpu>`
/// (shame's nested-struct pattern).
#[derive(shame_gui::GpuStruct, Clone, Copy)]
#[repr(C)]
pub struct NestedField {
    pub a: Vec2,
    pub b: Vec2u,
    pub c: Vec2i,
}

/// An instance struct nesting `NestedField` plus a mix of vector types.
#[derive(shame_gui::GpuStruct, Clone, Copy)]
#[repr(C, align(16))]
pub struct NestedInstance {
    pub inner: NestedField,
    pub color: Vec4,
    pub z: f32,
}

#[test]
fn size_is_wire_size() {
    assert_eq!(<RectInstance as GpuStruct>::wire_size(), 48);
}

#[test]
fn serializes_fields_at_wgsl_offsets_with_zero_padding() {
    let instance = RectInstance {
        rect: Rect::new(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)),
        color: Vec4::new(5.0, 6.0, 7.0, 8.0),
        z: 9.0,
    };
    let mut out = Vec::new();
    instance.serialize(&mut out);
    assert_eq!(out.len(), 48);
    assert_eq!(&out[0..8], bytemuck::bytes_of(&[1.0f32, 2.0]));
    assert_eq!(&out[8..16], bytemuck::bytes_of(&[3.0f32, 4.0]));
    assert_eq!(&out[16..32], bytemuck::bytes_of(&[5.0f32, 6.0, 7.0, 8.0]));
    assert_eq!(&out[32..36], bytemuck::bytes_of(&9.0f32));
    assert!(out[36..48].iter().all(|&byte| byte == 0));
}

#[test]
fn gpu_layout_equals_cpu_layout() {
    use shame_gui::sm;
    let cpu = <RectInstance as sm::CpuLayout>::cpu_layout();
    let gpu = <RectInstanceGpu as sm::GpuLayout>::gpu_layout();
    assert_eq!(cpu, gpu);
}

#[test]
fn serializing_multiple_instances_appends() {
    let first = RectInstance {
        rect: Rect::new(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)),
        color: Vec4::new(5.0, 6.0, 7.0, 8.0),
        z: 9.0,
    };
    let second = RectInstance {
        rect: Rect::new(Vec2::new(10.0, 11.0), Vec2::new(12.0, 13.0)),
        color: Vec4::new(14.0, 15.0, 16.0, 17.0),
        z: 18.0,
    };
    let mut out = Vec::new();
    first.serialize(&mut out);
    second.serialize(&mut out);
    assert_eq!(out.len(), 96);
    assert_eq!(&out[0..8], bytemuck::bytes_of(&[1.0f32, 2.0]));
    assert_eq!(&out[8..16], bytemuck::bytes_of(&[3.0f32, 4.0]));
    assert_eq!(&out[32..36], bytemuck::bytes_of(&9.0f32));
    assert_eq!(&out[48..56], bytemuck::bytes_of(&[10.0f32, 11.0]));
    assert_eq!(&out[56..64], bytemuck::bytes_of(&[12.0f32, 13.0]));
    assert_eq!(&out[80..84], bytemuck::bytes_of(&18.0f32));
    assert!(out[84..96].iter().all(|&byte| byte == 0));
}

/// A nested `GpuStruct` field must serialize at its own WGSL size (24 bytes,
/// align 8 — NOT rounded to 16) and keep the whole instance wire layout
/// consistent: inner@0, color@32 (align 16), z@48, padded to 64.
#[test]
fn nested_gpu_struct_layout_and_serialization() {
    assert_eq!(<NestedField as GpuStruct>::SIZE, 24);
    assert_eq!(<NestedField as GpuStruct>::ALIGN, 8);
    assert_eq!(<NestedInstance as GpuStruct>::wire_size(), 64);

    let instance = NestedInstance {
        inner: NestedField {
            a: Vec2::new(1.0, 2.0),
            b: Vec2u::new(3, 4),
            c: Vec2i::new(5, 6),
        },
        color: Vec4::new(7.0, 8.0, 9.0, 10.0),
        z: 11.0,
    };
    let mut out = Vec::new();
    instance.serialize(&mut out);
    assert_eq!(out.len(), 64);
    assert_eq!(&out[0..8], bytemuck::bytes_of(&[1.0f32, 2.0]));
    assert_eq!(&out[8..16], bytemuck::bytes_of(&[3u32, 4]));
    assert_eq!(&out[16..24], bytemuck::bytes_of(&[5i32, 6]));
    assert_eq!(&out[32..48], bytemuck::bytes_of(&[7.0f32, 8.0, 9.0, 10.0]));
    assert_eq!(&out[48..52], bytemuck::bytes_of(&11.0f32));
    assert!(out[52..64].iter().all(|&byte| byte == 0));
}

#[test]
fn nested_gpu_struct_cpu_layout_matches_gpu_layout() {
    use shame_gui::sm;
    let cpu = <NestedInstance as sm::CpuLayout>::cpu_layout();
    let gpu = <NestedInstanceGpu as sm::GpuLayout>::gpu_layout();
    assert_eq!(cpu, gpu);
    let field_cpu = <NestedField as sm::CpuLayout>::cpu_layout();
    let field_gpu = <NestedFieldGpu as sm::GpuLayout>::gpu_layout();
    assert_eq!(field_cpu, field_gpu);
}

/// The `SIZE`/`ALIGN` consts must agree with the actual shame
/// `GpuLayout` of each field type's GPU twin (guards against drift).
#[test]
fn instance_field_consts_match_gpu_layouts() {
    use shame_gui::sm;
    fn check<T: GpuStruct>() {
        let layout = <T::Gpu as sm::GpuLayout>::gpu_layout();
        assert_eq!(
            T::SIZE,
            layout.byte_size.unwrap(),
            "size of {}",
            std::any::type_name::<T>()
        );
        assert_eq!(
            T::ALIGN,
            layout.byte_align.0,
            "align of {}",
            std::any::type_name::<T>()
        );
    }
    check::<f32>();
    check::<u32>();
    check::<Vec2>();
    check::<Vec2u>();
    check::<Vec2i>();
    check::<Vec4>();
    check::<[f32; 2]>();
    check::<[f32; 4]>();
    check::<[f32; 16]>();
    check::<Rect>();
    check::<NestedField>();
}
