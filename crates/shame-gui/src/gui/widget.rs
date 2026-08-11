//! The [`Widget`] trait and its viewport integration: [`WidgetNode`]
//! (handle + cached state), the type-erased [`AnyWidget`], and the per-frame
//! [`RenderContext`].

use std::any::Any;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::graph::StateArena;
use crate::gui::event::{EventResponse, InputEvent};
use crate::math::Vec2u;
use crate::rect::Rect;
use crate::shader::RectEntry;
use crate::text::TextObject;

/// The per-frame render context passed to every widget. Widgets push
/// instance data into `fills`/`outlines`/`texts`. The GUI draws one batch
/// per material afterwards.
pub struct RenderContext<'a> {
    /// Filled rects produced this frame (batched into one draw call).
    pub fills: &'a mut Vec<RectEntry>,
    /// Outlined rects produced this frame.
    pub outlines: &'a mut Vec<RectEntry>,
    /// Text objects produced this frame (queued into the text system).
    pub texts: &'a mut Vec<TextObject>,
    /// The framebuffer size in physical pixels.
    pub framebuffer: Vec2u,
}

/// A widget is a UI handle for viewing / editing a value stored in the
/// StateArena. `Self` is the handle (e.g. `Port<f32>`); the value lives
/// in the arena and is accessed via `arena.read()` / `arena.write()`.
/// `Data` holds cached interaction state (cursor position, editing buffer).
pub trait Widget: 'static + Clone {
    /// Cached interaction state, paired with the widget by the derive.
    type Data: WidgetData;

    /// The widget's desired layout behavior (size, padding).
    fn layout_style(&self, arena: &StateArena, data: &Self::Data) -> taffy::Style;

    /// Pushes instance data into `ctx`. Reads current value from `arena`.
    /// Colors must be **linear**.
    fn render(
        &self,
        arena: &mut StateArena,
        data: &Self::Data,
        rect: Rect,
        ctx: &mut RenderContext,
    );

    /// Handles one input event within `rect`. `Consumed` stops routing.
    /// Writes value changes into `arena` (which also marks dirty).
    fn on_event(
        &self,
        arena: &mut StateArena,
        data: &mut Self::Data,
        event: &InputEvent,
        rect: Rect,
    ) -> EventResponse;

    /// Whether this widget can claim focus (receive keyboard events).
    fn selectable(&self) -> bool {
        false
    }
}

/// Marker for the cached-state struct paired with a widget.
pub trait WidgetData: 'static {}

/// Type-erased widget handle + data pair. `render`/`on_event` are
/// monomorphized fn pointers over the concrete widget type.
pub struct AnyWidget {
    widget: Box<dyn Any>,
    data: UnsafeCell<Box<dyn Any>>,
    render: fn(&dyn Any, &dyn Any, &mut StateArena, Rect, &mut RenderContext<'_>),
    event: for<'a, 'b, 'c> fn(
        &'a dyn Any,
        &'b mut dyn Any,
        &'c mut StateArena,
        &'c InputEvent,
        Rect,
    ) -> EventResponse,
    layout: fn(&dyn Any, &dyn Any, &StateArena) -> taffy::Style,
    selectable: bool,
    duplicate_widget: fn(&dyn Any) -> Box<dyn Any>,
    default_data: fn() -> Box<dyn Any>,
}

impl AnyWidget {
    /// Type-erases a widget handle + its cached state.
    ///
    /// The `Data` is boxed behind `UnsafeCell` (mutated during event
    /// routing); `render`/`event`/`layout`/`duplicate` are monomorphized
    /// fn pointers over the concrete widget type.
    pub fn new<W: Widget>(widget: W, data: W::Data) -> Self
    where
        W::Data: Default,
    {
        let selectable = widget.selectable();
        let render: fn(&dyn Any, &dyn Any, &mut StateArena, Rect, &mut RenderContext<'_>) =
            |w, d, arena, rect, ctx| {
                <W as Widget>::render(
                    w.downcast_ref::<W>().unwrap(),
                    arena,
                    d.downcast_ref::<W::Data>().unwrap(),
                    rect,
                    ctx,
                )
            };
        let event: for<'a, 'b, 'c> fn(
            &'a dyn Any,
            &'b mut dyn Any,
            &'c mut StateArena,
            &'c InputEvent,
            Rect,
        ) -> EventResponse = |w, d, arena, input, rect| {
            <W as Widget>::on_event(
                w.downcast_ref::<W>().unwrap(),
                arena,
                d.downcast_mut::<W::Data>().unwrap(),
                input,
                rect,
            )
        };
        let layout: fn(&dyn Any, &dyn Any, &StateArena) -> taffy::Style = |w, d, arena| {
            <W as Widget>::layout_style(
                w.downcast_ref::<W>().unwrap(),
                arena,
                d.downcast_ref::<W::Data>().unwrap(),
            )
        };
        let duplicate_widget: fn(&dyn Any) -> Box<dyn Any> =
            |w| Box::new(w.downcast_ref::<W>().unwrap().clone());
        let default_data: fn() -> Box<dyn Any> = || Box::<W::Data>::default();
        Self {
            widget: Box::new(widget),
            data: UnsafeCell::new(Box::new(data)),
            render,
            event,
            layout,
            selectable,
            duplicate_widget,
            default_data,
        }
    }

    pub(crate) fn render(&self, arena: &mut StateArena, rect: Rect, ctx: &mut RenderContext<'_>) {
        let data = unsafe { &*self.data.get() };
        (self.render)(self.widget.as_ref(), data.as_ref(), arena, rect, ctx)
    }

    pub(crate) fn on_event(
        &self,
        arena: &mut StateArena,
        event: &InputEvent,
        rect: Rect,
    ) -> EventResponse {
        let data = unsafe { &mut *self.data.get() };
        (self.event)(self.widget.as_ref(), data.as_mut(), arena, event, rect)
    }

    pub(crate) fn selectable(&self) -> bool {
        self.selectable
    }

    pub(crate) fn layout_style(&self, arena: &StateArena) -> taffy::Style {
        let data = unsafe { &*self.data.get() };
        (self.layout)(self.widget.as_ref(), data.as_ref(), arena)
    }

    pub(crate) fn duplicate(&self) -> AnyWidget {
        AnyWidget {
            widget: (self.duplicate_widget)(self.widget.as_ref()),
            data: UnsafeCell::new((self.default_data)()),
            render: self.render,
            event: self.event,
            layout: self.layout,
            selectable: self.selectable,
            duplicate_widget: self.duplicate_widget,
            default_data: self.default_data,
        }
    }
}

/// One widget in the viewport tree, with its paired data.
///
/// A `WidgetNode` wraps a widget handle (e.g. `Port<f32>`) plus its cached
/// interaction state ([`Widget::Data`]). It is the leaf of the viewport
/// tree — wrap it in [`ViewportNode::Widget`](crate::gui::ViewportNode::Widget)
/// to place it in the tree.
pub struct WidgetNode {
    pub(crate) widget: AnyWidget,
    pub(crate) id: u64,
    pub(crate) port_ids: Vec<crate::graph::PortId>,
}

static NODE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

impl WidgetNode {
    /// Creates a node with explicit cached state.
    pub fn new<W: Widget>(widget: W, data: W::Data) -> Self
    where
        W::Data: Default,
    {
        Self {
            widget: AnyWidget::new(widget, data),
            id: NODE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            port_ids: Vec::new(),
        }
    }

    /// Creates a node with `Data::default()` cached state.
    pub fn new_default<W: Widget>(widget: W) -> Self
    where
        W::Data: Default,
    {
        Self::new(widget, W::Data::default())
    }

    /// Records the arena slot ids this widget edits. Used by the `Widget`
    /// derive to wire dirty tracking from widget edits into the DAG; also
    /// used by `ViewportNode` duplication.
    #[must_use]
    pub fn with_port_ids(mut self, ids: Vec<crate::graph::PortId>) -> Self {
        self.port_ids = ids;
        self
    }

    /// The node's unique id (used for focus tracking).
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Clones the widget handle (same arena slots) with fresh `Data::default()`
    /// state — the same port_ids are shared, so the duplicate edits the same
    /// values. Used by `ViewportNode`'s split/duplicate operations.
    #[must_use]
    pub fn duplicate(&self) -> Self {
        Self {
            widget: self.widget.duplicate(),
            id: NODE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            port_ids: self.port_ids.clone(),
        }
    }
}
