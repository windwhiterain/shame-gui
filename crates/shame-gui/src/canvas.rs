use std::any::TypeId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::num::NonZero;
use std::rc::Rc;
use std::sync::Arc;

use wgpu::util::DeviceExt as _;
use wgpu::{BufferUsages, LoadOp::*, StoreOp::*};

use shame_wgpu as sm;

use crate::gpu::MAX_IMMEDIATE_BYTES;
use crate::graph::{DagStructRef, Port};
use crate::instance::GpuStruct;
use crate::material::{
    Draw, ErasedKey, GpuBufferSlot, InstanceArena, Material, MaterialBindings, MaterialHandle,
};

/// Panics when a push constant exceeds the device's `max_immediate_size`
/// (wgpu's validation limit) — fail fast with a clear message instead of a
/// wgpu validation error surfacing one frame later.
fn check_immediate_size(bytes: &[u8]) {
    assert!(
        bytes.len() <= MAX_IMMEDIATE_BYTES,
        "push constant is {} bytes; the device limit (max_immediate_size) is {MAX_IMMEDIATE_BYTES}",
        bytes.len(),
    );
}

/// A render slot: Canvas reads the gpu buffer, bind group, and push constant
/// from state ports each frame and issues draw calls.
#[allow(dead_code)]
pub(crate) struct RenderSlot<S> {
    pub(crate) gpu_buffer_port: Port<Option<Arc<GpuBufferSlot>>, S>,
    pub(crate) bind_group_port: Port<Option<Arc<wgpu::BindGroup>>, S>,
    pub(crate) has_fb_in_push_constant: bool,
    pub(crate) material_slot: Option<usize>,
    /// Re-run every frame: registers the *current* material value read from
    /// the state's material port (`register_material` dedups by value, so
    /// unchanged materials cost one map lookup; a changed value rebuilds the
    /// pipeline).
    pub(crate) register_material:
        Option<Box<dyn Fn(&mut Canvas<S>, &sm::Gpu, &DagStructRef<S>) -> usize>>,
    pub(crate) read_push_constant: Box<dyn Fn(&S) -> Vec<u8>>,
}

/// A batched render-object group: one shared [`InstanceArena`] + one material +
/// one push constant. Each `register_render_object_batched` object is an arena
/// slice; the canvas issues one `multi_draw_indexed_indirect` per group.
#[allow(dead_code)]
pub(crate) struct BatchedGroupSlot<S> {
    pub(crate) arena: Rc<RefCell<InstanceArena>>,
    pub(crate) has_fb_in_push_constant: bool,
    pub(crate) material_slot: Option<usize>,
    /// Re-run every frame — see [`RenderSlot::register_material`].
    pub(crate) register_material:
        Option<Box<dyn Fn(&mut Canvas<S>, &sm::Gpu, &DagStructRef<S>) -> usize>>,
    pub(crate) read_push_constant: Box<dyn Fn(&S) -> Vec<u8>>,
}

/// Per-frame push constant batch for the fast path (widget fills/outlines).
struct PushBatch {
    push_bytes: Vec<u8>,
    start: u32,
}

/// Fast-path buffer data stored per material for widget fills/outlines.
struct FastPathData {
    cpu: Vec<u8>,
    gpu_buffer: Option<wgpu::Buffer>,
    bind_group: Option<wgpu::BindGroup>,
    frame_count: u32,
    push_batches: Vec<PushBatch>,
}

/// Per-material cached pipeline + draw metadata. Fast-path slots also
/// hold instance buffer storage; render slots bypass this and use state ports.
struct MaterialSlot {
    pipeline: wgpu::RenderPipeline,
    index_buffer: wgpu::Buffer,
    draw: Draw,
    blend: Option<wgpu::BlendState>,
    bindings: MaterialBindings,
    fast: Option<FastPathData>,
}

/// Immediate-mode canvas: fast-path (widget) draws and render-slot draws
/// are both issued from `render()`. Pipeline + draw metadata is cached per
/// material. Fast-path GPU buffers are managed here; render-slot buffers
/// are produced by DAG upload nodes and read from the state.
pub(crate) struct Canvas<S> {
    registry: HashMap<TypeId, HashMap<ErasedKey, usize>>,
    slots: Vec<MaterialSlot>,
    render_slots: Vec<RenderSlot<S>>,
    batched_groups: Vec<BatchedGroupSlot<S>>,
    depth: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    width: u32,
    height: u32,
    clear_color: wgpu::Color,
    _marker: PhantomData<fn() -> S>,
}

impl<S> Canvas<S> {
    pub(crate) fn new() -> Self {
        Self {
            registry: HashMap::new(),
            slots: Vec::new(),
            render_slots: Vec::new(),
            batched_groups: Vec::new(),
            depth: None,
            width: 0,
            height: 0,
            clear_color: wgpu::Color {
                r: 0.118,
                g: 0.118,
                b: 0.118,
                a: 1.0,
            },
            _marker: PhantomData,
        }
    }

    /// Idempotent: an equal `M` already registered returns the existing handle.
    pub(crate) fn register_material<M: Material>(
        &mut self,
        gpu: &sm::Gpu,
        material: M,
    ) -> MaterialHandle<M> {
        let type_map = self.registry.entry(TypeId::of::<M>()).or_default();
        let probe = ErasedKey::new(material);
        if let Some(&index) = type_map.get(&probe) {
            return MaterialHandle::new(index);
        }

        let pipeline_data = probe.material::<M>().build(gpu);
        let bindings = <M::Instance as GpuStruct>::make_bindings(gpu);
        let indices = match &pipeline_data.draw {
            Draw::Primitive { indices } => indices,
        };
        let index_buffer = gpu.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(indices),
            usage: BufferUsages::INDEX,
        });
        let index = self.slots.len();
        self.slots.push(MaterialSlot {
            pipeline: pipeline_data.pipeline,
            index_buffer,
            draw: pipeline_data.draw,
            blend: pipeline_data.blend,
            bindings,
            fast: None,
        });
        type_map.insert(probe, index);
        MaterialHandle::new(index)
    }

    // ── Fast-path API (widget fills/outlines) ──────────────────────────

    fn fast(&mut self, idx: usize) -> &mut FastPathData {
        let slot = &mut self.slots[idx];
        slot.fast.get_or_insert_with(|| FastPathData {
            cpu: Vec::new(),
            gpu_buffer: None,
            bind_group: None,
            frame_count: 0,
            push_batches: Vec::new(),
        })
    }

    pub(crate) fn add_instance<M: Material>(
        &mut self,
        handle: &MaterialHandle<M>,
        instance: &M::Instance,
    ) {
        let fast = self.fast(handle.index);
        instance.serialize(&mut fast.cpu);
        fast.frame_count += 1;
    }

    pub(crate) fn set_push_constant<M: Material>(
        &mut self,
        handle: &MaterialHandle<M>,
        data: &M::PushConstant,
    ) {
        let fast = self.fast(handle.index);
        let data_bytes = bytemuck::bytes_of(data);
        check_immediate_size(data_bytes);
        let same = fast
            .push_batches
            .last()
            .is_some_and(|b| b.push_bytes.as_slice() == data_bytes);
        if same {
            return;
        }
        fast.push_batches.push(PushBatch {
            push_bytes: data_bytes.to_vec(),
            start: fast.frame_count,
        });
    }

    /// Clears fast-path instance buffers each frame (retains capacity).
    /// Render slots are left untouched — their data lives in the state.
    pub(crate) fn clear(&mut self) {
        for slot in &mut self.slots {
            if let Some(ref mut fast) = slot.fast {
                fast.frame_count = 0;
                fast.cpu.clear();
                fast.push_batches.clear();
            }
        }
    }

    // ── Render slot registration ───────────────────────────────────────

    pub(crate) fn add_render_slot(&mut self, slot: RenderSlot<S>) {
        self.render_slots.push(slot);
    }

    pub(crate) fn add_batched_group(&mut self, group: BatchedGroupSlot<S>) {
        self.batched_groups.push(group);
    }

    // ── Window / depth ─────────────────────────────────────────────────

    pub(crate) fn framebuffer_size(&self) -> crate::math::Vec2u {
        crate::math::Vec2u::new(self.width, self.height)
    }

    pub(crate) fn set_clear_color(&mut self, color: wgpu::Color) {
        self.clear_color = color;
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.depth = None;
    }

    pub(crate) fn depth_view(&self) -> Option<wgpu::TextureView> {
        self.depth.as_ref().map(|(_, view, _, _)| view.clone())
    }

    fn ensure_depth_texture(&mut self, gpu: &sm::Gpu) {
        let recreate = self
            .depth
            .as_ref()
            .is_none_or(|(_, _, width, height)| *width != self.width || *height != self.height);
        if !recreate {
            return;
        }
        let texture = gpu.create_texture(&wgpu::TextureDescriptor {
            label: Some("canvas depth"),
            size: wgpu::Extent3d {
                width: self.width.max(1),
                height: self.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth24Plus,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth = Some((texture, view, self.width, self.height));
    }

    // ── Render ─────────────────────────────────────────────────────────

    pub(crate) fn render(
        &mut self,
        gpu: &sm::Gpu,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        state: &mut DagStructRef<S>,
    ) {
        self.ensure_depth_texture(gpu);
        let depth_view = self.depth.as_ref().unwrap().1.clone();
        let fb = self.framebuffer_size();

        // Phase 1: resolve material slots for render slots. Registrars re-run
        // every frame and read the material port value from the state, so a
        // changed material rebuilds the pipeline (`register_material` dedups
        // by value, keeping unchanged materials a single map lookup). The
        // closure is taken out of the slot so it can borrow `self` mutably.
        let render_count = self.render_slots.len();
        for si in 0..render_count {
            let register = self.render_slots[si].register_material.take();
            if let Some(register) = register {
                let idx = register(self, gpu, state);
                self.render_slots[si].material_slot = Some(idx);
                self.render_slots[si].register_material = Some(register);
            }
        }

        // Phase 1b: resolve material slots for batched groups.
        let bg_count = self.batched_groups.len();
        for gi in 0..bg_count {
            let register = self.batched_groups[gi].register_material.take();
            if let Some(register) = register {
                let idx = register(self, gpu, state);
                self.batched_groups[gi].material_slot = Some(idx);
                self.batched_groups[gi].register_material = Some(register);
            }
        }

        // Phase 2: upload fast-path GPU buffers.
        for slot in &mut self.slots {
            if let Some(ref mut fast) = slot.fast {
                if fast.frame_count == 0 {
                    continue;
                }
                let needs_recreate = fast
                    .gpu_buffer
                    .as_ref()
                    .is_none_or(|buffer| buffer.size() < fast.cpu.len().max(1) as u64);
                if needs_recreate {
                    let contents: &[u8] = if fast.cpu.is_empty() {
                        &[0u8]
                    } else {
                        &fast.cpu
                    };
                    let buffer = gpu.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents,
                        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                    });
                    let binding = wgpu::BufferBinding {
                        buffer: &buffer,
                        offset: 0,
                        size: Some(NonZero::new(buffer.size()).unwrap()),
                    };
                    let bind_group =
                        (slot.bindings.make_bind_group)(gpu, &slot.bindings.layout, &binding);
                    fast.gpu_buffer = Some(buffer);
                    fast.bind_group = Some(bind_group);
                } else if !fast.cpu.is_empty() {
                    gpu.queue()
                        .write_buffer(fast.gpu_buffer.as_ref().unwrap(), 0, &fast.cpu);
                }
            }
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: Clear(self.clear_color),
                        store: Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: Clear(1.0),
                        store: Store,
                    }),
                    stencil_ops: None,
                }),
                multiview_mask: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // ── Fast-path draws (widget fills/outlines) ─────────────
            let mut sorted_fast: Vec<usize> = (0..self.slots.len()).collect();
            sorted_fast.sort_by_key(|&i| self.slots[i].blend.is_some());
            for &i in &sorted_fast {
                let slot = &self.slots[i];
                let Some(ref fast) = slot.fast else {
                    continue;
                };
                if fast.frame_count == 0 {
                    continue;
                }
                let Some(ref bind_group) = fast.bind_group else {
                    continue;
                };
                let index_count = match &slot.draw {
                    Draw::Primitive { indices } => indices.len() as u32,
                };
                for (bi, batch) in fast.push_batches.iter().enumerate() {
                    let end = if bi + 1 < fast.push_batches.len() {
                        fast.push_batches[bi + 1].start
                    } else {
                        fast.frame_count
                    };
                    pass.set_pipeline(&slot.pipeline);
                    pass.set_bind_group(0, bind_group, &[]);
                    pass.set_index_buffer(slot.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    if !batch.push_bytes.is_empty() {
                        pass.set_immediates(0, &batch.push_bytes);
                    }
                    pass.draw_indexed(0..index_count, 0, batch.start..end);
                }
            }

            // ── Render slot draws (state-backed) ────────────────────
            let mut sorted_rs: Vec<usize> = (0..self.render_slots.len()).collect();
            sorted_rs.sort_by_key(|&si| {
                self.render_slots[si]
                    .material_slot
                    .and_then(|mi| self.slots[mi].blend.is_some().then_some(true))
                    .unwrap_or(false)
            });
            for &si in &sorted_rs {
                let slot = &self.render_slots[si];
                let Some(mat_idx) = slot.material_slot else {
                    continue;
                };
                let mat = &self.slots[mat_idx];

                let gpu_buf: &Option<Arc<GpuBufferSlot>> = slot.gpu_buffer_port.read(state);
                let Some(gpu_buf) = gpu_buf.as_ref() else {
                    continue;
                };
                let bind_group: &Option<Arc<wgpu::BindGroup>> = slot.bind_group_port.read(state);
                let Some(bind_group) = bind_group.as_ref() else {
                    continue;
                };

                let mut push_bytes = (slot.read_push_constant)(state.inner());
                if slot.has_fb_in_push_constant && push_bytes.len() >= 8 {
                    push_bytes[..8].copy_from_slice(bytemuck::bytes_of(&fb));
                }
                check_immediate_size(&push_bytes);

                let index_count = match &mat.draw {
                    Draw::Primitive { indices } => indices.len() as u32,
                };

                pass.set_pipeline(&mat.pipeline);
                pass.set_bind_group(0, bind_group.as_ref(), &[]);
                pass.set_index_buffer(mat.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                if !push_bytes.is_empty() {
                    pass.set_immediates(0, &push_bytes);
                }
                pass.draw_indexed(0..index_count, 0, 0..gpu_buf.instance_count);
            }

            // ── Batched group draws (arena + indirect) ──────────────
            let mut sorted_bg: Vec<usize> = (0..self.batched_groups.len()).collect();
            sorted_bg.sort_by_key(|&gi| {
                self.batched_groups[gi]
                    .material_slot
                    .and_then(|mi| self.slots[mi].blend.is_some().then_some(true))
                    .unwrap_or(false)
            });
            for &gi in &sorted_bg {
                let group = &self.batched_groups[gi];
                let Some(mat_idx) = group.material_slot else {
                    continue;
                };
                let mat = &self.slots[mat_idx];

                let mut push_bytes = (group.read_push_constant)(state.inner());
                if group.has_fb_in_push_constant && push_bytes.len() >= 8 {
                    push_bytes[..8].copy_from_slice(bytemuck::bytes_of(&fb));
                }
                check_immediate_size(&push_bytes);

                let index_count = match &mat.draw {
                    Draw::Primitive { indices } => indices.len() as u32,
                };

                let mut arena = group.arena.borrow_mut();
                let needed = arena.committed_size();
                arena.ensure_capacity(gpu, needed);
                arena.ensure_bind_group(gpu, &mat.bindings);
                let bind_group = arena.bind_group().cloned();
                let args = arena.args(index_count);
                let args_buffer = arena.write_args(gpu, &args).cloned();
                drop(arena);

                let Some(bind_group) = bind_group else {
                    continue;
                };
                let Some(args_buffer) = args_buffer else {
                    continue;
                };

                pass.set_pipeline(&mat.pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.set_index_buffer(mat.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                if !push_bytes.is_empty() {
                    pass.set_immediates(0, &push_bytes);
                }
                pass.multi_draw_indexed_indirect(&args_buffer, 0, args.len() as u32);
            }
        }
    }
}
