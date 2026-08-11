<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://readme-typing-svg.demolab.com?font=Fira+Code&weight=500&size=28&duration=2500&pause=800&color=60A5FA&center=false&vCenter=true&width=435&lines=shame-gui">
  <img alt="shame-gui" src="https://readme-typing-svg.demolab.com?font=Fira+Code&weight=500&size=28&duration=2500&pause=800&color=2563EB&center=false&vCenter=true&width=435&lines=shame-gui">
</picture>

**A GPU-accelerated 2D GUI framework where shaders, widgets, and dataflow are all defined in Rust.**

Define your state as a struct. Derive a widget tree from it. Wire up a DAG compute graph. Write custom GPU shaders — in Rust, not GLSL. shame-gui builds the bridge from your data model to the screen, pixel by pixel.

---

## The idea

Most GUI frameworks force you into a renderer's fixed pipeline. shame-gui inverts this:

```
    ┌──────────┐     dirty ports     ┌──────────┐
    │  Widgets  │ ──────────────────▶│   DAG    │
    │  (edit)   │                    │ (compute)│
    └──────────┘                    └──────────┘
         ▲                                │
         │       arena reads              │  arena writes
         └────────────────────────────────┘
```

1. **Define state** — a plain Rust struct with `#[derive(DagStruct, Widget)]`
2. **Build a DAG** — nodes read input ports, compute outputs, write back. No rendering code.
3. **Widgets render the state** — the GUI reads arena values and draws them. Edit a value → port marked dirty → DAG recomputes → updated output appears.

Every custom shader is written in [**shame**](https://github.com/windwhiterain/shame), an embedded DSL that compiles Rust expressions directly to WGSL — no string templates, no separate shader files.

---

## Quick start

```rust
use shame_gui::app::App;
use shame_gui::graph::{StateArena, build_state_ports};
use shame_gui::gui::{Gui, ViewportNode, ViewportTree};
use shame_gui::math::Vec2;
use shame_gui::text::TextSystem;
use shame_gui_derive::{DagStruct, Widget};

// 1. Define your state — widget tree is derived automatically
#[derive(Clone, Default, DagStruct, Widget)]
struct CalcState {
    operand1: f32,
    operand2: f32,
    vec_a: Vec2,
    vec_b: Vec2,
    sum: f32,   // computed by DAG
    dot: f32,   // computed by DAG
}

fn main() {
    let mut app = App::new("Calculator");
    let state = CalcState {
        operand1: 12.0, operand2: 34.0,
        vec_a: Vec2::new(3.0, 4.0), vec_b: Vec2::new(5.0, 6.0),
        ..Default::default()
    };

    // 2. Allocate state ports in the arena
    let ports = build_state_ports(&state, app.arena_mut());

    // 3. Build the DAG — compute outputs from inputs
    {
        let mut b = app.graph_builder();
            use shame_gui::graph::IdGroup;
            b.add_node(
                {
                    let p = ports.clone();
                    move |arena: &mut StateArena, _gpu: Option<&shame_gui::sm::Gpu>| {
                        let a = *p.operand1.read(arena);
                        let b = *p.operand2.read(arena);
                        p.sum.write(arena, a + b);
                        p.dot.write(arena, a * b);
                    }
                },
                (p.operand1, p.operand2),
                (p.sum, p.dot),
            );
    }
    app.finalize_graph();

    // 4. Build widget tree from the same ports — one editor per field
    let tree = ViewportTree::new(ViewportNode::container(
        state.into_viewport_nodes(app.arena_mut(), Some(ports)),
    ));
    app.add_gui(Gui::new(tree));

    app.run(TextSystem::new());
}
```

```sh
cargo run -p shame-gui --example calc
```

---

## Architecture

| Layer | What it does |
|-------|-------------|
| **DAG engine** (`graph/`) | Typed ports, arena storage, dirty tracking, topological execution |
| **Widget tree** (`gui/`) | Taffy layout, viewport splits/tabs, inline editing widgets |
| **Canvas** (`canvas.rs`) | Per-material batching via arena ports; DAG upload nodes produce GPU buffers |
| **Shaders** (`shader/`) | Built-in rect + wireframe; custom materials via the shame EDSL |
| **Text** (`text.rs`) | glyphon 0.11 + cosmic-text 0.18, shares depth buffer with rect pass |
| **App** (`app.rs`) | winit event loop, wires GUI events → DAG tick → render |

### Write GPU shaders in Rust

The built-in `RectMaterial` and `WireframeMaterial` are defined in pure Rust using the shame EDSL. No WGSL strings, no separate `.wgsl` files:

```rust
// Defining a custom gradient shader (excerpt)
let gradient_material = GradientRectMaterial;
let pipeline = {
    let mut encoder: sm::PipelineEncoder<sm::pipeline_kind::Render> =
        gpu.create_pipeline_encoder(Default::default()).unwrap();
    let mut drawcall = encoder.new_render_pipeline(sm::Indexing::BufferU32);
    let instance = instances.index(drawcall.vertices.instance_index);
    let position = instance.rect.pos.extend(instance.z);
    let color = instance.color_a;  // vertex attributes → fragment shader
    drawcall.vertices.assemble(position, sm::Draw::triangle_list(sm::Winding::Cw))
        .rasterize(Default::default())
        .fill(color);
    // ...
};
```

### How widgets work

A widget is anything that implements the `Widget` trait — read from arena, render into a rect, handle input events. Leaf widgets ship for all common types:

| Widget type | Binds to |
|------------|----------|
| Number field | `Port<f32>`, `Port<u32>`, `Port<i32>`, `Port<usize>` |
| String field | `Port<String>` |
| Vec2 editor | `Vec2Ports` |
| Bool toggle | `Port<bool>` |

Composite widgets are generated by `#[derive(Widget)]` — the struct's fields become labelled editors laid out with Taffy.

### DAG + Widget = reactive dataflow

When a user edits a number field and presses Enter:

1. Widget writes the new value into the arena
2. `walk_event` collects the dirty `PortId`
3. `graph.mark_dirty()` marks the port
4. `graph.tick()` runs only the DAG nodes whose inputs are dirty
5. Output ports update in the arena
6. Next frame, widgets read the new values and render them

No polling. No event buses. Just data flowing through typed Rust ports.

---

## Examples

| Example | Description |
|---------|------------|
| `calc` | State-driven calculator: DAG computes outputs, GUI displays/edits everything |
| `form` | Login form + settings tabs, table-mode layout with Taffy |
| `rects` | Custom gradient material, widget fills/outlines |
| `text` | Text rendering with glyphon, custom styling |
| `push_constant` | Custom material with typed push constants |

```sh
cargo run -p shame-gui --example calc
cargo run -p shame-gui --example form
cargo run -p shame-gui --example rects
cargo run -p shame-gui --example text
```

---

## Development

```sh
# Check + format (run before committing)
python check.py

# Run all tests (snapshot tests included — no feature flag needed)
cargo test -p shame-gui

# Build a single example
cargo check -p shame-gui --example calc
```

**Requirements**: Rust stable, Windows/Mac/Linux with a GPU supporting wgpu 29.

The [shame EDSL](https://github.com/windwhiterain/shame) crates are pulled from a sibling workspace at `../shame` or from git (tag `v2.0.0-beta.2`).

---

## License

MIT
