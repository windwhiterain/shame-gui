//! # shame-gui
//!
//! A 2D immediate-mode GUI framework built on [wgpu] and the [shame] Rust shader EDSL.
//!
//! ## Architecture
//!
//! ```text
//! App (window + event loop)
//!  |-- State S (a #[derive(DagStruct)] struct; typed Port handles access its fields)
//!  |-- DAG graph (dataflow: source ports -> compute nodes -> render ports)
//!  `-- Viewport tree (split/tab/container layout -> WidgetNodes -> Widget trait)
//!       `-- Canvas (material registry -> one draw call per material per frame -> GPU)
//! ```
//!
//! ## Quick start
//!
//! ```rust,no_run
#![doc = include_str!("../examples/calc.rs")]
//! ```
//!
//! ## Key concepts
//!
//! - **Ports, State, DAG** — values live in a plain state struct `S`
//!   (the global app state, or a collection element); typed `Port<D, S>`
//!   handles read/write its fields through monomorphized accessors (no
//!   downcasts, no runtime store). The DAG engine (`Graph`) runs nodes when
//!   their input ports are marked dirty (by widget edits or source-port
//!   updates). See the [`graph`] module.
//! - **Widgets and Viewport** — The [`Widget`] trait defines how a value is rendered and edited.
//!   `ViewportNode` builds a tree of splits, tabs, containers, and widget leaves.
//!   See the [`gui`] module.
//! - **Materials and rendering** — Custom rendering through [`Material`](crate::material::Material)
//!   (shader + pipeline). Register via [`App::register_render_object`](crate::app::App::register_render_object)
//!   with a material port, typed [`InstanceBuffer`](crate::material::InstanceBuffer) port, and push constant port.
//!   See the [`material`] and [`app`] modules.
//! - **Derive macros** — `#[derive(GpuStruct)]`, `#[derive(DagStruct)]`, and `#[derive(Widget)]`
//!   generate GPU twins, port groups, and viewport-construction methods. See the [`instance`] module
//!   and the [`graph`] module.
//!
//! ## Modules
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`app`] | `App` — entry point, window loop, object management |
//! | [`graph`] | DAG engine: `Port`, `DagStruct`, `Graph`, `BuiltinState` |
//! | [`gui`] | Widget/viewport layer: `Widget` trait, `ViewportNode` tree, `Gui` |
//! | [`math`] | GPU-compatible vector types: `Vec2`, `Vec4`, `Vec2u`, `Vec2i` |
//! | [`rect`] | `Rect` — position + size with pixel→NDC conversion |
//! | [`color`] | `Color` — sRGB authoring; `to_linear()` for GPU linear space |
//! | [`shader`] | Built-in materials: `RectMaterial`, `WireframeMaterial` |
//! | [`text`] | Glyphon-based text: `TextObject`, `TextSystem` |
//! | [`material`] | `Material` trait for custom shading |
//! | [`instance`] | `GpuStruct` trait — GPU twin, WGSL layout, serialization |
//! | [`capture`] | `AppRunner` trait, frame readback for tests |
//! | [`prelude`] | Convenience re-exports of common types |
//!
//! [shame]: https://github.com/windwhiterain/shame
//! [wgpu]: https://wgpu.rs

// Lets derive-generated code reference `::shame_gui::...` from inside this crate.
extern crate self as shame_gui;

#[doc(hidden)]
pub use bytemuck;
#[doc(hidden)]
pub use shame_wgpu as sm;
#[doc(hidden)]
pub use taffy;

pub mod app;
pub mod buffer_pool;
mod canvas;
pub mod capture;
pub mod color;
pub mod dual;
pub mod gpu;
pub mod graph;
pub mod gui;
pub mod instance;
pub mod material;
pub mod math;
pub mod prelude;
pub mod rect;
pub mod shader;
pub mod text;

// Derive macros — re-exported at the crate root so generated code can reference
// `::shame_gui::GpuStruct` etc.
pub use shame_gui_derive::DagStruct;
pub use shame_gui_derive::GpuStruct;
pub use shame_gui_derive::Widget;
pub use shame_gui_derive::state;

// Convenience re-exports of the most-used types. For a complete set, see the
// [`prelude`] module.
pub use crate::color::Color;
pub use crate::instance::GpuStruct;
pub use crate::math::{Vec2, Vec2i, Vec2u, Vec4};
