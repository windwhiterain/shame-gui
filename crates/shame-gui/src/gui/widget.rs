//! The [`Widget`] trait and its viewport integration: [`WidgetNode`]
//! (handle + cached state), the type-erased [`AnyWidget`], and the per-frame
//! [`RenderContext`].

use std::any::Any;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::graph::DagStructRef;
use crate::gui::event::{EventResponse, InputEvent};
use crate::gui::viewport::node::ViewportNode;
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

/// A widget is a UI handle for viewing / editing a value stored in a state
/// `S`. `Self` is the handle (e.g. `Port<f32, S>`); the value lives in the
/// state and is accessed via `port.read(&state)` / `port.write(&mut state, v)`
/// through a [`DagStructRef`]. `Data` holds cached interaction state.
pub trait Widget<S>: 'static + Clone {
    /// Cached interaction state, paired with the widget by the derive.
    type Data: WidgetData;

    /// The widget's desired layout behavior (size, padding).
    fn layout_style(&self, state: &DagStructRef<S>, data: &Self::Data) -> taffy::Style;

    /// Pushes instance data into `ctx`. Reads the current value from `state`.
    /// Colors must be **linear**.
    fn render(
        &self,
        state: &mut DagStructRef<S>,
        data: &Self::Data,
        rect: Rect,
        ctx: &mut RenderContext,
    );

    /// Handles one input event within `rect`. `Consumed` stops routing.
    /// Writes value changes into `state` (which also marks dirty).
    fn on_event(
        &self,
        state: &mut DagStructRef<S>,
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

/// A `#[derive(Widget)]` struct usable as the element type of a
/// [`Port<HashMap<String, T>, S>`](crate::graph::Port) map widget: provides
/// the per-field child-widget template rendered when an entry is expanded.
///
/// Implemented automatically by the `Widget` derive (the same method it
/// generates as an inherent `into_viewport_nodes()`); the trait form is how
/// the map widget builds its template generically.
pub trait WidgetElement: Sized + 'static {
    /// The labelled child-widget template for this struct, one node per
    /// non-`#[widget(skip)]` field.
    fn into_viewport_nodes() -> Vec<(String, ViewportNode<Self>)>;
}

/// Type-erased widget handle + data pair. `render`/`on_event` are
/// monomorphized fn pointers over the concrete widget type.
///
/// `data` is `UnsafeCell`-backed because `render`/`layout_style` read it
/// through `&self` while `on_event` writes it through `&self`; the read and
/// write borrows never coexist. This is sound only under the GUI's
/// non-reentrant, single-threaded walk — a widget callback must never
/// dispatch events recursively, and a widget must not be shared across
/// threads.
pub struct AnyWidget<S> {
    widget: Box<dyn Any>,
    data: UnsafeCell<Box<dyn Any>>,
    render: fn(&dyn Any, &dyn Any, &mut DagStructRef<S>, Rect, &mut RenderContext<'_>),
    event: for<'a, 'b, 'c> fn(
        &'a dyn Any,
        &'b mut dyn Any,
        &'c mut DagStructRef<S>,
        &'c InputEvent,
        Rect,
    ) -> EventResponse,
    layout: fn(&dyn Any, &dyn Any, &DagStructRef<S>) -> taffy::Style,
    selectable: bool,
    duplicate_widget: fn(&dyn Any) -> Box<dyn Any>,
    default_data: fn() -> Box<dyn Any>,
}

impl<S> AnyWidget<S> {
    /// Type-erases a widget handle + its cached state.
    pub fn new<W: Widget<S>>(widget: W, data: W::Data) -> Self
    where
        W::Data: Default,
    {
        let selectable = widget.selectable();
        let render: fn(&dyn Any, &dyn Any, &mut DagStructRef<S>, Rect, &mut RenderContext<'_>) =
            |w, d, state, rect, ctx| {
                <W as Widget<S>>::render(
                    w.downcast_ref::<W>().unwrap(),
                    state,
                    d.downcast_ref::<W::Data>().unwrap(),
                    rect,
                    ctx,
                )
            };
        let event: for<'a, 'b, 'c> fn(
            &'a dyn Any,
            &'b mut dyn Any,
            &'c mut DagStructRef<S>,
            &'c InputEvent,
            Rect,
        ) -> EventResponse = |w, d, state, input, rect| {
            <W as Widget<S>>::on_event(
                w.downcast_ref::<W>().unwrap(),
                state,
                d.downcast_mut::<W::Data>().unwrap(),
                input,
                rect,
            )
        };
        let layout: fn(&dyn Any, &dyn Any, &DagStructRef<S>) -> taffy::Style = |w, d, state| {
            <W as Widget<S>>::layout_style(
                w.downcast_ref::<W>().unwrap(),
                state,
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

    pub(crate) fn render(
        &self,
        state: &mut DagStructRef<S>,
        rect: Rect,
        ctx: &mut RenderContext<'_>,
    ) {
        let data = unsafe { &*self.data.get() };
        (self.render)(self.widget.as_ref(), data.as_ref(), state, rect, ctx)
    }

    pub(crate) fn on_event(
        &self,
        state: &mut DagStructRef<S>,
        event: &InputEvent,
        rect: Rect,
    ) -> EventResponse {
        let data = unsafe { &mut *self.data.get() };
        (self.event)(self.widget.as_ref(), data.as_mut(), state, event, rect)
    }

    pub(crate) fn selectable(&self) -> bool {
        self.selectable
    }

    pub(crate) fn layout_style(&self, state: &DagStructRef<S>) -> taffy::Style {
        let data = unsafe { &*self.data.get() };
        (self.layout)(self.widget.as_ref(), data.as_ref(), state)
    }

    pub(crate) fn duplicate(&self) -> AnyWidget<S> {
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
/// A `WidgetNode` wraps a widget handle (e.g. `Port<f32, S>`) plus its cached
/// interaction state ([`Widget::Data`]). It is the leaf of the viewport
/// tree — wrap it in [`ViewportNode::Widget`](crate::gui::ViewportNode::Widget)
/// to place it in the tree.
pub struct WidgetNode<S> {
    pub(crate) widget: AnyWidget<S>,
    pub(crate) id: u64,
}

static NODE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

impl<S> WidgetNode<S> {
    /// Creates a node with explicit cached state.
    pub fn new<W: Widget<S>>(widget: W, data: W::Data) -> Self
    where
        W::Data: Default,
    {
        Self {
            widget: AnyWidget::new(widget, data),
            id: NODE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Creates a node with `Data::default()` cached state.
    pub fn new_default<W: Widget<S>>(widget: W) -> Self
    where
        W::Data: Default,
    {
        Self::new(widget, W::Data::default())
    }

    /// The node's unique id (used for focus tracking).
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Clones the widget handle with fresh `Data::default()` state — the same
    /// port accessors are shared, so the duplicate edits the same value.
    #[must_use]
    pub fn duplicate(&self) -> Self {
        Self {
            widget: self.widget.duplicate(),
            id: NODE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
        }
    }
}
