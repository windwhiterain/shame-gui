//! A nested-struct [`Widget`] for `Port<T, S>`: renders `T`'s own field
//! rows (from `T::into_viewport_nodes()`) as a labelled table inside the
//! parent's row — the plain-field counterpart of the map widget's expanded
//! element form. Works to arbitrary depth: a nested field may itself contain
//! nested fields (the same blanket impl recurses) or maps.
//!
//! Dirty boundaries: the widget borrows the field through
//! [`Port::with_field_ref`](crate::graph::Port::with_field_ref), which tracks
//! sub-writes into a **shared per-field record** and marks the field port
//! only when a sub-widget actually wrote — rendering and layout never wake
//! the DAG, so a clean tick stays clean. Any sub-write (including one inside
//! a nested map) wakes as a "whole field changed" mark on the field port for
//! plain `add_node` readers; a render tree whose path crosses the field
//! ([`FieldPath`](crate::graph::FieldPath)) consumes the same shared record
//! and reprocesses exactly the changed leaf.

use std::cell::RefCell;

use crate::graph::DagStructRef;
use crate::graph::port::{Port, PortValue};
use crate::gui::event::{EventResponse, InputEvent, RoutingMode, event_pos};
use crate::gui::style;
use crate::gui::viewport::layout::container_table_rects;
use crate::gui::viewport::node::ViewportNode;
use crate::gui::viewport::render::{walk_event, walk_render};
use crate::gui::widget::{RenderContext, Widget, WidgetData, WidgetElement};
use crate::math::Vec2;
use crate::rect::Rect;
use crate::text::TextObject;

/// Cached state for a nested-struct field widget.
pub struct FieldWidgetData<T> {
    /// The labelled child-widget template built from `T::into_viewport_nodes()`.
    pub template: Vec<(String, ViewportNode<T>)>,
    /// The content height measured at the last render; `layout_style` reads
    /// this so the assigned rect matches the real content (falls back to
    /// [`super::template_height`] before the first render).
    pub height: RefCell<f32>,
    /// The WidgetNode id inside the nested table holding keyboard focus.
    pub sub_focus: Option<u64>,
}

impl<T: 'static> WidgetData for FieldWidgetData<T> {}

impl<T: WidgetElement> Default for FieldWidgetData<T> {
    fn default() -> Self {
        let template = T::into_viewport_nodes();
        Self {
            height: RefCell::new(super::template_height(&template)),
            template,
            sub_focus: None,
        }
    }
}

impl<S: 'static, T: WidgetElement + PortValue> Widget<S> for Port<T, S> {
    type Data = FieldWidgetData<T>;

    fn layout_style(&self, _state: &DagStructRef<S>, data: &Self::Data) -> taffy::Style {
        taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Column,
            min_size: taffy::Size {
                width: taffy::Dimension::auto(),
                height: taffy::Dimension::length(*data.height.borrow()),
            },
            ..Default::default()
        }
    }

    fn render(
        &self,
        state: &mut DagStructRef<S>,
        data: &Self::Data,
        rect: Rect,
        ctx: &mut RenderContext,
    ) {
        self.with_field_ref(state, |fref| {
            if let Some(rows) = container_table_rects(&data.template, rect, fref) {
                for ((label, child), (label_rect, editor_rect)) in
                    data.template.iter().zip(rows.iter())
                {
                    ctx.texts.push(
                        TextObject::new(label.clone())
                            .at_position(
                                label_rect.pos + Vec2::new(style::LABEL_PAD_X, style::LABEL_PAD_Y),
                            )
                            .with_font_size(style::LABEL_FONT_SIZE)
                            .with_color(style::LABEL_TEXT_COLOR)
                            .with_z(style::Z_TEXT),
                    );
                    walk_render(child, *editor_rect, ctx, fref);
                }
                *data.height.borrow_mut() = measure_height(&rows, rect);
            }
        });
    }

    fn on_event(
        &self,
        state: &mut DagStructRef<S>,
        data: &mut Self::Data,
        event: &InputEvent,
        rect: Rect,
    ) -> EventResponse {
        let visit_all = event.routing() == RoutingMode::Broadcast;
        if !visit_all && event_pos(event).is_some_and(|p| !rect.contains(p)) {
            return EventResponse::Ignored;
        }
        self.with_field_ref(state, |fref| {
            let mut consumed = false;
            if let Some(rows) = container_table_rects(&data.template, rect, fref) {
                for ((_, child), (_, editor_rect)) in data.template.iter_mut().zip(rows.iter()) {
                    let r = walk_event(child, *editor_rect, event, &mut data.sub_focus, fref);
                    if r == EventResponse::Consumed && !visit_all {
                        consumed = true;
                        break;
                    }
                }
            }
            if consumed {
                EventResponse::Consumed
            } else {
                EventResponse::Ignored
            }
        })
    }

    fn selectable(&self) -> bool {
        true
    }
}

/// The content height of the nested table: the last row's editor bottom
/// minus the widget top, plus the container's bottom padding.
fn measure_height(rows: &[(Rect, Rect)], rect: Rect) -> f32 {
    let last = &rows[rows.len() - 1];
    (last.1.pos.y + last.1.size.y) - rect.pos.y + style::FIELD_PADDING
}
