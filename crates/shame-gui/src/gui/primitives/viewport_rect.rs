//! A passive [`Widget`] that captures the layout [`Rect`] assigned by the
//! viewport and writes it into DAG ports.
//!
//! Place a `ViewportRect` in your viewport tree and connect its ports to a
//! DAG node. During each frame's render walk the widget writes the pixel-space
//! rectangle it receives into the arena via the port group's [`.write()`] method.
//! Downstream DAG nodes can then call [`.read()`] on the same port group to
//! reconstruct the `Rect` and, for example, pass it as a push constant to a
//! custom material via [`App::register_render_object`](crate::App::register_render_object).
//!
//! ```ignore
//! #[derive(DagStruct, Widget)]
//! struct MyState {
//!     viewport: ViewportRect,
//! }
//! ```

use crate::graph::StateArena;
use crate::gui::event::{EventResponse, InputEvent};
use crate::gui::widget::{RenderContext, Widget, WidgetData};
use crate::rect::Rect;

/// A value type that captures the layout rectangle. Derive [`DagStruct`] to
/// generate `ViewportRectPorts` — that port group implements [`Widget`] so
/// that during each render walk the assigned pixel-space [`Rect`] is written
/// into the arena.
#[derive(Clone, Default, shame_gui_derive::DagStruct)]
pub struct ViewportRect {
    /// The captured rectangle (initial value ignored; overwritten each frame).
    pub rect: Rect,
}

/// Cached state for `ViewportRect` — no interaction state is needed.
pub struct ViewportRectData;

impl WidgetData for ViewportRectData {}

impl Default for ViewportRectData {
    fn default() -> Self {
        Self
    }
}

impl Widget for ViewportRectPorts {
    type Data = ViewportRectData;

    fn layout_style(&self, _arena: &StateArena, _data: &Self::Data) -> taffy::Style {
        taffy::Style {
            min_size: taffy::Size {
                width: taffy::Dimension::auto(),
                height: taffy::Dimension::auto(),
            },
            ..Default::default()
        }
    }

    fn render(
        &self,
        arena: &mut StateArena,
        _data: &Self::Data,
        rect: Rect,
        _ctx: &mut RenderContext,
    ) {
        // Write the layout-assigned rect into the arena via the port group.
        // This decomposes the Rect (pos, size) into the individual f32 ports.
        self.rect.write(arena, rect);
    }

    fn on_event(
        &self,
        _arena: &mut StateArena,
        _data: &mut Self::Data,
        _event: &InputEvent,
        _rect: Rect,
    ) -> EventResponse {
        EventResponse::Ignored
    }
}
