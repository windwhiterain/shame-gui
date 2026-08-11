//! A checkbox [`Widget`] for `Port<bool, S>`.

use crate::graph::DagStructRef;
use crate::graph::port::Port;
use crate::gui::event::{EventResponse, InputEvent, MouseButton};
use crate::gui::style;
use crate::gui::widget::{RenderContext, Widget, WidgetData};
use crate::math::Vec2;
use crate::rect::Rect;
use crate::shader::RectEntry;

/// Cached state for a checkbox.
pub struct BoolWidgetData {
    /// Whether the cursor is currently over the box.
    pub hovered: bool,
}

impl WidgetData for BoolWidgetData {}

impl Default for BoolWidgetData {
    fn default() -> Self {
        Self { hovered: false }
    }
}

impl<S: 'static> Widget<S> for Port<bool, S> {
    type Data = BoolWidgetData;

    fn layout_style(&self, _state: &DagStructRef<S>, _data: &Self::Data) -> taffy::Style {
        taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Row,
            min_size: taffy::Size {
                width: taffy::Dimension::length(24.0),
                height: taffy::Dimension::length(24.0),
            },
            ..Default::default()
        }
    }

    fn render(
        &self,
        state: &mut DagStructRef<S>,
        _data: &Self::Data,
        rect: Rect,
        ctx: &mut RenderContext,
    ) {
        let checked = *self.read(state);
        let box_rect = Rect::new(rect.pos, Vec2::new(18.0, 18.0));
        ctx.outlines.push(RectEntry {
            rect: box_rect,
            color: style::BORDER.to_linear(),
            z: style::Z_BORDER,
        });
        if checked {
            let inner = Rect::new(rect.pos + Vec2::new(4.0, 4.0), Vec2::new(10.0, 10.0));
            ctx.fills.push(RectEntry {
                rect: inner,
                color: style::CHECK_ON.to_linear(),
                z: style::Z_PANEL,
            });
        }
    }

    fn on_event(
        &self,
        state: &mut DagStructRef<S>,
        data: &mut Self::Data,
        event: &InputEvent,
        rect: Rect,
    ) -> EventResponse {
        match event {
            InputEvent::MouseMove { pos, .. } => {
                data.hovered = rect.contains(*pos);
                EventResponse::Consumed
            }
            InputEvent::MouseDown { pos, button, .. } => {
                if !rect.contains(*pos) {
                    return EventResponse::Ignored;
                }
                if *button == MouseButton::Left {
                    let new_val = !*self.read(state);
                    self.write(state, new_val);
                }
                EventResponse::Consumed
            }
            _ => EventResponse::Ignored,
        }
    }
}
