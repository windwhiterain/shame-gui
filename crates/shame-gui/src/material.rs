//! Material traits: [`Material`] (shader + pipeline parameters),
//! [`InstanceBuffer`] (typed CPU-side instance storage), and
//! [`GpuBufferSlot`] (GPU buffer + instance count).
//!
//! Materials are `Eq + Hash + Clone + Default`, so equal parameters
//! deduplicate to one pipeline. The [`shader`](crate::shader) module's
//! `RectMaterial` / `WireframeMaterial` are the reference implementations.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::hash::Hash;
use std::marker::PhantomData;
use std::num::NonZero;
use std::rc::Rc;

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
#[derive(Debug)]
pub struct GpuBufferSlot {
    pub buffer: wgpu::Buffer,
    pub instance_count: u32,
}

/// One render object's slice inside a shared [`InstanceArena`].
#[derive(Debug, Clone, Copy)]
struct ArenaSlice {
    /// Byte offset of this object's instances within the arena buffer.
    offset: u32,
    /// Byte size of the slice (`instance_count * wire_size`).
    byte_size: u32,
    /// Number of instances stored in this slice.
    instance_count: u32,
}

/// A shared, grow-only GPU buffer that holds instances from many render
/// objects of the same material, plus the indirect-draw arguments buffer that
/// turns them into a single `multi_draw_indexed_indirect` dispatch.
///
/// Instances are uploaded by DAG nodes ([`App::register_render_object_batched`](crate::app::App::register_render_object_batched))
/// into per-object slices; the [`Canvas`](crate::canvas::Canvas) builds the
/// args buffer and issues one indirect dispatch per arena. A CPU byte mirror
/// is kept so that a grow (buffer recreation) can re-upload every slice without
/// re-serializing instances — only the changed slice's bytes are ever written.
pub struct InstanceArena {
    /// Shared storage buffer (STORAGE | COPY_DST). Created lazily on the
    /// first non-empty upload; recreated (grow-only) when slices no longer fit.
    buffer: Option<wgpu::Buffer>,
    /// CPU mirror of the buffer contents (for grow re-upload).
    cpu_mirror: Vec<u8>,
    /// Bind group over `buffer`; rebuilt when the buffer is recreated.
    bind_group: Option<wgpu::BindGroup>,
    /// Wire byte size of one instance (for byte-offset → `first_instance`).
    wire_size: usize,
    /// Per-object slices, indexed by the object's slot id (stable across
    /// frames, assigned at registration time).
    slots: Vec<Option<ArenaSlice>>,
    /// Free byte ranges available for reuse (first-fit).
    free_list: Vec<FreeBlock>,
    /// Append cursor used when no free block fits.
    next_offset: u32,
    /// Indirect args buffer (INDIRECT | COPY_DST); grown as needed.
    args_buffer: Option<wgpu::Buffer>,
}

/// A free byte range in the arena's free-list.
#[derive(Debug, Clone, Copy)]
struct FreeBlock {
    offset: u32,
    size: u32,
}

impl InstanceArena {
    pub(crate) fn new(wire_size: usize) -> Self {
        Self {
            buffer: None,
            cpu_mirror: Vec::new(),
            bind_group: None,
            wire_size,
            slots: Vec::new(),
            free_list: Vec::new(),
            next_offset: 0,
            args_buffer: None,
        }
    }

    /// Registers a new object slot and returns its stable id.
    pub(crate) fn add_slot(&mut self) -> usize {
        self.slots.push(None);
        self.slots.len() - 1
    }

    /// Total bytes committed so far (the arena buffer must be at least this
    /// large to hold every allocated slice).
    pub(crate) fn committed_size(&self) -> u64 {
        self.next_offset as u64
    }

    /// Clears a slot's slice (the object became empty).
    pub(crate) fn free_slot(&mut self, slot: usize) {
        if let Some(old) = self.slots[slot].take() {
            self.free(old.offset, old.byte_size);
        }
    }

    /// Returns a slice's byte range to the free-list.
    fn free(&mut self, offset: u32, size: u32) {
        if size == 0 {
            return;
        }
        self.free_list.push(FreeBlock { offset, size });
    }

    /// Allocates a slice of `bytes` (a multiple of wire size) and returns its
    /// byte offset. The caller writes the instance data at that offset.
    fn alloc(&mut self, bytes: u32) -> u32 {
        debug_assert!(bytes > 0, "alloc(0) is invalid; guard empty slices first");
        if let Some(idx) = self.free_list.iter().position(|b| b.size >= bytes) {
            let block = self.free_list.remove(idx);
            let rem = block.size - bytes;
            if rem > 0 {
                self.free_list.push(FreeBlock {
                    offset: block.offset + bytes,
                    size: rem,
                });
            }
            return block.offset;
        }
        let offset = self.next_offset;
        self.next_offset += bytes;
        offset
    }

    /// Replaces a slot's slice: frees the old one, allocates a fresh slice for
    /// `bytes`, and records it. Returns the new byte offset.
    pub(crate) fn alloc_for_slot(&mut self, slot: usize, instance_count: u32, bytes: u32) -> u32 {
        self.free_slot(slot);
        let offset = self.alloc(bytes);
        let byte_size = instance_count as usize * self.wire_size;
        self.slots[slot] = Some(ArenaSlice {
            offset,
            byte_size: byte_size as u32,
            instance_count,
        });
        offset
    }

    /// Ensures the storage buffer has at least `needed` bytes, recreating it
    /// (grow-only) when necessary and re-uploading the CPU mirror.
    pub(crate) fn ensure_capacity(&mut self, gpu: &sm::Gpu, needed: u64) {
        if self.buffer.as_ref().is_none_or(|b| b.size() < needed) {
            let size = needed.max(1);
            self.buffer = Some(gpu.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instance arena"),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.cpu_mirror.resize(size as usize, 0);
            gpu.queue()
                .write_buffer(self.buffer.as_ref().unwrap(), 0, &self.cpu_mirror);
            self.bind_group = None; // buffer changed → rebuild bind group
        }
    }

    /// Writes `bytes` for a slice into both the CPU mirror and the GPU buffer
    /// at `offset`.
    pub(crate) fn upload(&mut self, gpu: &sm::Gpu, offset: u32, bytes: &[u8]) {
        if let Some(buffer) = self.buffer.as_ref() {
            gpu.queue().write_buffer(buffer, offset as u64, bytes);
        }
        let start = offset as usize;
        let end = start + bytes.len();
        if self.cpu_mirror.len() < end {
            self.cpu_mirror.resize(end, 0);
        }
        self.cpu_mirror[start..end].copy_from_slice(bytes);
    }

    /// The storage buffer, if it has been created.
    #[allow(dead_code)]
    pub(crate) fn buffer(&self) -> Option<&wgpu::Buffer> {
        self.buffer.as_ref()
    }

    /// Builds (or returns the cached) bind group over the arena buffer using
    /// the material's cached bindings.
    pub(crate) fn ensure_bind_group(
        &mut self,
        gpu: &sm::Gpu,
        bindings: &MaterialBindings,
    ) -> Option<&wgpu::BindGroup> {
        if self.bind_group.is_some() {
            return self.bind_group.as_ref();
        }
        let buffer = self.buffer.as_ref()?;
        let binding = wgpu::BufferBinding {
            buffer,
            offset: 0,
            size: Some(NonZero::new(buffer.size()).unwrap()),
        };
        let bind_group = (bindings.make_bind_group)(gpu, &bindings.layout, &binding);
        self.bind_group = Some(bind_group);
        self.bind_group.as_ref()
    }

    /// The cached bind group.
    pub(crate) fn bind_group(&self) -> Option<&wgpu::BindGroup> {
        self.bind_group.as_ref()
    }

    /// Builds the indirect draw args for the current slices, one per live
    /// slice. `index_count` is the material's index buffer length.
    pub(crate) fn args(&self, index_count: u32) -> Vec<wgpu::util::DrawIndexedIndirectArgs> {
        let wire_size = self.wire_size.max(1) as u32;
        self.slots
            .iter()
            .filter_map(|s| {
                s.map(|s| wgpu::util::DrawIndexedIndirectArgs {
                    index_count,
                    instance_count: s.instance_count,
                    first_index: 0,
                    base_vertex: 0,
                    first_instance: s.offset / wire_size,
                })
            })
            .collect()
    }

    /// Uploads the args buffer (recreating it if too small) and returns it.
    pub(crate) fn write_args(
        &mut self,
        gpu: &sm::Gpu,
        args: &[wgpu::util::DrawIndexedIndirectArgs],
    ) -> Option<&wgpu::Buffer> {
        if args.is_empty() {
            return None;
        }
        let bytes: Vec<u8> = args.iter().flat_map(|a| a.as_bytes().to_vec()).collect();
        if self
            .args_buffer
            .as_ref()
            .is_none_or(|b| b.size() < bytes.len() as u64)
        {
            self.args_buffer = Some(gpu.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instance arena args"),
                size: bytes.len().max(1) as u64,
                usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        gpu.queue()
            .write_buffer(self.args_buffer.as_ref().unwrap(), 0, &bytes);
        self.args_buffer.as_ref()
    }
}

/// A handle to an element's instance data in the shared batched GPU buffer.
///
/// From the user's perspective this *is* the element's GPU buffer: it is
/// written by [`App::register_map_render_objects_batched`](crate::app::App::register_map_render_objects_batched)
/// after the CPU [`InstanceBuffer`] is uploaded, and it releases its GPU
/// storage automatically when the element is dropped (a key is removed from
/// the map). The arena/slice bookkeeping behind it is an implementation
/// detail.
pub struct GpuInstanceBuffer {
    inner: Rc<GpuBufferGuard>,
}

/// Owning guard for one element's region in a shared [`InstanceArena`].
/// Dropping the last clone releases the element's slice.
struct GpuBufferGuard {
    arena: Rc<RefCell<InstanceArena>>,
    slot: usize,
}

impl Drop for GpuBufferGuard {
    fn drop(&mut self) {
        self.arena.borrow_mut().free_slot(self.slot);
    }
}

impl Clone for GpuInstanceBuffer {
    /// Clones the handle (shares the underlying GPU buffer), rather than
    /// duplicating ownership — so an element clone during map processing does
    /// not release the buffer early.
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl PartialEq for GpuInstanceBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.inner.slot == other.inner.slot
    }
}
impl Eq for GpuInstanceBuffer {}

impl GpuInstanceBuffer {
    pub(crate) fn new(arena: Rc<RefCell<InstanceArena>>, slot: usize) -> Self {
        Self {
            inner: Rc::new(GpuBufferGuard { arena, slot }),
        }
    }

    pub(crate) fn slot(&self) -> usize {
        self.inner.slot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_alloc_and_args() {
        // wire_size = 16 (e.g. a 16-byte instance struct).
        let mut arena = InstanceArena::new(16);
        let s0 = arena.add_slot();
        let s1 = arena.add_slot();

        // s0: 3 instances (48 bytes) at offset 0.
        let off0 = arena.alloc_for_slot(s0, 3, 48);
        assert_eq!(off0, 0);
        // s1: 1 instance (16 bytes) at offset 48.
        let off1 = arena.alloc_for_slot(s1, 1, 16);
        assert_eq!(off1, 48);
        assert_eq!(arena.committed_size(), 64);

        let args = arena.args(6);
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].first_instance, 0);
        assert_eq!(args[0].instance_count, 3);
        assert_eq!(args[1].first_instance, 3); // 48 bytes / 16
        assert_eq!(args[1].instance_count, 1);
    }

    #[test]
    fn arena_free_reuses_space() {
        let mut arena = InstanceArena::new(16);
        let s0 = arena.add_slot();
        let s1 = arena.add_slot();

        arena.alloc_for_slot(s0, 4, 64); // 64 bytes
        arena.alloc_for_slot(s1, 2, 32); // 32 bytes, offset 64
        assert_eq!(arena.committed_size(), 96);

        // Freeing s0 puts a 64-byte block back; a new 32-byte alloc reuses it.
        arena.free_slot(s0);
        let s2 = arena.add_slot();
        let off2 = arena.alloc_for_slot(s2, 2, 32);
        assert_eq!(off2, 0);
        assert_eq!(arena.committed_size(), 96); // no growth

        let args = arena.args(6);
        assert_eq!(args.len(), 2); // s0 is gone; s1 + s2 remain
    }
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
