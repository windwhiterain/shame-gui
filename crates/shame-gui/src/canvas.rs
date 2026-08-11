use std::any::TypeId;
use std::collections::HashMap;
use std::num::NonZero;
use std::sync::Arc;

use wgpu::util::DeviceExt as _;
use wgpu::{BufferUsages, LoadOp::*, StoreOp::*};

use shame_wgpu as sm;

use crate::graph::{PortId, StateArena};
use crate::instance::GpuStruct;
use crate::material::{Draw, ErasedKey, GpuBufferSlot, Material, MaterialBindings, MaterialHandle};

/// A render slot: Canvas reads material, gpu buffer, bind group, and push
/// constant from arena ports each frame and issues draw calls.
#[allow(dead_code)]
pub(crate) struct RenderSlot {
    pub(crate) material_port: PortId,
    pub(crate) gpu_buffer_port: PortId,
    pub(crate) bind_group_port: PortId,
    pub(crate) constant_port: PortId,
    pub(crate) has_fb_in_push_constant: bool,
    pub(crate) material_slot: Option<usize>,
    pub(crate) register_material: Option<Box<dyn FnOnce(&mut Canvas, &sm::Gpu) -> usize>>,
    pub(crate) read_push_constant: fn(&StateArena, PortId) -> Vec<u8>,
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
/// hold instance buffer storage; render slots bypass this and use arena.
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
/// are produced by DAG upload nodes and read from the arena.
pub(crate) struct Canvas {
    registry: HashMap<TypeId, HashMap<ErasedKey, usize>>,
    slots: Vec<MaterialSlot>,
    render_slots: Vec<RenderSlot>,
    depth: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    width: u32,
    height: u32,
    clear_color: wgpu::Color,
}

impl Canvas {
    pub(crate) fn new() -> Self {
        Self {
            registry: HashMap::new(),
            slots: Vec::new(),
            render_slots: Vec::new(),
            depth: None,
            width: 0,
            height: 0,
            clear_color: wgpu::Color {
                r: 0.118,
                g: 0.118,
                b: 0.118,
                a: 1.0,
            },
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
    /// Render slots are left untouched — their data lives in the arena.
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

    pub(crate) fn add_render_slot(&mut self, slot: RenderSlot) {
        self.render_slots.push(slot);
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
        arena: &mut StateArena,
    ) {
        self.ensure_depth_texture(gpu);
        let depth_view = self.depth.as_ref().unwrap().1.clone();
        let fb = self.framebuffer_size();

        // Phase 1: resolve material slots for first-time render slots.
        let render_count = self.render_slots.len();
        let mut deferred: Vec<(usize, Box<dyn FnOnce(&mut Canvas, &sm::Gpu) -> usize>)> =
            Vec::new();
        for si in 0..render_count {
            let slot = &mut self.render_slots[si];
            if slot.material_slot.is_none() {
                if let Some(register) = slot.register_material.take() {
                    deferred.push((si, register));
                }
            }
        }
        for (si, register) in deferred {
            let idx = register(self, gpu);
            self.render_slots[si].material_slot = Some(idx);
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

            // ── Render slot draws (arena-backed) ────────────────────
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

                let gpu_buf: &Option<Arc<GpuBufferSlot>> = arena.read(slot.gpu_buffer_port);
                let Some(gpu_buf) = gpu_buf.as_ref() else {
                    continue;
                };
                let bind_group: &Option<Arc<wgpu::BindGroup>> = arena.read(slot.bind_group_port);
                let Some(bind_group) = bind_group.as_ref() else {
                    continue;
                };

                let mut push_bytes = (slot.read_push_constant)(arena, slot.constant_port);
                if slot.has_fb_in_push_constant && push_bytes.len() >= 8 {
                    push_bytes[..8].copy_from_slice(bytemuck::bytes_of(&fb));
                }

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
        }
    }
}
