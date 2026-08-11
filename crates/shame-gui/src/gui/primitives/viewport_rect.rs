//! A passive [`Widget`] for `Port<ViewportRect, S>` that captures the layout
//! [`Rect`] assigned by the viewport and writes it into the state.
//!
//! Place a `ViewportRect` field in your state and connect its port to a DAG
//! node. During each frame's render walk the widget writes the pixel-space
//! rectangle it receives into the state. Downstream DAG nodes can then read
//! the captured rect and, for example, pass it as a push constant to a custom
//! material.

use crate::graph::DagStructRef;
use crate::graph::port::Port;
use crate::gui::event::{EventResponse, InputEvent};
use crate::gui::widget::{RenderContext, Widget, WidgetData};
use crate::rect::Rect;

/// A value type that captures the layout rectangle. Derive [`DagStruct`] to
/// generate `ViewportRectPorts`; `Port<ViewportRect, S>` implements
/// [`Widget<S>`] so the assigned pixel-space [`Rect`] is written into the
/// state each frame.
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

impl<S: 'static> Widget<S> for Port<ViewportRect, S> {
    type Data = ViewportRectData;

    fn layout_style(&self, _state: &DagStructRef<S>, _data: &Self::Data) -> taffy::Style {
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
        state: &mut DagStructRef<S>,
        _data: &Self::Data,
        rect: Rect,
        _ctx: &mut RenderContext,
    ) {
        self.write(state, ViewportRect { rect });
    }

    fn on_event(
        &self,
        _state: &mut DagStructRef<S>,
        _data: &mut Self::Data,
        _event: &InputEvent,
        _rect: Rect,
    ) -> EventResponse {
        EventResponse::Ignored
    }
}
