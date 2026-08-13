//! Convenience re-exports of the most commonly used types in shame-gui.
//!
//! Add `use shame_gui::prelude::*;` to bring the entire public API into scope
//! in one line. For more selective imports, use the individual module paths
//! (e.g. `use shame_gui::app::App`).

pub use crate::app::App;
pub use crate::capture::{AppContext, AppRunner, FrameOutput, read_frame_rgba};
pub use crate::color::Color;
pub use crate::graph::{
    AppState, BuiltinState, DagStruct, DagStructRef, Graph, MapEntry, Port, PortGroup, PortId,
    PortValue, RenderPorts, SourcePorts,
};
pub use crate::gui::{
    AnyWidget, EventResponse, Gui, InputEvent, Key, MouseButton, RenderContext, SplitDir,
    ViewportNode, ViewportTree, Widget, WidgetData, WidgetNode, event_pos,
};
pub use crate::instance::GpuStruct;
pub use crate::material::Material;
pub use crate::math::{Vec2, Vec2i, Vec2u, Vec4};
pub use crate::rect::Rect;
pub use crate::shader::{RectEntry, RectMaterial, WireframeMaterial};
pub use crate::text::{TextObject, TextSystem};
pub use shame_gui_derive::{DagStruct, GpuStruct};
