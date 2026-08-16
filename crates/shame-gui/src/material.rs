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
    /// First instance index whose bytes may be stale on the GPU. `push`/
    /// `extend` move it forward to the first not-yet-uploaded append; `clear`
    /// resets it to 0 (a rebuilt buffer is entirely dirty). The upload node
    /// uploads only `[dirty_from..]` and clears the watermark by writing the
    /// buffer back (`clear_dirty`), so a cell that only grows between edits
    /// re-uploads its append tail instead of its whole content.
    dirty_from: u32,
    _marker: PhantomData<I>,
}

impl<I: GpuStruct> InstanceBuffer<I> {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            instance_count: 0,
            dirty_from: 0,
            _marker: PhantomData,
        }
    }

    pub fn push(&mut self, instance: &I) {
        self.dirty_from = self.dirty_from.min(self.instance_count);
        instance.serialize(&mut self.data);
        self.instance_count += 1;
    }

    pub fn extend(&mut self, instances: &[I]) {
        self.dirty_from = self.dirty_from.min(self.instance_count);
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
        self.dirty_from = 0;
    }

    /// First instance index whose bytes may be stale on the GPU
    /// (0 = the whole buffer is dirty).
    pub fn dirty_from(&self) -> u32 {
        self.dirty_from
    }

    /// Marks the whole buffer clean — called by the upload node after the
    /// dirty range has been uploaded.
    pub fn clear_dirty(&mut self) {
        self.dirty_from = self.instance_count;
    }
}

impl<I: GpuStruct> Clone for InstanceBuffer<I> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            instance_count: self.instance_count,
            dirty_from: self.dirty_from,
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

/// A shared GPU buffer that holds instances from many render objects of the
/// same material, plus the indirect-draw arguments buffer that turns them
/// into a single `multi_draw_indexed_indirect` dispatch.
///
/// Instances are uploaded by DAG nodes ([`App::register_render_object_batched`](crate::app::App::register_render_object_batched))
/// into per-object slices; the [`Canvas`](crate::canvas::Canvas) builds the
/// args buffer and issues one indirect dispatch per arena. A CPU byte mirror
/// is kept so that a grow (buffer recreation) can re-upload every slice without
/// re-serializing instances — only the changed slice's bytes are ever written.
///
/// Allocation reuses freed space: freed slices go to a coalescing free list
/// (adjacent blocks merge, so fragmented garbage recombines into large
/// reusable blocks), and a slice freed at the very end of the arena simply
/// rewinds the append cursor — a cell that grows by repeated re-uploads never
/// accumulates dead space. The underlying GPU buffer itself is grow-only and
/// recreated in chunks (see [`arena_grow_size`]), so recreation — which
/// re-uploads the whole mirror — happens O(log n) times over a session
/// instead of on every edit.
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
    /// Free byte ranges available for reuse, sorted by offset (first-fit).
    /// Never contains adjacent blocks — `free` merges neighbors.
    free_list: Vec<(u32, u32)>,
    /// Append cursor used when no free block fits; rewinds when a tail slice
    /// is freed.
    next_offset: u32,
    /// Indirect args buffer (INDIRECT | COPY_DST); grown as needed.
    args_buffer: Option<wgpu::Buffer>,
}

/// Minimum size for the arena's storage buffer. The buffer only grows, so a
/// session that reaches tens of MB should not start from byte-sized buffers.
const MIN_ARENA_BYTES: u64 = 1024 * 1024;

/// Size to grow the arena storage buffer to, given its current size and the
/// bytes actually needed: at least 1.5× the current size (so recreation is
/// amortized) and at least [`MIN_ARENA_BYTES`].
fn arena_grow_size(current: u64, needed: u64) -> u64 {
    needed.max(1).max(MIN_ARENA_BYTES).max(current * 3 / 2)
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
    /// large to hold every allocated slice). May shrink when tail slices are
    /// freed — see [`Self::free`].
    pub(crate) fn committed_size(&self) -> u64 {
        self.next_offset as u64
    }

    /// Clears a slot's slice (the object became empty).
    pub(crate) fn free_slot(&mut self, slot: usize) {
        if let Some(old) = self.slots[slot].take() {
            self.free(old.offset, old.byte_size);
        }
    }

    /// Returns a slice's byte range to the free-list, merging with adjacent
    /// free blocks so fragmented garbage recombines into reusable blocks.
    ///
    /// A block at the very end of the arena is not added at all: the append
    /// cursor rewinds past it (and past any free block that becomes the new
    /// tail), so repeated re-uploads of a growing cell leave no dead space.
    fn free(&mut self, offset: u32, size: u32) {
        if size == 0 {
            return;
        }
        // Tail block → the arena shrinks instead of keeping a free block.
        if offset + size == self.next_offset {
            self.next_offset = offset;
            // The last free block may now be the tail too — pop it as well.
            if let Some(&(off, sz)) = self.free_list.last() {
                if off + sz == self.next_offset {
                    self.next_offset = off;
                    self.free_list.pop();
                }
            }
            return;
        }
        // Insert sorted by offset, merging with either neighbor. Slices are
        // freed exactly once and blocks never overlap, so exact adjacency is
        // the only case to handle.
        let mut idx = self.free_list.partition_point(|&(off, _)| off < offset);
        let mut off = offset;
        let mut size = size;
        if idx > 0 {
            let (left_off, left_sz) = self.free_list[idx - 1];
            if left_off + left_sz == offset {
                off = left_off;
                size += left_sz;
                self.free_list.remove(idx - 1);
                idx -= 1; // the right neighbor (if any) moved into `idx`
            }
        }
        if idx < self.free_list.len() {
            let (right_off, right_sz) = self.free_list[idx];
            if off + size == right_off {
                size += right_sz;
                self.free_list.remove(idx);
            }
        }
        self.free_list.insert(idx, (off, size));
    }

    /// Allocates a slice of `bytes` (a multiple of wire size) and returns its
    /// byte offset. The caller writes the instance data at that offset.
    fn alloc(&mut self, bytes: u32) -> u32 {
        debug_assert!(bytes > 0, "alloc(0) is invalid; guard empty slices first");
        // First-fit scan. The free list stays short (coalescing keeps it from
        // fragmenting into many tiny blocks), so a linear scan is cheap.
        if let Some(idx) = self.free_list.iter().position(|&(_, sz)| sz >= bytes) {
            let (offset, size) = self.free_list.remove(idx);
            let rem = size - bytes;
            if rem > 0 {
                self.free_list.insert(idx, (offset + bytes, rem));
            }
            return offset;
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
        let current = self.buffer.as_ref().map(|b| b.size()).unwrap_or(0);
        if current < needed {
            // Recreate in chunks (1.5×, never below the minimum) so
            // recreation — which re-uploads the whole mirror — is amortized
            // over many small uploads instead of happening on every edit.
            // Only the committed bytes are re-uploaded.
            let size = arena_grow_size(current, needed);
            self.buffer = Some(gpu.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instance arena"),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.cpu_mirror.resize(size as usize, 0);
            let committed = self.next_offset as usize;
            if committed > 0 {
                gpu.queue().write_buffer(
                    self.buffer.as_ref().unwrap(),
                    0,
                    &self.cpu_mirror[..committed],
                );
            }
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

    /// The first instance index that must be uploaded to make `slot`'s slice
    /// match `cpu`'s content: the buffer's `dirty_from` watermark when the
    /// slice stayed at its previous offset (only the app-changed tail is
    /// stale), 0 when it moved or the slot is fresh (the head at a new
    /// offset would be stale), clamped to the instance count.
    fn dirty_upload_start(
        prev_offset: Option<u32>,
        offset: u32,
        dirty_from: u32,
        count: u32,
    ) -> u32 {
        match prev_offset {
            Some(p) if p == offset => dirty_from,
            _ => 0,
        }
        .min(count)
    }

    /// Syncs `slot`'s slice with `cpu`'s content: uploads only the dirty tail
    /// (from the buffer's [`InstanceBuffer::dirty_from`] watermark) when the
    /// slice stayed at its previous offset — the common re-upload of a cell
    /// that only grew — and the full buffer when the slice moved, because the
    /// head bytes at a new offset would be stale. Returns the new offset and
    /// whether anything was uploaded.
    pub(crate) fn sync_slice<I: GpuStruct>(
        &mut self,
        gpu: &sm::Gpu,
        slot: usize,
        cpu: &InstanceBuffer<I>,
    ) -> (u32, bool) {
        let count = cpu.instance_count();
        let bytes = cpu.as_bytes();
        let bytes_len = (bytes.len() as u32).max(1);
        let prev = self.slice(slot as u32).map(|s| s.offset);
        let offset = self.alloc_for_slot(slot, count, bytes_len);
        let start = Self::dirty_upload_start(prev, offset, cpu.dirty_from(), count);
        if start < count {
            // Grow the arena buffer *before* writing, so `upload` never
            // targets an offset past the current buffer's end.
            let needed = self.committed_size();
            self.ensure_capacity(gpu, needed);
            let ws = self.wire_size.max(1) as usize;
            self.upload(
                gpu,
                offset + start as u32 * ws as u32,
                &bytes[start as usize * ws..],
            );
            (offset, true)
        } else {
            (offset, false)
        }
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

    /// Number of registered slots (including freed ones). Slots are stable
    /// handles assigned at registration; freed slots are skipped when drawing.
    pub(crate) fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// The slice currently stored at `slot`, if any (freed slots return `None`).
    fn slice(&self, slot: u32) -> Option<&ArenaSlice> {
        self.slots.get(slot as usize).and_then(|s| s.as_ref())
    }

    /// Indirect draw args for the given slots, in order (freed slots skipped).
    pub(crate) fn args_for(
        &self,
        index_count: u32,
        slots: &[u32],
    ) -> Vec<wgpu::util::DrawIndexedIndirectArgs> {
        let wire_size = self.wire_size.max(1) as u32;
        slots
            .iter()
            .filter_map(|&slot| {
                self.slice(slot)
                    .map(|s| wgpu::util::DrawIndexedIndirectArgs {
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

        let args = arena.args_for(6, &[0, 1]);
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

        let args = arena.args_for(6, &[0, 1, 2]);
        assert_eq!(args.len(), 2); // s0 is gone; s1 + s2 remain
    }

    #[test]
    fn arena_growing_slot_reuses_tail_in_place() {
        let mut arena = InstanceArena::new(16);
        let s0 = arena.add_slot();
        arena.alloc_for_slot(s0, 3, 48); // 0..48, committed 48
        assert_eq!(arena.committed_size(), 48);
        // Re-uploading a *larger* slice into the tail rewinds the cursor and
        // reuses the space in place: no dead space, no arena growth. This is
        // the common painting pattern (a cell only grows between edits).
        arena.alloc_for_slot(s0, 6, 96);
        assert_eq!(arena.committed_size(), 96);
        let args = arena.args_for(6, &[0]);
        assert_eq!(args[0].first_instance, 0);
        assert_eq!(args[0].instance_count, 6);
    }

    #[test]
    fn arena_tail_free_rewinds_cursor() {
        let mut arena = InstanceArena::new(16);
        let s0 = arena.add_slot();
        let s1 = arena.add_slot();
        arena.alloc_for_slot(s0, 3, 48); // 0..48
        arena.alloc_for_slot(s1, 1, 16); // 48..64
        assert_eq!(arena.committed_size(), 64);
        // Freeing the tail slice shrinks the arena; the non-tail slice's free
        // block then becomes the new tail and is popped too.
        arena.free_slot(s1);
        assert_eq!(arena.committed_size(), 48);
        arena.free_slot(s0);
        assert_eq!(arena.committed_size(), 0);
    }

    #[test]
    fn arena_adjacent_frees_merge_and_reuse() {
        let mut arena = InstanceArena::new(16);
        let a = arena.add_slot();
        let b = arena.add_slot();
        arena.alloc_for_slot(a, 2, 32); // 0..32
        arena.alloc_for_slot(b, 2, 32); // 32..64
        arena.alloc_for_slot(a, 4, 64); // old 0..32 freed, new 64..128
        arena.alloc_for_slot(b, 4, 64); // old 32..64 freed → merges with 0..32 → reused
        assert_eq!(arena.committed_size(), 128); // no growth beyond live bytes
        let args = arena.args_for(6, &[0, 1]);
        assert_eq!(args[0].first_instance, 4); // a lives at 64..128
        assert_eq!(args[1].first_instance, 0); // b reused the merged block
    }

    #[test]
    fn arena_grow_size_chunks() {
        assert_eq!(arena_grow_size(0, 100), MIN_ARENA_BYTES);
        assert_eq!(
            arena_grow_size(MIN_ARENA_BYTES, MIN_ARENA_BYTES + 1),
            MIN_ARENA_BYTES * 3 / 2
        );
        // A need beyond 1.5× of the current size wins.
        assert_eq!(
            arena_grow_size(MIN_ARENA_BYTES, MIN_ARENA_BYTES * 3),
            MIN_ARENA_BYTES * 3
        );
    }

    #[derive(Clone, Copy, crate::GpuStruct)]
    #[repr(C)]
    struct Probe {
        x: f32,
        y: f32,
    }

    #[test]
    fn instance_buffer_dirty_watermark_tracks_appends() {
        let mut ib = InstanceBuffer::<Probe>::new();
        assert_eq!(ib.dirty_from(), 0);
        ib.push(&Probe { x: 1.0, y: 2.0 });
        ib.push(&Probe { x: 3.0, y: 4.0 });
        // A fresh buffer is entirely dirty until the upload node clears it.
        assert_eq!(ib.dirty_from(), 0);
        assert_eq!(ib.instance_count(), 2);

        ib.clear_dirty();
        assert_eq!(ib.dirty_from(), 2);

        // Appends after a successful upload dirty only the new tail.
        ib.push(&Probe { x: 5.0, y: 6.0 });
        assert_eq!(ib.dirty_from(), 2);
        ib.extend(&[Probe { x: 7.0, y: 8.0 }, Probe { x: 9.0, y: 10.0 }]);
        assert_eq!(ib.dirty_from(), 2);
        assert_eq!(ib.instance_count(), 5);

        // A rebuild (clear + push) dirties everything again.
        ib.clear();
        assert_eq!(ib.dirty_from(), 0);
        ib.push(&Probe { x: 0.0, y: 0.0 });
        assert_eq!(ib.dirty_from(), 0);
    }

    #[test]
    fn dirty_upload_start_tail_move_and_clamp() {
        // Same offset → upload only the app-dirtied tail.
        assert_eq!(InstanceArena::dirty_upload_start(Some(64), 64, 5, 10), 5);
        // Moved slice → everything is stale, the watermark is ignored.
        assert_eq!(InstanceArena::dirty_upload_start(Some(64), 0, 5, 10), 0);
        // Fresh slot (no previous slice) → full upload.
        assert_eq!(InstanceArena::dirty_upload_start(None, 0, 5, 10), 0);
        // Clamped to the instance count (e.g. the buffer shrank after a move).
        assert_eq!(InstanceArena::dirty_upload_start(Some(0), 0, 12, 10), 10);
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
