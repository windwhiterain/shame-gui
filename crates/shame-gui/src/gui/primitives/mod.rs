//! [`Widget`](crate::gui::Widget) implementations for primitive types.
//!
//! Each module implements `Widget<S>` for `Port<T, S>` handles and exposes
//! the cached-interaction-state struct (the `Widget::Data` type the `Widget`
//! derive pairs with fields of that type):
//!
//! - `bool` module — `Port<bool, S>` checkboxes;
//! - [`number`] — `Port<f32, S>` / `Port<u32, S>` / `Port<i32, S>` /
//!   `Port<usize, S>` numeric editors;
//! - [`string`] — `Port<String, S>` text editors;
//! - [`vec2`] — `Port<Vec2, S>` two-field editors;
//! - [`viewport_rect`] — `Port<ViewportRect, S>` passive layout-rect capture.

pub mod bool;
pub mod number;
pub mod string;
pub mod vec2;
pub mod viewport_rect;
