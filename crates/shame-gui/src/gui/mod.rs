//! Widget/viewport layer on top of the render framework: a tree of splits
//! and widget nodes where widgets are struct editors (see `Widget`).
//!
//! ## Structure
//!
//! - [`viewport`] — the [`ViewportNode`] tree and its driver [`Gui`]:
//!   splits, tabs, containers, and the right-click context menu.
//! - [`widget`] — the [`Widget`] trait (how a value is rendered/edited),
//!   [`WidgetNode`] (handle + cached state), and [`RenderContext`].
//! - [`event`] — [`InputEvent`], [`Key`], [`MouseButton`], [`EventResponse`].
//! - [`layout`] — the per-frame rect computation helpers (table/split/tab).
//! - [`style`] — shared visual constants (colors, sizes, z-layers).
//! - [`primitives`] — [`Widget`] implementations for primitive types
//!   (numbers, strings, bools, `Vec2`).

pub mod event;
pub mod layout;
pub mod primitives;
pub mod style;
pub mod viewport;
pub mod widget;

pub use event::{EventResponse, InputEvent, Key, MouseButton, RoutingMode, event_pos};
pub use shame_gui_derive::Widget;
pub use viewport::{Gui, SplitDir, SplitNode, ViewportNode, ViewportTree};
pub use widget::{AnyWidget, RenderContext, Widget, WidgetData, WidgetNode};
