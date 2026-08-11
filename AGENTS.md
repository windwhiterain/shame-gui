# AGENTS.md

## Project state

- `shame-gui`: a 2D GUI framework for wgpu, built on the **shame** Rust EDSL. Being rebuilt from scratch on the `rebuild` branch (orphan, root commit `864b223`). **`master` holds the old codebase** — ignore it; the working tree on `rebuild` is the only truth.
- `report.md` at the repo root describes the **old** (`master`) codebase — ignore it.
- The `shame` crates live in a **sibling workspace** at `../shame`. Read its `examples/api_showcase` and `examples/hello_triangles` before guessing shame APIs.

## Workspace layout

- `crates/shame-gui/src/app.rs` — public API: `App::new(title).add_gui(gui).run(ts)`. Exposes `arena_mut()` / `arena()` for widget construction against the app's StateArena. Also `graph_builder()` + `finalize_graph()` for DAG construction — this is the only graph-building path; use it when the caller needs to hold port handles for GUI widget binding. `register_render_object::<M>(material, cpu_buffer, constant)` registers a material + typed instance buffer + push constant for dirty-managed GPU drawing. Two internal DAG nodes are created: an upload node (`cpu_buffer` → wgpu buffer) and a bind group node (`gpu_buffer` + `material` → bind group). Canvas reads material, gpu buffer, bind group, and constant from arena ports each frame.
- `crates/shame-gui/src/canvas.rs` — `pub(crate)`: material registry, one draw call per material per frame. Takes an external `CommandEncoder` (does NOT submit).
- `crates/shame-gui/src/material.rs` — `Material`, `InstanceBuffer`, `GpuBufferSlot`, `MakeBindGroupFn`.
- `crates/shame-gui/src/instance.rs` + `crates/shame_gui_derive` — `#[derive(GpuStruct)]`: GPU twin, layout, serialize, bind group.
- `crates/shame-gui/src/math.rs` — `Vec2`, `Vec4`, `Vec2u`, `Vec2i`, `Rect`. `#[repr(C)]`, `Pod`/`Zeroable`. Derive `DagStruct` to generate `{Name}Ports` port groups.
- `crates/shame-gui/src/color.rs` — `Color` (sRGB, CPU-only, NOT `GpuStruct`). Use `to_linear()` in place when building instance data.
- `crates/shame-gui/src/shader/` — `RectMaterial` / `WireframeMaterial`. Batched via `InstanceBuffer<RectInstance>` + fast-path `Canvas::add_instance()` = one draw call per material.
- `crates/shame-gui/src/text.rs` — **glyphon 0.11 + cosmic-text 0.18**. Must stay glyphon 0.11 (last compatible with wgpu 29). Text renders in a second pass sharing canvas depth.
- `crates/shame-gui/src/capture.rs` — `AppRunner` trait (test extension point), `FrameOutput`, `read_frame_rgba` (GPU readback utility).
- `crates/shame-gui/tests/common/scenes.rs` — demo scenes, shared by snapshot tests and examples.
- `crates/shame-gui/examples/calc.rs` — reference state-driven DAG + GUI: `#[derive(DagStruct, Widget)]` on the state struct, `build_state_ports()` to allocate ports, manual DAG via `app.graph_builder()`, GUI via `into_viewport_nodes(arena, Some(ports))`. Demonstrates the full round-trip: edit → dirty → DAG → render.

## Commands (PowerShell, from repo root)

- `python check.py` — cargo check + `cargo fix --allow-dirty` + cargo fmt. Run before committing.
- `cargo test -p shame-gui` — all tests including pixel-exact snapshot tests (no feature flag needed). Each scene is its own test binary (winit `EventLoop` limitation). `actual.png` must match `expected.png`; missing goldens auto-accept. Only commit `expected.png`.
- never run `cargo clippy`.
- Do not run tests or `cargo build` from repo root — use `-p shame-gui`.

## Deliberate design decisions — do not "fix"

- **No error handling**: no `Result`/`Error`; unwrap/expect. Panic-fast.
- **No blending, no z-sorting**: `Depth24Plus` with `less_equal`. **Smaller z = closer**. Shared across rect + text passes.
- **Physical pixel coords, y-down**. `Rect::to_ndc(fb: Vec2u)` is the only NDC conversion.
- `Canvas.render()` reads per-slot data from arena ports (material, gpu buffer, bind group, constant). Render slots do NOT own GPU buffers — they are produced by DAG upload/bind nodes and stored in arena ports as `Option<Arc<GpuBufferSlot>>` / `Option<Arc<wgpu::BindGroup>>`. Fast-path (widget fills/outlines) still uses `Canvas::add_instance()` + `Canvas::set_push_constant()` directly.
- `Material` requires `Eq + Hash + Clone`; registration dedups by value. One pipeline + draw call per material.
- **`#[derive(Widget)]` does NOT generate a Widget impl** — it generates `into_viewport_nodes(self, arena, ports: Option<{Name}Ports>) -> Vec<(String, ViewportNode)>` and `into_tab_nodes(...) -> Vec<(String, Vec<(String, ViewportNode)>)>`. Each WidgetNode is wrapped in `ViewportNode::Widget(...)`. When `ports` is `Some`, seed arena with initial values via `write_ports` and wire port IDs for dirty tracking. When `None`, allocates standalone arena slots.
- **`#[derive(Widget)]` requires `#[derive(DagStruct)]`** on the same struct (like `Copy: Clone`). The Widget derive unconditionally references `<Self as DagStruct>::Ports`; if DagStruct is missing the compiler will point you at it.
- **`into_viewport_nodes()` consumes `self`** — fields are moved into WidgetNodes backed by arena slots.
- `Gui.focus: Option<u64>` — direct WidgetNode ID focus. No indirection.
- `Canvas` is `pub(crate)` — don't expose it or draw helpers.

## `#[derive(GpuStruct)]` contract

- One trait, one derive. `align(16)` = instance struct (full serialize, real `make_bindings`); no `align(16)` = field type (flat `bytes_of`, `Gpu = sm::Struct<NameGpu>`).
- Wire size differs: instance → div-ceil-16; field → max member alignment.
- Derive also generates `Pod` + `Zeroable` — do NOT hand-write them.
- `serialize` pads grow-only to wire-size boundary. `out.resize(offset, 0)` silently drops trailing instances.
- `Canvas.clear()` empties fast-path CPU buffers (capacity retained) but skips slots backed by render slot registrations; GPU buffer grow-only.

## text / taffy / winit gotchas

- glyphon `prepare_with_depth` per-glyph z via `Attrs::new().metadata(area_index)`.
- **taffy 0.13**: `Dimension` is a struct (not an enum) — construct with `length(f32)`, `auto()`, `percent(f32)`. Cannot pattern-match; use `is_auto()` / `value()` to query. `Style` needs `..Default::default()`.
- **taffy 0.13 widget layout**: all primitive widgets put their size in `min_size` (`width: Length(80)`, `height: Length(24)`), NOT in `size` (which defaults to `Auto`). When computing a composite ViewportNode's size, read both fields: prefer `size` over `min_size`.
- taffy `Layout::location` is relative to parent — sum ancestors for absolute position.
- winit 0.30: `MouseInput` has no cursor position; `Char` events skip control chars (Backspace is `\u{8}` → fall through to named-key match).
- `crate::math::Vec2` has `x`/`y` only — no `width`/`height`.

## Viewport tree (`gui/viewport/`)

**`ViewportNode` variants:**
- `Widget(WidgetNode)` — leaf: a single widget.
- `Split(SplitNode { dir, ratio, children: [Box<ViewportNode>; 2] })` — two children divided by a draggable divider.
- `Tab(TabNode { tabs: Vec<(String, ViewportNode)>, active: usize })` — tabbed panels, one visible at a time.
- `Container(Vec<(String, ViewportNode)>)` — labelled children in a vertical table layout (taffy). **Holds `ViewportNode` directly, not `WidgetNode`** — nesting works without an escape hatch.

**There is no `sub_viewport` on `WidgetNode`** — it was removed. Nest any node directly via `ViewportNode` variants.

**Layout is hybrid, not unified:**
- **Container**: builds a per-frame taffy tree via `container_table_rects`. Children's `layout_style()` is consulted here.
- **Split / Tab**: pure pixel arithmetic — `split_rects` / `tab_content_rect`. `layout_style()` is NOT called during render/event walks; it's only used when the Split/Tab is a child of a Container.
- **Widget**: renders into whatever `rect` it receives. Its `layout_style()` is only a hint for parent Containers.

**Margin indentation (8px per nesting level):**
- Split nodes have a margin matching their axis: horizontal split → top margin, vertical split → left margin. Use `split_margin_rect(rect, dir)`.
- Tab nodes have **no margin** (tab bar provides clickability).
- Container nodes have **no margin** (labels provide clickability).
- Margins are visual clickable strips for right-click context menus.

**Right-click context menu:** Right-click any `ViewportNode` → "Split Horizontal" / "Split Vertical" popup. Click the item to wrap the node in a `SplitNode` with a duplicate sharing the same `port_ids`. `find_context_target` traverses the tree with `container_table_rects` to find the deepest node under the cursor.

**`execute_split` uses `ptr::read` + `ptr::write`** to move the old node out and write the new SplitNode in without a temporary — this avoids needing `Default` on `ViewportNode`.

**`ViewportNode::layout_style(&self, arena) -> taffy::Style`:** Recursive size computation used by parent Containers. Split: combines children's dimensions (horizontal: widths sum, heights max; vertical: opposite) plus `INDENT` on the margin axis. Tab: active child + tab bar height.

## DAG system (`graph/`)

**Core abstractions:**
- `PortValue: Clone + Default + 'static` — types that flow through ports (f32, u32, bool, String, Vec2, Rect, Vec<RectEntry>, etc.).
- `PortId(pub u64)` — dense index into `StateArena` slots.
- `StateArena` (`arena.rs`) — central typed value storage. `alloc::<T>()` / `alloc_with(v)` create slots; `read::<T>(id) -> &T` / `write(id, v)` access them. Slots are never deleted.
- `Port<T>` — thin `Copy` handle (holds `PortId`). `read(arena)` / `write(arena, value)`. Multiple `Port<T>` handles can reference the same arena slot — "connection" is just sharing an ID. Manual `Clone`/`Copy` impls (not derived — derive adds unnecessary `T: Copy` bound).

**Traits:**
- `PortGroup` — implemented by every port (single or composite). `leaf_count()`, `port_ids()`, `set_port_ids(&[PortId])`, `alloc_slots(arena) -> Self`.
  - `Port<T>` is a trivial 1-leaf PortGroup.
  - Composite types get `{Name}Ports` structs via `#[derive(DagStruct)]` with recursive delegation.
- `DagStruct: PortValue` — maps a value type to its port group. `type Ports: PortGroup`, `fn write_ports(&self, arena, ports: &Self::Ports)`, `fn read_ports(arena, ports) -> Self` (static, reconstructs the value from arena ports — inverse of `write_ports`).
  - Primitives: `type Ports = Port<T>` (manual impls in `port.rs`).
  - Structs: `type Ports = {Name}Ports` (generated by `#[derive(DagStruct)]`).
  - `#[derive(DagStruct)]` also auto-implements `PortValue` for the struct.

**Build flow (`GraphBuilder`):**
- `builder.port_with(arena, value)` — allocates an arena slot with initial value, returns `Port<T>`.
- `builder.port(arena)` — allocates with `Default::default()`.
- `builder.connect(&src, &mut dst)` — copies `PortId` (not pointer sharing — both reference the same arena slot).
- `builder.connect_group(&src, &mut dst)` — positional ID copy (leaf 0→0, 1→1, …). Both must have same `leaf_count()`.
- `builder.add_node(eval, inputs, outputs)` — registers a node. `eval: FnMut(&mut StateArena, Option<&sm::Gpu>)`. `inputs` are the port IDs this node reads from (dirty-triggers for tick). Nodes capture `Port<T>` handles (Copy) and read/write through the arena. `gpu` is `Some` during render frames so upload nodes can create wgpu buffers.
- `builder.source()` / `builder.render()` — access built-in source/render ports.

**Auto-build ports from state:**
- `build_state_ports(&state, arena) -> State::Ports` — free function in `port.rs`. Allocates arena slots for every leaf port, writes initial values via `write_ports`.

**Manual DAG construction (the only graph-building path):**
- Use `app.graph_builder()` for DAG construction. Typical flow:
  1. `let ports = build_state_ports(&state, app.arena_mut());`
  2. `{ let mut b = app.graph_builder(); b.add_node(...); }`
  3. `app.finalize_graph();`
  4. Build GUI with `state.into_viewport_nodes(arena, Some(ports))`.
- Sequential borrows: `arena_mut()` and `graph_builder()` must be called one at a time (they borrow different fields of `App` but Rust can't see that through method calls).

**Execution (`Graph::tick(arena, gpu: Option<&sm::Gpu>)`):**
- Graph does NOT own the arena — `tick()` takes `&mut StateArena`.
- `gpu` is `Some` during render frames (GPU upload nodes need it) and `None` for CPU-only ticks (tests and `App::step()`).
- Dirty-triggered evaluation: when a port is marked dirty, only nodes reading from it run. Nodes with empty inputs always run.

**Widget → DAG bridge:**
- `Gui::on_event()` returns `Vec<PortId>` — dirty port IDs from widgets whose value changed.
- `app.rs` drains these into `graph.mark_dirty(id)` before calling `graph.tick(&mut arena)`.

**DAG-produced custom render objects:**

Custom materials (beyond built-in fills/outlines/texts) go through DAG ports:
1. Define a struct implementing `Material`.
2. Build instance data into a typed `InstanceBuffer<M::Instance>` and write it into arena ports (e.g. via a DAG node).
3. Pass `(material: Port<M>, cpu_buffer: Port<InstanceBuffer<M::Instance>>, constant: Port<M::PushConstant>)` to `App::register_render_object`.
4. Two internal DAG nodes are created: an upload node reads `cpu_buffer` and creates a wgpu buffer (written to a `gpu_buffer` port), and a bind group node reads `gpu_buffer` + `material` and creates a bind group (written to a `bind_group` port).
5. Canvas reads material, gpu buffer, bind group, and constant from arena ports each frame.
6. Arena dirty flags are **only** consumed by Canvas (`arena.clear_dirty()` in `canvas.render()`). The graph has its own separate dirty-triggered evaluation — `graph.tick()` does NOT touch `arena.dirty`.
7. `Canvas` is `pub(crate)` — the public API is `App::register_render_object()`.

Example (see `tests/common/scenes.rs` `rects_scene`):
```rust
let mut ib = InstanceBuffer::<GradientRectInstance>::new();
ib.push(&GradientRectInstance { rect, color_a, color_b, z });

let material_port: Port<GradientRectMaterial> =
    Port::new(app.arena_mut().alloc_with(GradientRectMaterial));
let cpu_buffer_port: Port<InstanceBuffer<GradientRectInstance>> =
    Port::new(app.arena_mut().alloc_with(ib));
let constant_port: Port<()> = Port::new(app.arena_mut().alloc::<()>());
app.register_render_object(material_port, cpu_buffer_port, constant_port);
```

## Widget layer (`gui/widget.rs`)

- **`Widget: 'static + Clone`** — widget handles must be clonable (they are `Port<T>` or `Vec2Ports`, all `Copy`/`Clone`). `Clone` is required for `WidgetNode::duplicate()`.
- `Widget` trait: `type Data: WidgetData`, `fn layout_style`, `fn render`, `fn on_event`.
  - `self` is the widget handle (e.g. `Port<f32>`, `Vec2Ports`, `ViewportRectPorts`), NOT the value. Read/write values through `arena`.
  - **`render` receives `&mut StateArena`** — passive widgets (e.g. `ViewportRectPorts`) can write into the arena during the render walk, not only during events.
- `RenderContext { fills, outlines, texts, framebuffer }` — widgets push `RectEntry`/`TextObject` into these vecs. **No `add_render_object()`** — custom rendering goes through DAG + `App::register_render_object()`.
- **`ViewportRectPorts`** (`primitives/viewport_rect.rs`) — passive widget: writes the layout-assigned [`Rect`] into DAG ports each frame via `write_ports`. Zero-size, invisible, ignores all events. Use it to feed GUI layout rects into DAG push constants.
- `WidgetData` holds cached interaction state (buffer, cursor). Lives in `AnyWidget`, accessed via `UnsafeCell`.
- `WidgetNode` holds `widget: AnyWidget`, `id: u64`, `port_ids: Vec<PortId>`. **No `sub_viewport`** — use `ViewportNode` variants for nesting.
- `WidgetNode::duplicate()` — clones the widget handle (same `port_ids`, same arena slots), creates fresh `WidgetData::default()`.
- `AnyWidget` stores `duplicate_widget` / `default_data` fn pointers for type-erased cloning.

## Testing quirks

- `App::step()` — test helper for frame-by-frame DAG + widget simulation without a winit loop.
- Snapshot tests (`tests/snapshot_*.rs`) each run in their own process. No feature flag needed.
- `Gui.drag` tests use `InputEvent::from_winit` + `Gui::on_event` directly.
- For widget unit tests, create a `StateArena`, allocate `Port<T>` handles, then call `port.on_event(&mut arena, &mut data, ...)`.
- Scene functions (like `form_scene()`) must use `app.arena_mut()` for widget construction — creating a separate local arena will cause type mismatch crashes at render time (port IDs reference wrong arena).

## shame EDSL gotchas

- Comparisons are methods: `x.lt(y)`, `x.le(y)` — `x < y` does not compile.
- `shame_wgpu::bind_group!` generates `{Name}Resources` struct.
- Generated code uses `::shame_gui::...` and `shame::__private::...` — user crates must depend on `shame` directly.
- Reference EDSL: `crates/shame-gui/src/shader/rect.rs`.
