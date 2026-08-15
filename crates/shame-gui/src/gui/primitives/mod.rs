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
//! - [`viewport_rect`] — `Port<ViewportRect, S>` passive layout-rect capture;
//! - [`field`] — `Port<T, S>` nested struct editors (`T: WidgetElement`):
//!   renders `T`'s own field rows as a labelled table;
//! - [`map`] — `Port<HashMap<String, T>, S>` collapsible key-value lists
//!   (`T: WidgetElement`);
//! - [`selector`] — `MapSelector`: a dropdown editing a `Port<String, S>`
//!   key value constrained to the keys of a `Port<HashMap<String, T>, S>`
//!   (a manual handle — it binds two ports).

pub mod bool;
pub mod field;
pub mod map;
pub mod number;
pub mod selector;
pub mod string;
pub mod vec2;
pub mod viewport_rect;

use crate::gui::style;
use crate::gui::viewport::node::ViewportNode;

/// Fallback content height for a labelled widget template (map elements,
/// nested struct fields): every row at the primitive row height, plus the
/// container's gaps and padding. Exact for uniform templates; replaced by
/// measured heights once rendered.
pub(crate) fn template_height<T>(template: &[(String, ViewportNode<T>)]) -> f32 {
    let n = template.len();
    if n == 0 {
        0.0
    } else {
        n as f32 * style::MAP_ROW_H + (n - 1) as f32 * style::FIELD_GAP + 2.0 * style::FIELD_PADDING
    }
}
