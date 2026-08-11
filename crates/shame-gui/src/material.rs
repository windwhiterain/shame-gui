//! Material traits: [`Material`] (shader + pipeline parameters),
//! [`InstanceBuffer`] (typed CPU-side instance storage), and
//! [`GpuBufferSlot`] (GPU buffer + instance count).
//!
//! Materials are `Eq + Hash + Clone + Default`, so equal parameters
//! deduplicate to one pipeline. The [`shader`](crate::shader) module's
//! `RectMaterial` / `WireframeMaterial` are the reference implementations.

use std::any::{Any, TypeId};
use std::hash::Hash;
use std::marker::PhantomData;

use shame_wgpu as sm;

use crate::instance::GpuStruct;

/// Index-buffer based drawing. The topology (triangle/line list) is baked into
/// the pipeline at build time; only the index buffer is described here.
pub enum Draw {
    /// A primitive draw with the given index buffer.
    Primitive {
        /// The index buffer contents.
        indices: Box<[u32]>,
    },
}

/// Function pointer that creates a bind group. Captures no state — all types
/// are known at compile time from the auto-generated bind group struct.
pub type MakeBindGroupFn = fn(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::BufferBinding,
) -> wgpu::BindGroup;

/// Bind-group layout + constructor. Users never touch this directly; the
/// canvas constructs it via `Material::make_bindings()`.
pub struct MaterialBindings {
    /// The bind group layout for the instance storage buffer.
    pub layout: wgpu::BindGroupLayout,
    /// Creates a bind group for one buffer binding.
    pub make_bind_group: MakeBindGroupFn,
}

/// What `Material::build()` returns — just the GPU pipeline and draw info.
pub struct PipelineData {
    /// The compiled render pipeline.
    pub pipeline: wgpu::RenderPipeline,
    /// How to issue the draw (index buffer + topology).
    pub draw: Draw,
    /// Optional blend state for the color attachment.
    pub blend: Option<wgpu::BlendState>,
}

/// A material: parameters + the shame EDSL that builds the pipeline.
///
/// `build` receives the gpu, so the material struct itself holds parameters
/// only — which is why `Eq + Hash` is well-defined and the canvas can dedup
/// registrations by parameter equality.
pub trait Material: 'static + Eq + Hash + Clone + Default {
    /// The per-instance data type (see [`GpuStruct`]).
    type Instance: GpuStruct;
    /// The push constant data type for this material. Passed via
    /// `set_immediates` before each draw call. Set to `()` for materials
    /// that don't use push constants. Must implement [`GpuStruct`] so the
    /// shader EDSL can retrieve it with the correct type via
    /// [`Self::push_constant`].
    type PushConstant: GpuStruct + Default;
    /// Whether the Canvas should inject `fb_size: Vec2u` at offset 0 of
    /// the push constant. True for materials that do pixel→NDC conversion
    /// in the vertex shader.
    const HAS_FB_PUSH_CONSTANT: bool = false;
    /// Build the GPU pipeline from the shame EDSL. Returns only pipeline +
    /// draw info — bindings are constructed by the canvas via
    /// `GpuStruct::make_bindings`, so `build` stays pure shader code.
    fn build(&self, gpu: &sm::Gpu) -> PipelineData;

    /// Helper: retrieve push constants with the GPU type that matches
    /// [`Self::PushConstant`]. Uses [`<Self::PushConstant as GpuStruct>::Gpu`].
    ///
    /// Materials that set `PushConstant = ()` should not call this method.
    ///
    /// ```ignore
    /// let params = Self::push_constant(drawcall.push_constants);
    /// ```
    fn push_constant(pc: sm::PushConstants<'_>) -> <Self::PushConstant as GpuStruct>::Gpu
    where
        <Self::PushConstant as GpuStruct>::Gpu: sm::NoAtomics + sm::NoBools,
    {
        pc.get()
    }

    /// Helper: bind the instance storage buffer at slot 0 of a bind group,
    /// returning the typed buffer. Uses [`<Self::Instance as GpuStruct>::Gpu`].
    ///
    /// ```ignore
    /// let instances = Self::get_instance(&mut drawcall.bind_groups.at(0));
    /// let instance = instances.index(drawcall.vertices.instance_index);
    /// ```
    fn get_instance(
        bindings: &mut sm::BindingIter<'_>,
    ) -> sm::Buffer<sm::Array<sm::Struct<<Self::Instance as GpuStruct>::Gpu>>, sm::mem::Storage>
    where
        <Self::Instance as GpuStruct>::Gpu: sm::NoAtomics + sm::NoBools + sm::SizedFields,
    {
        bindings.at(0)
    }
}

/// Typed CPU-side instance buffer. Wraps serialized bytes with type
/// information so push operations are compile-time checked against the
/// material's instance type.
///
/// [`App::register_render_object`](crate::app::App::register_render_object)
/// takes `Port<InstanceBuffer<M::Instance>>` so the connection between
/// material and instance data is type-safe.
pub struct InstanceBuffer<I: GpuStruct> {
    data: Vec<u8>,
    instance_count: u32,
    _marker: PhantomData<I>,
}

impl<I: GpuStruct> InstanceBuffer<I> {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            instance_count: 0,
            _marker: PhantomData,
        }
    }

    pub fn push(&mut self, instance: &I) {
        instance.serialize(&mut self.data);
        self.instance_count += 1;
    }

    pub fn extend(&mut self, instances: &[I]) {
        for inst in instances {
            inst.serialize(&mut self.data);
        }
        self.instance_count += instances.len() as u32;
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn instance_count(&self) -> u32 {
        self.instance_count
    }

    pub fn is_empty(&self) -> bool {
        self.instance_count == 0
    }

    pub fn clear(&mut self) {
        self.data.clear();
        self.instance_count = 0;
    }
}

impl<I: GpuStruct> Clone for InstanceBuffer<I> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            instance_count: self.instance_count,
            _marker: PhantomData,
        }
    }
}

impl<I: GpuStruct> Default for InstanceBuffer<I> {
    fn default() -> Self {
        Self::new()
    }
}

/// GPU buffer + instance count produced by a DAG upload node and read
/// by [`Canvas`](crate::canvas::Canvas) for drawing.
pub struct GpuBufferSlot {
    pub buffer: wgpu::Buffer,
    pub instance_count: u32,
}

/// Type-erased, hashable registry key for one material value.
///
/// `PartialEq`/`Hash` are delegated to the concrete material type through
/// monomorphized fn pointers (a trait-object supertrait would not be
/// dyn-compatible, since `PartialEq` uses `Self` as a type parameter).
pub(crate) struct ErasedKey {
    type_id: TypeId,
    value: Box<dyn Any>,
    eq: fn(&dyn Any, &dyn Any) -> bool,
    hash: fn(&dyn Any, &mut dyn std::hash::Hasher),
}

impl ErasedKey {
    pub(crate) fn new<M: Material>(value: M) -> Self {
        Self {
            type_id: TypeId::of::<M>(),
            value: Box::new(value),
            eq: |a, b| a.downcast_ref::<M>().unwrap() == b.downcast_ref::<M>().unwrap(),
            hash: |value, mut state| value.downcast_ref::<M>().unwrap().hash(&mut state),
        }
    }

    /// The material value this key holds (typed accessor for the dedup miss
    /// path, where the caller still needs `&M` to build the pipeline).
    pub(crate) fn material<M: Material>(&self) -> &M {
        self.value.downcast_ref::<M>().unwrap()
    }
}

impl PartialEq for ErasedKey {
    fn eq(&self, other: &Self) -> bool {
        self.type_id == other.type_id && (self.eq)(self.value.as_ref(), other.value.as_ref())
    }
}
impl Eq for ErasedKey {}
impl std::hash::Hash for ErasedKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.hash)(self.value.as_ref(), state)
    }
}

/// Typed handle returned by `Canvas::register_material`.
#[derive(Clone, Copy)]
pub(crate) struct MaterialHandle<M: Material> {
    pub(crate) index: usize,
    _phantom: PhantomData<M>,
}

impl<M: Material> MaterialHandle<M> {
    pub(crate) fn new(index: usize) -> Self {
        Self {
            index,
            _phantom: PhantomData,
        }
    }
}

impl<M: Material> PartialEq for MaterialHandle<M> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<M: Material> Eq for MaterialHandle<M> {}
impl<M: Material> std::fmt::Debug for MaterialHandle<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaterialHandle")
            .field("index", &self.index)
            .finish()
    }
}
