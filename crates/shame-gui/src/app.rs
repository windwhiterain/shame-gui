//! The application entry point: window setup, the winit event loop, and
//! orchestration of the DAG and the GUI.
//!
//! [`App`] is a builder — configure it with [`App::add_gui`], optional DAG
//! construction ([`App::graph_builder`] / [`App::finalize_graph`]), then
//! start the event loop with [`App::run`].
//!
//! Custom render objects are registered with [`App::register_render_object`]:
//! pass a [`Material`](crate::material::Material) port, a typed
//! [`InstanceBuffer`](crate::material::InstanceBuffer) port, and a push
//! constant port. Two DAG nodes are created internally — one that uploads
//! the CPU buffer to a GPU buffer, and one that creates the bind group.

use std::num::NonZero;
use std::sync::Arc;
use std::time::Instant;

use wgpu::util::DeviceExt as _;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::Window;

use crate::canvas::Canvas;
use crate::gpu;
use crate::graph::RenderPorts;
use crate::graph::source::SourcePorts;
use crate::graph::{Graph, Port, PortId, StateArena};
use crate::gui::Gui;
use crate::gui::event::InputEvent;
use crate::instance::GpuStruct;
use crate::material::{GpuBufferSlot, InstanceBuffer, Material};
use crate::math::{Vec2, Vec2u};
use crate::shader::{RectEntry, RectInstance, RectMaterial, ViewportParams, WireframeMaterial};
use crate::text::{TextObject, TextSystem};
use shame_wgpu as sm;

struct PendingRenderSlot {
    material_port: PortId,
    gpu_buffer_port: PortId,
    bind_group_port: PortId,
    constant_port: PortId,
    has_fb_in_push_constant: bool,
    read_push_constant: fn(&StateArena, PortId) -> Vec<u8>,
    register_material: Option<Box<dyn FnOnce(&mut Canvas, &sm::Gpu) -> usize>>,
}

fn read_push_bytes<PC: GpuStruct + crate::graph::PortValue>(
    arena: &StateArena,
    port: PortId,
) -> Vec<u8> {
    let pc: &PC = arena.read(port);
    crate::bytemuck::bytes_of(pc).to_vec()
}

/// A windowed app with an optional GUI. Custom rendering is registered
/// via [`App::register_render_object`] (material + typed instance buffer +
/// push constant ports); two internal DAG nodes upload the CPU buffer and
/// create the bind group. Built-in [`RenderPorts`] (fills, outlines, texts)
/// are drawn by DAG nodes or passed directly to the canvas. Widget
/// fills/outlines/texts are merged on top.
///
/// The app owns a [`StateArena`] and a [`Graph`] from creation — arena slots
/// and DAG nodes can be allocated at any time before `run`. All builder
/// methods take `&mut self` and return it, so calls can be chained.
pub struct App {
    title: String,
    gui: Option<Gui>,
    text_system: Option<TextSystem>,
    clear_color: wgpu::Color,
    runner: Option<Box<dyn crate::capture::AppRunner>>,
    arena: StateArena,
    graph: Graph,
    pending_render_slots: Vec<PendingRenderSlot>,
}

impl App {
    /// Creates an app with the given window title and a fresh arena + graph.
    ///
    /// The graph comes pre-wired with the built-in [`SourcePorts`]
    /// (framebuffer size, mouse, timing) and [`RenderPorts`] (fills,
    /// outlines, texts) — see [`Graph::new`].
    pub fn new(title: impl Into<String>) -> Self {
        let mut arena = StateArena::new();
        let fb_id = arena.alloc_with(crate::math::Vec2u::new(0, 0));
        let mouse_id = arena.alloc_with(crate::math::Vec2::new(0.0, 0.0));
        let md_id = arena.alloc_with(false);
        let scroll_id = arena.alloc_with(0.0f32);
        let dt_id = arena.alloc_with(0.0f32);
        let elapsed_id = arena.alloc_with(0.0f32);
        let fills_id = arena.alloc_with(Vec::<RectEntry>::new());
        let outlines_id = arena.alloc_with(Vec::<RectEntry>::new());
        let texts_id = arena.alloc_with(Vec::<TextObject>::new());

        let source = SourcePorts {
            framebuffer_size: Port::new(fb_id),
            mouse_pos: Port::new(mouse_id),
            mouse_down: Port::new(md_id),
            scroll_delta: Port::new(scroll_id),
            delta_time: Port::new(dt_id),
            elapsed: Port::new(elapsed_id),
        };
        let render = RenderPorts {
            fills: Port::new(fills_id),
            outlines: Port::new(outlines_id),
            texts: Port::new(texts_id),
        };

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
            arena,
            graph: Graph::new(source, render),
            pending_render_slots: Vec::new(),
        }
    }

    /// Attaches a GUI (viewport tree + widgets); it renders above the
    /// DAG-produced objects and receives all window input.
    pub fn add_gui(&mut self, gui: Gui) -> &mut Self {
        self.gui = Some(gui);
        self
    }

    /// Registers a custom render object driven by DAG ports.
    ///
    /// - `material` — a [`Port<M>`] holding the material (pipeline definition).
    /// - `cpu_buffer` — a [`Port<InstanceBuffer<M::Instance>>`] holding
    ///   serialized instance data.
    /// - `constant` — a [`Port<P>`] holding the push constant.
    ///
    /// Internally creates two DAG nodes:
    /// 1. Upload node: reads `cpu_buffer`, creates a wgpu buffer, writes
    ///    to a `gpu_buffer` port.
    /// 2. Bind group node: reads `gpu_buffer` + `material`, creates a bind
    ///    group, writes to a `bind_group` port.
    ///
    /// Call before [`App::finalize_graph`].
    pub fn register_render_object<M: Material + crate::graph::PortValue>(
        &mut self,
        material: Port<M>,
        cpu_buffer: Port<InstanceBuffer<M::Instance>>,
        constant: Port<M::PushConstant>,
    ) -> &mut Self
    where
        M::PushConstant: crate::graph::PortValue,
    {
        let gpu_buffer_port: Port<Option<Arc<GpuBufferSlot>>> =
            Port::new(self.arena.alloc::<Option<Arc<GpuBufferSlot>>>());
        let bind_group_port: Port<Option<Arc<wgpu::BindGroup>>> =
            Port::new(self.arena.alloc::<Option<Arc<wgpu::BindGroup>>>());

        {
            let mut b = self.graph.builder();
            let cpu_p = cpu_buffer;
            let gpu_p = gpu_buffer_port;

            // Node 1: upload CPU buffer → GPU buffer.
            b.add_node(
                move |arena: &mut StateArena, gpu: Option<&sm::Gpu>| {
                    let Some(gpu) = gpu else { return };
                    let cpu: &InstanceBuffer<M::Instance> = arena.read(cpu_p.id());
                    if cpu.is_empty() {
                        *arena.read_mut::<Option<Arc<GpuBufferSlot>>>(gpu_p.id()) = None;
                        return;
                    }
                    let bytes = cpu.as_bytes();
                    let buffer = gpu.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: if bytes.is_empty() { &[0u8] } else { bytes },
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    });
                    *arena.read_mut(gpu_p.id()) = Some(Arc::new(GpuBufferSlot {
                        buffer,
                        instance_count: cpu.instance_count(),
                    }));
                },
                cpu_p,
                gpu_p,
            );
        }

        {
            let mut b = self.graph.builder();
            let gpu_p = gpu_buffer_port;
            let mat_p = material;
            let bind_p = bind_group_port;

            // Node 2: create bind group from GPU buffer + material instance layout.
            b.add_node(
                move |arena: &mut StateArena, gpu: Option<&sm::Gpu>| {
                    let Some(gpu) = gpu else { return };
                    let gpu_buf: &Option<Arc<GpuBufferSlot>> = arena.read(gpu_p.id());
                    let Some(ref gpu_buf) = *gpu_buf else {
                        *arena.read_mut::<Option<Arc<wgpu::BindGroup>>>(bind_p.id()) = None;
                        return;
                    };
                    let bindings = <M::Instance as GpuStruct>::make_bindings(gpu);
                    let binding = wgpu::BufferBinding {
                        buffer: &gpu_buf.buffer,
                        offset: 0,
                        size: Some(NonZero::new(gpu_buf.buffer.size()).unwrap()),
                    };
                    let bind_group = (bindings.make_bind_group)(gpu, &bindings.layout, &binding);
                    *arena.read_mut(bind_p.id()) = Some(Arc::new(bind_group));
                },
                (gpu_p, mat_p),
                bind_p,
            );
        }

        let has_fb = M::HAS_FB_PUSH_CONSTANT;
        self.pending_render_slots.push(PendingRenderSlot {
            material_port: material.id(),
            gpu_buffer_port: gpu_buffer_port.id(),
            bind_group_port: bind_group_port.id(),
            constant_port: constant.id(),
            has_fb_in_push_constant: has_fb,
            read_push_constant: read_push_bytes::<M::PushConstant>,
            register_material: Some(Box::new(move |canvas, gpu| {
                canvas.register_material(gpu, M::default()).index
            })),
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
    pub fn with_runner(mut self, runner: impl crate::capture::AppRunner + 'static) -> Self {
        self.runner = Some(Box::new(runner));
        self
    }

    /// Returns a `GraphBuilder` for manual DAG node construction.
    ///
    /// Panics if the graph is already finalized.
    pub fn graph_builder(&mut self) -> crate::graph::GraphBuilder<'_> {
        assert!(
            !self.graph.is_active(),
            "graph is already finalized — call graph_builder before finalize_graph"
        );
        self.graph.builder()
    }

    /// Finalizes the graph after manual DAG construction via `graph_builder()`.
    pub fn finalize_graph(&mut self) {
        self.graph.finalize();
    }

    /// Mutable access to the arena for widget construction.
    pub fn arena_mut(&mut self) -> &mut StateArena {
        &mut self.arena
    }

    /// Immutable access to the arena (for reading).
    pub fn arena(&self) -> &StateArena {
        &self.arena
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
        for input in inputs {
            if let Some(gui) = &mut self.gui {
                gui.on_event(input, fb_size, &mut self.arena);
            }
        }
        self.tick_dag(dt, elapsed, cursor, fb_size);
        self.arena.clear_dirty();
    }

    fn tick_dag(&mut self, dt: f32, elapsed: f32, cursor: Vec2, fb_size: Vec2u) {
        if self.graph.is_active() {
            {
                let src = &self.graph.source;
                src.framebuffer_size.write(&mut self.arena, fb_size);
                src.mouse_pos.write(&mut self.arena, cursor);
                src.delta_time.write(&mut self.arena, dt);
                src.elapsed.write(&mut self.arena, elapsed);
                src.mouse_down.write(&mut self.arena, false);
                src.scroll_delta.write(&mut self.arena, 0.0);
            }
            self.graph.tick(&mut self.arena, None);
        }
        // Let the runner inspect state after the tick.
        if let Some(ref mut runner) = self.runner {
            let ctx = crate::capture::AppContext {
                arena: &self.arena,
                graph: &self.graph,
                gui: self.gui.as_ref(),
            };
            runner.after_tick(&ctx);
        }
    }

    /// Starts the winit event loop. `text_system` renders all text objects
    /// produced by DAG nodes and the GUI in a second pass above the render
    /// objects.
    ///
    /// Consumes the app; the loop runs until the window is closed (or a
    /// runner returns `true` from `after_render`).
    pub fn run(mut self, text_system: TextSystem) {
        self.text_system = Some(text_system);
        self.run_impl()
    }

    fn run_impl(self) {
        let mut builder = winit::event_loop::EventLoop::builder();
        // Visual snapshot tests run on a test thread; winit's default
        // main-thread check would panic there. any_thread is a no-op
        // when the loop is created on the main thread anyway.
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
            arena,
            graph,
            pending_render_slots,
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
            arena,
            graph,
            last_instant: Instant::now(),
            elapsed: 0.0,
            mouse_down: false,
            scroll_delta: 0.0,
            pending_render_slots,
        };
        event_loop.run_app(&mut app).unwrap();
    }
}

struct FrameApp {
    title: String,
    gui: Option<Gui>,
    text_system: TextSystem,
    clear_color: wgpu::Color,
    window: Option<Arc<Window>>,
    gpu_setup: Option<gpu::Setup>,
    canvas: Option<Canvas>,
    cursor: Vec2,
    arena: StateArena,
    graph: Graph,
    last_instant: Instant,
    elapsed: f32,
    mouse_down: bool,
    scroll_delta: f32,
    pending_render_slots: Vec<PendingRenderSlot>,
    runner: Option<Box<dyn crate::capture::AppRunner>>,
}

impl FrameApp {
    fn tick_dag(&mut self, gpu: Option<&sm::Gpu>) {
        if !self.graph.is_active() {
            return;
        }
        let dt = self.last_instant.elapsed().as_secs_f32();
        self.last_instant = Instant::now();
        self.elapsed += dt;

        let fb_size = self.canvas.as_ref().unwrap().framebuffer_size();
        {
            let src = &self.graph.source;
            src.framebuffer_size.write(&mut self.arena, fb_size);
            src.mouse_pos.write(&mut self.arena, self.cursor);
            src.delta_time.write(&mut self.arena, dt);
            src.elapsed.write(&mut self.arena, self.elapsed);
            src.mouse_down.write(&mut self.arena, self.mouse_down);
            src.scroll_delta.write(&mut self.arena, self.scroll_delta);
        }
        self.scroll_delta = 0.0;

        self.graph.tick(&mut self.arena, gpu);

        if let Some(ref mut runner) = self.runner {
            let ctx = crate::capture::AppContext {
                arena: &self.arena,
                graph: &self.graph,
                gui: self.gui.as_ref(),
            };
            runner.after_tick(&ctx);
        }
    }

    fn handle_input(&mut self, event: &WindowEvent) {
        // Track the cursor so mouse-button events (which carry no position
        // in winit) can be located.
        if let WindowEvent::CursorMoved { position, .. } = event {
            self.cursor = Vec2::new(position.x as f32, position.y as f32);
        }
        // Capture DAG source port state from raw winit events (mouse
        // button and scroll delta aren't tracked by the GUI event system).
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
        // Forward input to the GUI.
        match event {
            WindowEvent::CursorMoved { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
            | WindowEvent::KeyboardInput { .. } => {
                if let Some(gui) = &mut self.gui {
                    if let Some(input) = InputEvent::from_winit(event, self.cursor) {
                        let size = self.window.as_ref().unwrap().inner_size();
                        gui.on_event(&input, Vec2u::new(size.width, size.height), &mut self.arena);
                        self.window.as_ref().unwrap().request_redraw();
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                // No further MouseMoves arrive once the cursor leaves the
                // window; synthesize one far outside so every widget clears
                // its hover state.
                if let Some(gui) = &mut self.gui {
                    let size = self.window.as_ref().unwrap().inner_size();
                    gui.on_event(
                        &InputEvent::MouseMove {
                            pos: Vec2::new(-10000.0, -10000.0),
                            pressure: None,
                        },
                        Vec2u::new(size.width, size.height),
                        &mut self.arena,
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

        // Collect DAG render output (fills/outlines/texts written by nodes).
        // Owned copies — arena is mutably borrowed during GUI render below.
        let dag_fills = self
            .arena
            .read::<Vec<RectEntry>>(self.graph.render.fills.id())
            .clone();
        let dag_outlines = self
            .arena
            .read::<Vec<RectEntry>>(self.graph.render.outlines.id())
            .clone();
        let dag_texts = self
            .arena
            .read::<Vec<TextObject>>(self.graph.render.texts.id())
            .clone();

        // Text collection: queue DAG-produced text objects, then let the GUI
        // queue its own text through the render walk.
        self.text_system.clear();
        for t in dag_texts {
            self.text_system.queue(t.clone());
        }
        if let Some(gui) = &self.gui {
            gui.render_with_dag(
                &gpu_setup.gpu,
                canvas,
                &mut self.text_system,
                &mut self.arena,
                &dag_fills,
                &dag_outlines,
            );
        } else {
            // No GUI — draw DAG fills/outlines directly through canvas.
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
        canvas.render(&gpu_setup.gpu, &mut encoder, &view, &mut self.arena);
        self.arena.clear_dirty();

        // Text pass: shared depth texture, loaded (not cleared) so
        // text depth-tests against the objects' depth.
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

        // Hook: let the runner inspect or capture the rendered frame.
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

impl ApplicationHandler for FrameApp {
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
        // Apply deferred render slot registrations.
        for pending in self.pending_render_slots.drain(..) {
            canvas.add_render_slot(crate::canvas::RenderSlot {
                material_port: pending.material_port,
                gpu_buffer_port: pending.gpu_buffer_port,
                bind_group_port: pending.bind_group_port,
                constant_port: pending.constant_port,
                has_fb_in_push_constant: pending.has_fb_in_push_constant,
                material_slot: None,
                register_material: pending.register_material,
                read_push_constant: pending.read_push_constant,
            });
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
