//! [`Widget`](crate::gui::Widget) implementations for primitive types.
//!
//! Each module implements `Widget` for a `Port<T>` handle and exposes its
//! cached-interaction-state struct (the `Widget::Data` type the `Widget`
//! derive pairs with fields of that type):
//!
//! - `bool` module — `Port<bool>` checkboxes;
//! - [`number`] — `Port<f32>` / `Port<u32>` numeric editors;
//! - [`string`] — `Port<String>` text editors;
//! - [`vec2`] — `Vec2Ports` two-field editors;
//! - [`viewport_rect`] — `ViewportRectPorts` passive layout-rect capture.

pub mod bool;
pub mod number;
pub mod string;
pub mod vec2;
pub mod viewport_rect;
