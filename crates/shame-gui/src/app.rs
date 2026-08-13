//! The application entry point: window setup, the winit event loop, and
//! orchestration of the DAG and the GUI.
//!
//! [`App`] is a builder — configure it with [`App::add_gui`], optional DAG
//! construction (via [`App::graph_mut`]), then start the event loop with
//! [`App::run`].
//!
//! Custom render objects are registered with [`App::register_render_object`]:
//! pass a [`Material`](crate::material::Material) port, a typed
//! [`InstanceBuffer`](crate::material::InstanceBuffer) port, a push constant
//! port, and the declared GPU buffer + bind group ports. Two DAG nodes are
//! created internally — one uploads the CPU buffer to a GPU buffer, and one
//! creates the bind group.

use std::cell::RefCell;
use std::collections::HashMap;
use std::num::NonZero;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::Window;

use crate::buffer_pool::BufferPool;
use crate::canvas::{BatchedGroupSlot, Canvas, RenderSlot};
use crate::gpu;
use crate::graph::write_source_fields;
use crate::graph::{AppState, BuiltinState, DagStructRef, Graph, Port};
use crate::gui::Gui;
use crate::gui::event::InputEvent;
use crate::instance::GpuStruct;
use crate::material::{GpuBufferSlot, GpuInstanceBuffer, InstanceArena, InstanceBuffer, Material};
use crate::math::{Vec2, Vec2u};
use crate::shader::{RectInstance, RectMaterial, ViewportParams, WireframeMaterial};
use crate::text::TextSystem;
use shame_wgpu as sm;

fn push_bytes<PC: GpuStruct + crate::graph::PortValue, S: 'static>(
    constant: Port<PC, S>,
) -> Box<dyn Fn(&S) -> Vec<u8>> {
    Box::new(move |state: &S| {
        let pc: &PC = constant.read_state(state);
        crate::bytemuck::bytes_of(pc).to_vec()
    })
}

/// A windowed app with an optional GUI. Custom rendering is registered
/// via [`App::register_render_object`] (material + typed instance buffer +
/// push constant + gpu buffer + bind group ports); two internal DAG nodes
/// upload the CPU buffer and create the bind group. Built-in source/render
/// fields live in the state (via [`AppState`]); widget fills/outlines/texts
/// are merged on top.
///
/// The app owns a state `S` (a `#[derive(DagStruct)]` struct) and a
/// [`Graph<S>`]. Access the graph via [`App::graph_mut`] to add nodes,
/// connect ports, and mark ports dirty at any time.
pub struct App<S: AppState = BuiltinState> {
    title: String,
    gui: Option<Gui<S>>,
    text_system: Option<TextSystem>,
    clear_color: wgpu::Color,
    runner: Option<Box<dyn crate::capture::AppRunner<S>>>,
    state: S,
    graph: Graph<S>,
    pending_render_slots: Vec<RenderSlot<S>>,
    pending_batched_groups: Vec<BatchedGroupSlot<S>>,
    gpu_buffer_pool: Rc<RefCell<BufferPool<wgpu::Buffer>>>,
    /// Events emitted by the runner's `after_tick` hook, injected at the
    /// start of the next `step()` call.
    pending_events: Vec<InputEvent>,
}

impl<S: AppState> App<S> {
    /// Creates an app with the given window title and a default-initialized
    /// state. The state's built-in source fields (framebuffer, mouse, timing)
    /// are written every frame; the render fields (fills, outlines, texts)
    /// are read back after each tick.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            gui: None,
            text_system: None,
            clear_color: wgpu::Color {
                r: 0.118,
                g: 0.118,
                b: 0.118,
                a: 1.0,
            },
            runner: None,
            state: S::default(),
            graph: Graph::new(),
            pending_render_slots: Vec::new(),
            pending_batched_groups: Vec::new(),
            gpu_buffer_pool: Rc::new(RefCell::new(BufferPool::new())),
            pending_events: Vec::new(),
        }
    }

    /// Attaches a GUI (viewport tree + widgets); it renders above the
    /// DAG-produced objects and receives all window input.
    pub fn add_gui(&mut self, gui: Gui<S>) -> &mut Self {
        self.gui = Some(gui);
        self
    }

    /// Registers a custom render object driven by DAG ports.
    ///
    /// - `material` — a `Port<M, S>` holding the material (pipeline definition).
    /// - `cpu_buffer` — a `Port<InstanceBuffer<M::Instance>, S>` holding
    ///   serialized instance data.
    /// - `constant` — a `Port<M::PushConstant, S>` holding the push constant.
    /// - `gpu_buffer` / `bind_group` — declared `Option<Arc<...>>` state ports
    ///   the two internal nodes write to.
    pub fn register_render_object<M: Material>(
        &mut self,
        material: Port<M, S>,
        cpu_buffer: Port<InstanceBuffer<M::Instance>, S>,
        constant: Port<M::PushConstant, S>,
        gpu_buffer: Port<Option<Arc<GpuBufferSlot>>, S>,
        bind_group: Port<Option<Arc<wgpu::BindGroup>>, S>,
    ) -> &mut Self
    where
        M::PushConstant: crate::graph::PortValue,
    {
        let cpu_p = cpu_buffer;
        let gpu_p = gpu_buffer;
        let pool = self.gpu_buffer_pool.clone();
        self.graph.add_node(
            move |gref: &mut DagStructRef<S>, gpu: Option<&sm::Gpu>| {
                let Some(gpu) = gpu else { return };
                let (is_empty, bytes, instance_count) = {
                    let cpu: &InstanceBuffer<M::Instance> = cpu_p.read(gref);
                    (
                        cpu.is_empty(),
                        cpu.as_bytes().to_vec(),
                        cpu.instance_count(),
                    )
                };

                if is_empty {
                    let slot = gpu_p.read_mut(gref);
                    if let Some(old) = slot.take() {
                        if let Ok(s) = Arc::try_unwrap(old) {
                            let size = s.buffer.size();
                            pool.borrow_mut().release(s.buffer, size);
                        }
                    }
                    return;
                }
                let needed = bytes.len().max(1) as u64;

                let slot = gpu_p.read_mut(gref);
                let old_arc = slot.take();
                let buffer = if let Some(old_arc) = old_arc {
                    if old_arc.buffer.size() >= needed {
                        let old_slot = Arc::try_unwrap(old_arc).expect("sole GpuBufferSlot owner");
                        old_slot.buffer
                    } else {
                        if let Ok(s) = Arc::try_unwrap(old_arc) {
                            let size = s.buffer.size();
                            pool.borrow_mut().release(s.buffer, size);
                        }
                        pool.borrow_mut().acquire(needed, |n| {
                            gpu.create_buffer(&wgpu::BufferDescriptor {
                                label: None,
                                size: n,
                                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                                mapped_at_creation: false,
                            })
                        })
                    }
                } else {
                    pool.borrow_mut().acquire(needed, |n| {
                        gpu.create_buffer(&wgpu::BufferDescriptor {
                            label: None,
                            size: n,
                            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        })
                    })
                };

                gpu.queue().write_buffer(
                    &buffer,
                    0,
                    if bytes.is_empty() { &[0u8] } else { &bytes },
                );

                *slot = Some(Arc::new(GpuBufferSlot {
                    buffer,
                    instance_count,
                }));
            },
            cpu_p,
            gpu_p,
            None,
        );

        let gpu_p = gpu_buffer;
        let mat_p = material;
        let bind_p = bind_group;
        self.graph.add_node(
            move |gref: &mut DagStructRef<S>, gpu: Option<&sm::Gpu>| {
                let Some(gpu) = gpu else { return };
                let gpu_buf: &Option<Arc<GpuBufferSlot>> = gpu_p.read(gref);
                let Some(ref gpu_buf) = *gpu_buf else {
                    *bind_p.read_mut(gref) = None;
                    return;
                };
                let bindings = <M::Instance as GpuStruct>::make_bindings(gpu);
                let binding = wgpu::BufferBinding {
                    buffer: &gpu_buf.buffer,
                    offset: 0,
                    size: Some(NonZero::new(gpu_buf.buffer.size()).unwrap()),
                };
                let bind_group = (bindings.make_bind_group)(gpu, &bindings.layout, &binding);
                *bind_p.read_mut(gref) = Some(Arc::new(bind_group));
            },
            (gpu_p, mat_p),
            bind_p,
            None,
        );

        let has_fb = M::HAS_FB_PUSH_CONSTANT;
        self.pending_render_slots.push(RenderSlot {
            gpu_buffer_port: gpu_buffer,
            bind_group_port: bind_group,
            has_fb_in_push_constant: has_fb,
            material_slot: None,
            register_material: Some(Box::new(move |canvas: &mut Canvas<S>, gpu| {
                canvas.register_material(gpu, M::default()).index
            })),
            read_push_constant: push_bytes::<M::PushConstant, S>(constant),
        });

        self
    }

    /// Registers multiple render objects that share one material and one push
    /// constant, batched into a single indirect dispatch.
    ///
    /// Unlike [`Self::register_render_object`] — which gives every object its
    /// own GPU buffer, bind group, and draw call — this writes all objects'
    /// instances into one shared [`InstanceArena`] and issues one
    /// `multi_draw_indexed_indirect` per call. Each object keeps its own
    /// [`InstanceBuffer`](crate::material::InstanceBuffer) port and its own
    /// upload node, so only the objects that changed are re-uploaded; no
    /// CPU-side concatenation happens.
    ///
    /// - `material` — a `Port<M, S>` holding the material (shared pipeline).
    /// - `constant` — a `Port<M::PushConstant, S>` holding the shared push
    ///   constant (must be identical across the whole batch; a single indirect
    ///   dispatch has a single push constant).
    /// - `cpu_buffers` — one `Port<InstanceBuffer<M::Instance>, S>` per object.
    pub fn register_render_objects_batched<M: Material>(
        &mut self,
        material: Port<M, S>,
        constant: Port<M::PushConstant, S>,
        cpu_buffers: impl IntoIterator<Item = Port<InstanceBuffer<M::Instance>, S>>,
    ) -> &mut Self
    where
        M::PushConstant: crate::graph::PortValue,
    {
        let wire_size = <M::Instance as GpuStruct>::wire_size();
        let arena = Rc::new(RefCell::new(InstanceArena::new(wire_size)));

        for cpu_p in cpu_buffers {
            let slot = arena.borrow_mut().add_slot();
            let arena2 = arena.clone();
            let mat_p = material;
            self.graph.add_node(
                move |gref: &mut DagStructRef<S>, gpu: Option<&sm::Gpu>| {
                    let Some(gpu) = gpu else { return };
                    let cpu: &InstanceBuffer<M::Instance> = cpu_p.read(gref);
                    let (is_empty, bytes, count) = (
                        cpu.is_empty(),
                        cpu.as_bytes().to_vec(),
                        cpu.instance_count(),
                    );
                    let mut arena = arena2.borrow_mut();
                    if is_empty {
                        arena.free_slot(slot);
                        return;
                    }
                    let bytes_len = bytes.len().max(1) as u32;
                    let offset = arena.alloc_for_slot(slot, count, bytes_len);
                    // Grow the arena buffer *before* writing, so `upload` never
                    // targets an offset past the current buffer's end.
                    let needed = arena.committed_size();
                    arena.ensure_capacity(gpu, needed);
                    arena.upload(gpu, offset, if bytes.is_empty() { &[0u8] } else { &bytes });
                },
                (cpu_p, mat_p),
                (),
                None,
            );
        }

        let has_fb = M::HAS_FB_PUSH_CONSTANT;
        self.pending_batched_groups.push(BatchedGroupSlot {
            arena,
            has_fb_in_push_constant: has_fb,
            material_slot: None,
            register_material: Some(Box::new(move |canvas: &mut Canvas<S>, gpu| {
                canvas.register_material(gpu, M::default()).index
            })),
            read_push_constant: push_bytes::<M::PushConstant, S>(constant),
        });

        self
    }

    /// Registers a dynamic `HashMap<K, E>` of render objects that share one
    /// material and one push constant, batched into a single indirect dispatch.
    ///
    /// Each element carries its own CPU [`InstanceBuffer`] (filled by the user)
    /// and an [`Option<GpuInstanceBuffer>`] handle that the framework writes
    /// after uploading. From the element's perspective this handle *is* its GPU
    /// buffer — the shared-buffer/arena bookkeeping is internal. Dropping an
    /// element (its key is removed from the map) releases its GPU buffer
    /// automatically.
    ///
    /// - `map` — the dynamic map port.
    /// - `material` — a `Port<M, S>` holding the material (shared pipeline).
    /// - `constant` — a `Port<M::PushConstant, S>` holding the shared push
    ///   constant (identical across all elements).
    /// - `cpu_buffer` — the element's `InstanceBuffer<M::Instance>` port.
    /// - `gpu_buffer` — the element's `Option<GpuInstanceBuffer>` port.
    pub fn register_map_render_objects_batched<M, K, E>(
        &mut self,
        map: Port<HashMap<K, E>, S>,
        material: Port<M, S>,
        constant: Port<M::PushConstant, S>,
        cpu_buffer: Port<InstanceBuffer<M::Instance>, E>,
        gpu_buffer: Port<Option<GpuInstanceBuffer>, E>,
    ) -> &mut Self
    where
        M: Material,
        K: Clone + Eq + std::hash::Hash + 'static,
        E: Clone + 'static,
        M::PushConstant: crate::graph::PortValue,
    {
        let wire_size = <M::Instance as GpuStruct>::wire_size();
        let arena = Rc::new(RefCell::new(InstanceArena::new(wire_size)));
        let arena2 = arena.clone();

        self.graph.add_map_node(
            map,
            material,
            (),
            cpu_buffer,
            gpu_buffer,
            move |_gref: &mut DagStructRef<S>,
                  gpu: Option<&sm::Gpu>,
                  _k: &K,
                  eref: &mut DagStructRef<E>| {
                let Some(gpu) = gpu else { return };
                let cpu: &InstanceBuffer<M::Instance> = cpu_buffer.read(eref);
                let is_empty = cpu.is_empty();
                let bytes = cpu.as_bytes().to_vec();
                let count = cpu.instance_count();

                if is_empty {
                    // Releasing the handle frees the element's GPU slice.
                    gpu_buffer.write(eref, None);
                    return;
                }

                let (slot, is_new) = match gpu_buffer.read(eref) {
                    Some(handle) => (handle.slot(), false),
                    None => (arena2.borrow_mut().add_slot(), true),
                };

                let bytes_len = bytes.len().max(1) as u32;
                {
                    let mut arena = arena2.borrow_mut();
                    let offset = arena.alloc_for_slot(slot, count, bytes_len);
                    let needed = arena.committed_size();
                    arena.ensure_capacity(gpu, needed);
                    arena.upload(gpu, offset, if bytes.is_empty() { &[0u8] } else { &bytes });
                }

                if is_new {
                    let handle = GpuInstanceBuffer::new(arena2.clone(), slot);
                    gpu_buffer.write(eref, Some(handle));
                }
            },
        );

        let has_fb = M::HAS_FB_PUSH_CONSTANT;
        self.pending_batched_groups.push(BatchedGroupSlot {
            arena,
            has_fb_in_push_constant: has_fb,
            material_slot: None,
            register_material: Some(Box::new(move |canvas: &mut Canvas<S>, gpu| {
                canvas.register_material(gpu, M::default()).index
            })),
            read_push_constant: push_bytes::<M::PushConstant, S>(constant),
        });

        self
    }

    /// Sets the clear color of the window background (wgpu linear-space RGBA).
    #[must_use]
    pub fn with_clear_color(mut self, color: wgpu::Color) -> Self {
        self.clear_color = color;
        self
    }

    /// Attaches a runner for test/extension hooks. The runner's `after_tick`
    /// is called after each DAG tick (both `step()` and `run()`); `after_render`
    /// is called after each rendered frame (`run()` only).
    #[must_use]
    pub fn with_runner(mut self, runner: impl crate::capture::AppRunner<S> + 'static) -> Self {
        self.runner = Some(Box::new(runner));
        self
    }

    /// Immutable access to the graph.
    pub fn graph(&self) -> &Graph<S> {
        &self.graph
    }

    /// Mutable access to the graph for adding nodes.
    pub fn graph_mut(&mut self) -> &mut Graph<S> {
        &mut self.graph
    }

    /// Immutable access to the state.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Mutable access to the state (for seeding initial values).
    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    /// Processes input events through the GUI and ticks the DAG for one frame.
    /// `elapsed` is the running total, `dt` the frame delta, `fb_size` the
    /// window size. Does not render — call from tests or before rendering.
    pub fn step(
        &mut self,
        inputs: &[InputEvent],
        dt: f32,
        elapsed: f32,
        cursor: Vec2,
        fb_size: Vec2u,
    ) {
        let mut events: Vec<InputEvent> = self.pending_events.drain(..).collect();
        events.extend_from_slice(inputs);

        let mut cursor = cursor;
        let mut mouse_down = false;
        for ev in &events {
            if let Some(pos) = crate::gui::event::event_pos(ev) {
                cursor = pos;
            }
            match ev {
                InputEvent::MouseDown { .. } => mouse_down = true,
                InputEvent::MouseUp { .. } => mouse_down = false,
                _ => {}
            }
        }

        for input in &events {
            if let Some(gui) = &mut self.gui {
                let mut dagref = self.graph.with_state(&mut self.state);
                gui.on_event(input, fb_size, &mut dagref);
            }
        }
        self.tick_dag(dt, elapsed, cursor, fb_size, mouse_down);
    }

    fn tick_dag(&mut self, dt: f32, elapsed: f32, cursor: Vec2, fb_size: Vec2u, mouse_down: bool) {
        if self.graph.is_active() {
            {
                let mut dagref = self.graph.with_state(&mut self.state);
                write_source_fields(&mut dagref, fb_size, cursor, mouse_down, 0.0, dt, elapsed);
            }
            self.graph.tick(&mut self.state, None);
        }
        let emitted = if let Some(ref mut runner) = self.runner {
            let ctx = crate::capture::AppContext {
                state: &self.state,
                graph: &self.graph,
                gui: self.gui.as_ref(),
            };
            runner.after_tick(&ctx)
        } else {
            vec![]
        };
        self.pending_events = emitted;
    }

    /// Starts the winit event loop. `text_system` renders all text objects
    /// produced by DAG nodes and the GUI in a second pass above the render
    /// objects.
    pub fn run(mut self, text_system: TextSystem) {
        self.text_system = Some(text_system);
        self.run_impl()
    }

    fn run_impl(self) {
        let mut builder = winit::event_loop::EventLoop::builder();
        #[cfg(windows)]
        {
            use winit::platform::windows::EventLoopBuilderExtWindows;
            builder.with_any_thread(true);
        }
        let event_loop = builder.build().unwrap();
        event_loop.set_control_flow(ControlFlow::Poll);
        let App {
            title,
            gui,
            text_system,
            clear_color,
            runner,
            state,
            graph,
            pending_render_slots,
            pending_batched_groups,
            gpu_buffer_pool: _,
            pending_events: _,
        } = self;
        let mut app = FrameApp {
            title,
            gui,
            text_system: text_system.unwrap(),
            clear_color,
            runner,
            window: None,
            gpu_setup: None,
            canvas: None,
            cursor: Vec2::new(0.0, 0.0),
            state,
            graph,
            last_instant: Instant::now(),
            elapsed: 0.0,
            mouse_down: false,
            scroll_delta: 0.0,
            pending_render_slots,
            pending_batched_groups,
        };
        event_loop.run_app(&mut app).unwrap();
    }
}

struct FrameApp<S: AppState> {
    title: String,
    gui: Option<Gui<S>>,
    text_system: TextSystem,
    clear_color: wgpu::Color,
    window: Option<Arc<Window>>,
    gpu_setup: Option<gpu::Setup>,
    canvas: Option<Canvas<S>>,
    cursor: Vec2,
    state: S,
    graph: Graph<S>,
    last_instant: Instant,
    elapsed: f32,
    mouse_down: bool,
    scroll_delta: f32,
    pending_render_slots: Vec<RenderSlot<S>>,
    pending_batched_groups: Vec<BatchedGroupSlot<S>>,
    runner: Option<Box<dyn crate::capture::AppRunner<S>>>,
}

impl<S: AppState> FrameApp<S> {
    fn tick_dag(&mut self, gpu: Option<&sm::Gpu>) {
        if !self.graph.is_active() {
            return;
        }
        let dt = self.last_instant.elapsed().as_secs_f32();
        self.last_instant = Instant::now();
        self.elapsed += dt;

        let fb_size = self.canvas.as_ref().unwrap().framebuffer_size();
        {
            let mut dagref = self.graph.with_state(&mut self.state);
            write_source_fields(
                &mut dagref,
                fb_size,
                self.cursor,
                self.mouse_down,
                self.scroll_delta,
                dt,
                self.elapsed,
            );
        }
        self.scroll_delta = 0.0;

        self.graph.tick(&mut self.state, gpu);

        if let Some(ref mut runner) = self.runner {
            let ctx = crate::capture::AppContext {
                state: &self.state,
                graph: &self.graph,
                gui: self.gui.as_ref(),
            };
            runner.after_tick(&ctx);
        }
    }

    fn handle_input(&mut self, event: &WindowEvent) {
        if let WindowEvent::CursorMoved { position, .. } = event {
            self.cursor = Vec2::new(position.x as f32, position.y as f32);
        }
        match event {
            WindowEvent::MouseInput { state, .. } => {
                self.mouse_down = *state == ElementState::Pressed;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y * 16.0,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
                };
                self.scroll_delta += dy;
            }
            _ => {}
        }
        match event {
            WindowEvent::CursorMoved { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
            | WindowEvent::KeyboardInput { .. } => {
                if let Some(gui) = &mut self.gui {
                    if let Some(input) = InputEvent::from_winit(event, self.cursor) {
                        let size = self.window.as_ref().unwrap().inner_size();
                        let mut dagref = self.graph.with_state(&mut self.state);
                        gui.on_event(&input, Vec2u::new(size.width, size.height), &mut dagref);
                        self.window.as_ref().unwrap().request_redraw();
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(gui) = &mut self.gui {
                    let size = self.window.as_ref().unwrap().inner_size();
                    let mut dagref = self.graph.with_state(&mut self.state);
                    gui.on_event(
                        &InputEvent::MouseMove {
                            pos: Vec2::new(-10000.0, -10000.0),
                            pressure: None,
                        },
                        Vec2u::new(size.width, size.height),
                        &mut dagref,
                    );
                    self.window.as_ref().unwrap().request_redraw();
                }
            }
            _ => {}
        }
    }

    fn handle_redraw(&mut self, event_loop: &ActiveEventLoop) {
        let gpu_ptr: *const sm::Gpu = &self.gpu_setup.as_ref().unwrap().gpu;
        self.tick_dag(unsafe { Some(&*gpu_ptr) });

        let window = Arc::clone(self.window.as_ref().unwrap());
        let gpu_setup = self.gpu_setup.as_mut().unwrap();
        let canvas = self.canvas.as_mut().unwrap();
        let (surface_texture, view) = gpu_setup.try_acquire_surface();

        canvas.clear();

        let render = S::render_ports();
        let dag_fills = render.fills.read_state(&self.state).clone();
        let dag_outlines = render.outlines.read_state(&self.state).clone();
        let dag_texts = render.texts.read_state(&self.state).clone();

        self.text_system.clear();
        for t in dag_texts {
            self.text_system.queue(t.clone());
        }
        if let Some(gui) = &self.gui {
            let mut dagref = self.graph.with_state(&mut self.state);
            gui.render_with_dag(
                &gpu_setup.gpu,
                canvas,
                &mut self.text_system,
                &mut dagref,
                &dag_fills,
                &dag_outlines,
            );
        } else {
            let fb = canvas.framebuffer_size();
            let vp = ViewportParams { fb_size: fb };
            if !dag_fills.is_empty() {
                let handle = canvas.register_material(&gpu_setup.gpu, RectMaterial);
                canvas.set_push_constant(&handle, &vp);
                for entry in &dag_fills {
                    canvas.add_instance(
                        &handle,
                        &RectInstance {
                            rect: entry.rect,
                            color: entry.color,
                            z: entry.z,
                        },
                    );
                }
            }
            if !dag_outlines.is_empty() {
                let handle = canvas.register_material(&gpu_setup.gpu, WireframeMaterial);
                canvas.set_push_constant(&handle, &vp);
                for entry in &dag_outlines {
                    canvas.add_instance(
                        &handle,
                        &RectInstance {
                            rect: entry.rect,
                            color: entry.color,
                            z: entry.z,
                        },
                    );
                }
            }
        }

        let mut encoder = gpu_setup
            .gpu
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut dagref = self.graph.with_state(&mut self.state);
            canvas.render(&gpu_setup.gpu, &mut encoder, &view, &mut dagref);
        }

        {
            let config = gpu_setup.surface_config();
            self.text_system
                .ensure_gpu(&gpu_setup.gpu, gpu_setup.gpu.queue(), config.format);
            self.text_system
                .update_viewport(gpu_setup.gpu.queue(), config.width, config.height);
            self.text_system
                .prepare(&gpu_setup.gpu, gpu_setup.gpu.queue())
                .expect("text prepare");
            let depth_view = canvas.depth_view().expect("canvas depth view");
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("text pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                multiview_mask: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.text_system.render(&mut pass).expect("text render");
        }
        gpu_setup.gpu.queue().submit([encoder.finish()]);

        if let Some(ref mut runner) = self.runner {
            let config = gpu_setup.surface_config();
            let frame = crate::capture::FrameOutput {
                surface_texture: &surface_texture.texture,
                surface_format: config.format,
                surface_width: config.width,
                surface_height: config.height,
                gpu: &gpu_setup.gpu,
            };
            if runner.after_render(&frame) {
                event_loop.exit();
                return;
            }
        }

        window.pre_present_notify();
        surface_texture.present();
        window.request_redraw();
    }

    fn handle_resize(&mut self, width: u32, height: u32) {
        let gpu_setup = self.gpu_setup.as_mut().unwrap();
        let canvas = self.canvas.as_mut().unwrap();
        gpu_setup.resize(width, height);
        canvas.resize(width.max(1), height.max(1));
        self.window.as_ref().unwrap().request_redraw();
    }
}

impl<S: AppState> ApplicationHandler for FrameApp<S> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu_setup.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(&self.title)
                        .with_inner_size(winit::dpi::PhysicalSize::new(1200, 800)),
                )
                .unwrap(),
        );
        let gpu_setup = gpu::Setup::new(&window);
        let mut canvas = Canvas::new();
        canvas.set_clear_color(self.clear_color);
        let size = window.inner_size();
        canvas.resize(size.width.max(1), size.height.max(1));
        for pending in self.pending_render_slots.drain(..) {
            canvas.add_render_slot(pending);
        }
        for pending in self.pending_batched_groups.drain(..) {
            canvas.add_batched_group(pending);
        }
        self.window = Some(window);
        self.gpu_setup = Some(gpu_setup);
        self.canvas = Some(canvas);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CursorMoved { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
            | WindowEvent::KeyboardInput { .. }
            | WindowEvent::CursorLeft { .. } => {
                self.handle_input(&event);
            }
            WindowEvent::RedrawRequested => self.handle_redraw(event_loop),
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.handle_resize(size.width, size.height),
            _ => {}
        }
    }
}
