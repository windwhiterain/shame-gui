use crate::canvas::Canvas;
use crate::graph::DagStructRef;
use crate::gui::event::{EventResponse, InputEvent, Key, RoutingMode, event_pos};
use crate::gui::style;
use crate::gui::widget::RenderContext;
use crate::math::{Vec2, Vec2u};
use crate::rect::Rect;
use crate::shader::{RectEntry, RectInstance, RectMaterial, ViewportParams, WireframeMaterial};
use crate::sm;
use crate::text::TextObject;

use super::layout::{
    container_table_rects, divider_hit, divider_rect, effective_ratio, split_margin_rect,
    split_ratio_range, split_rects, tab_bar_rect, tab_content_rect,
};
use super::node::{SplitDir, ViewportNode};

/// Drag state while the user is pulling a split divider.
pub struct DragState {
    pub split_rect: Rect,
    pub dir: SplitDir,
    pub start_ratio: f32,
    pub start_pos: Vec2,
}

// ── Render walk ─────────────────────────────────────────────────────────

/// Walks the tree collecting instance data: widgets submit fill rects,
/// outlines and text; splits emit their divider; tabbed nodes emit their
/// header rects and text labels.
pub fn walk_render<S>(
    node: &ViewportNode<S>,
    rect: Rect,
    ctx: &mut RenderContext<'_>,
    state: &mut DagStructRef<S>,
) {
    match node {
        ViewportNode::Widget(widget) => {
            render_node_outline(ctx, rect);
            widget.widget.render(state, rect, ctx);
        }
        ViewportNode::Split(split) => {
            render_node_outline(ctx, rect);
            let inner = split_margin_rect(rect, split.dir);
            ctx.fills.push(RectEntry {
                rect: divider_rect(split, inner, state),
                color: style::DIVIDER.to_linear(),
                z: style::Z_DIVIDER,
            });
            let (r0, r1) = split_rects(split, inner, state);
            walk_render(&split.children[0], r0, ctx, state);
            walk_render(&split.children[1], r1, ctx, state);
        }
        ViewportNode::Tab(tab) => {
            render_node_outline(ctx, rect);
            let count = tab.tabs.len();
            if count == 0 {
                return;
            }
            let active = tab.active.min(count - 1);
            let bar_rect = tab_bar_rect(rect);
            let header_w = bar_rect.size.x / count as f32;
            for (index, (name, _)) in tab.tabs.iter().enumerate() {
                let header_rect = Rect::new(
                    bar_rect.pos + Vec2::new(header_w * index as f32, 0.0),
                    Vec2::new(header_w, style::TAB_BAR),
                );
                let color = if index == active {
                    style::TAB_ACTIVE
                } else {
                    style::TAB_INACTIVE
                };
                ctx.fills.push(RectEntry {
                    rect: header_rect,
                    color: color.to_linear(),
                    z: style::Z_PANEL,
                });
                let text_color = if index == active {
                    style::TAB_ACTIVE_TEXT
                } else {
                    style::TAB_INACTIVE_TEXT
                };
                ctx.texts.push(
                    TextObject::new(name.clone())
                        .at_position(
                            header_rect.pos + Vec2::new(style::TAB_PAD_X, style::TAB_PAD_Y),
                        )
                        .with_font_size(style::TAB_FONT_SIZE)
                        .with_color(text_color)
                        .with_z(style::Z_TEXT),
                );
            }
            walk_render(&tab.tabs[active].1, tab_content_rect(rect), ctx, state);
        }
        ViewportNode::Container(children) => {
            render_node_outline(ctx, rect);
            let Some(rows) = container_table_rects(children, rect, state) else {
                return;
            };
            for ((label, child), (label_rect, editor_rect)) in children.iter().zip(rows.iter()) {
                ctx.texts.push(
                    TextObject::new(label.clone())
                        .at_position(
                            label_rect.pos + Vec2::new(style::LABEL_PAD_X, style::LABEL_PAD_Y),
                        )
                        .with_font_size(style::LABEL_FONT_SIZE)
                        .with_color(style::LABEL_TEXT_COLOR)
                        .with_z(style::Z_TEXT),
                );
                walk_render(child, *editor_rect, ctx, state);
            }
        }
    }
}

// ── Event walk ──────────────────────────────────────────────────────────

/// Routes one event through the tree. The `focus` parameter tracks which
/// WidgetNode is currently focused (set on MouseDown, used for keyboard
/// routing).
pub fn walk_event<S>(
    node: &mut ViewportNode<S>,
    rect: Rect,
    event: &InputEvent,
    focus: &mut Option<u64>,
    state: &mut DagStructRef<S>,
) -> EventResponse {
    let visit_all = event.routing() == RoutingMode::Broadcast;
    let pos = event_pos(event).unwrap_or(Vec2::new(0.0, 0.0));
    let is_focused = event.routing() == RoutingMode::Focused;
    match node {
        ViewportNode::Widget(widget) => {
            // Focused events (keyboard/char): only route to the focused
            // WidgetNode — and skip the hit test, since key events carry no
            // meaningful position.
            if is_focused && *focus != Some(widget.id) {
                return EventResponse::Ignored;
            }
            if !is_focused && !visit_all && !rect.contains(pos) {
                return EventResponse::Ignored;
            }
            // MouseDown on a selectable WidgetNode claims focus.
            if let InputEvent::MouseDown { .. } = event {
                if widget.widget.selectable() {
                    *focus = Some(widget.id);
                }
            }
            let response = widget.widget.on_event(state, event, rect);
            if response == EventResponse::Consumed && !visit_all {
                return EventResponse::Consumed;
            }
            EventResponse::Ignored
        }
        ViewportNode::Split(split) => {
            let inner = split_margin_rect(rect, split.dir);
            let (r0, r1) = split_rects(split, inner, state);
            if walk_event(&mut split.children[0], r0, event, focus, state)
                == EventResponse::Consumed
                && !visit_all
            {
                return EventResponse::Consumed;
            }
            walk_event(&mut split.children[1], r1, event, focus, state)
        }
        ViewportNode::Tab(tab) => {
            if tab.tabs.is_empty() {
                return EventResponse::Ignored;
            }
            let active = tab.active.min(tab.tabs.len() - 1);
            let bar_rect = tab_bar_rect(rect);
            // A press on the header bar switches the active tab.
            if let InputEvent::MouseDown { pos, .. } = event {
                if bar_rect.contains(*pos) {
                    let index = ((pos.x - bar_rect.pos.x)
                        / (bar_rect.size.x / tab.tabs.len() as f32))
                        as usize;
                    tab.active = index.min(tab.tabs.len() - 1);
                    return EventResponse::Consumed;
                }
            }
            walk_event(
                &mut tab.tabs[active].1,
                tab_content_rect(rect),
                event,
                focus,
                state,
            )
        }
        ViewportNode::Container(children) => {
            if children.is_empty() {
                return EventResponse::Ignored;
            }
            let is_focused = event.routing() == RoutingMode::Focused;
            let Some(rows) = container_table_rects(children, rect, state) else {
                return EventResponse::Ignored;
            };
            if is_focused {
                // Keyboard/char events: route through every row's editor rect
                // (walk_event's widget branch picks the focused node; the
                // position hit test is skipped for focused events). This also
                // reaches widgets nested inside split/tab/container children.
                for ((_, child), (_, editor_rect)) in children.iter_mut().zip(rows.iter()) {
                    if walk_event(child, *editor_rect, event, focus, state)
                        == EventResponse::Consumed
                    {
                        return EventResponse::Consumed;
                    }
                }
                return EventResponse::Ignored;
            }
            if event.routing() == RoutingMode::Broadcast {
                for ((_, child), (_, editor_rect)) in children.iter_mut().zip(rows.iter()) {
                    if let ViewportNode::Widget(w) = child {
                        w.widget.on_event(state, event, *editor_rect);
                    }
                }
                return EventResponse::Consumed;
            }
            for ((_, child), (_, editor_rect)) in children.iter_mut().zip(rows.iter()) {
                if !editor_rect.contains(pos) {
                    continue;
                }
                if let ViewportNode::Widget(w) = child {
                    if let InputEvent::MouseDown { .. } = event {
                        if w.widget.selectable() {
                            *focus = Some(w.id);
                        }
                    }
                    let response = w.widget.on_event(state, event, *editor_rect);
                    if response == EventResponse::Consumed {
                        return EventResponse::Consumed;
                    }
                } else {
                    let response = walk_event(child, *editor_rect, event, focus, state);
                    if response == EventResponse::Consumed && !visit_all {
                        return EventResponse::Consumed;
                    }
                }
                break;
            }
            EventResponse::Ignored
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn _maybe_dirty(_event: &InputEvent) -> bool {
    matches!(
        _event,
        InputEvent::MouseDown { .. }
            | InputEvent::KeyDown {
                key: Key::Enter,
                ..
            }
    )
}

/// Pushes a 1px wireframe outline around `rect` to make the node's bounds visible.
fn render_node_outline(ctx: &mut RenderContext<'_>, rect: Rect) {
    ctx.outlines.push(RectEntry {
        rect,
        color: style::NODE_OUTLINE.to_linear(),
        z: style::Z_NODE_OUTLINE,
    });
}

// ── Batch draw ──────────────────────────────────────────────────────────

/// Draws the collected fills and outlines each as a single batch: one
/// material registration + one draw call each.
pub fn batch_draw<S>(
    gpu: &sm::Gpu,
    canvas: &mut Canvas<S>,
    framebuffer: Vec2u,
    fills: Vec<RectEntry>,
    outlines: Vec<RectEntry>,
) {
    let vp = ViewportParams {
        fb_size: framebuffer,
    };
    if !fills.is_empty() {
        let handle = canvas.register_material(gpu, RectMaterial);
        canvas.set_push_constant(&handle, &vp);
        for entry in &fills {
            canvas.add_instance(
                &handle,
                &RectInstance {
                    rect: entry.rect,
                    color: entry.color,
                    z: entry.z,
                },
            );
        }
    }
    if !outlines.is_empty() {
        let handle = canvas.register_material(gpu, WireframeMaterial);
        canvas.set_push_constant(&handle, &vp);
        for entry in &outlines {
            canvas.add_instance(
                &handle,
                &RectInstance {
                    rect: entry.rect,
                    color: entry.color,
                    z: entry.z,
                },
            );
        }
    }
}

// ── Drag ────────────────────────────────────────────────────────────────

/// Finds the first split whose divider is under `pos` (walk order: outer
/// splits win over inner ones). Only visible tabs are searched.
pub fn find_split_divider<S>(
    node: &ViewportNode<S>,
    rect: Rect,
    pos: Vec2,
    state: &DagStructRef<S>,
) -> Option<(Rect, SplitDir, f32)> {
    match node {
        ViewportNode::Split(split) => {
            let inner = split_margin_rect(rect, split.dir);
            if divider_hit(split, inner, pos, 4.0, state) {
                // The drag starts from the effective ratio — the rendered
                // divider position — so grabbing it never jumps.
                return Some((inner, split.dir, effective_ratio(split, inner, state)));
            }
            let (r0, r1) = split_rects(split, inner, state);
            if r0.contains(pos) {
                if let Some(found) = find_split_divider(&split.children[0], r0, pos, state) {
                    return Some(found);
                }
            }
            if r1.contains(pos) {
                return find_split_divider(&split.children[1], r1, pos, state);
            }
            None
        }
        ViewportNode::Tab(tab) => {
            if tab.tabs.is_empty() {
                return None;
            }
            let active = tab.active.min(tab.tabs.len() - 1);
            find_split_divider(&tab.tabs[active].1, tab_content_rect(rect), pos, state)
        }
        ViewportNode::Widget(_) => None,
        ViewportNode::Container(children) => {
            // Children sit in per-row editor rects (container_table_rects);
            // the container's own rect would place dividers at wrong offsets.
            let Some(rows) = container_table_rects(children, rect, state) else {
                return None;
            };
            for ((_, child), (_, editor_rect)) in children.iter().zip(rows.iter()) {
                if let Some(found) = find_split_divider(child, *editor_rect, pos, state) {
                    return Some(found);
                }
            }
            None
        }
    }
}

/// Applies a drag to the split whose rect matches `drag.split_rect`.
pub fn apply_drag<S>(
    node: &mut ViewportNode<S>,
    rect: Rect,
    drag: &DragState,
    pos: Vec2,
    state: &DagStructRef<S>,
) {
    match node {
        ViewportNode::Split(split) => {
            let inner = split_margin_rect(rect, split.dir);
            if rects_equal(inner, drag.split_rect) {
                let delta = match drag.dir {
                    SplitDir::Horizontal => pos.x - drag.start_pos.x,
                    SplitDir::Vertical => pos.y - drag.start_pos.y,
                };
                let extent = match drag.dir {
                    SplitDir::Horizontal => inner.size.x,
                    SplitDir::Vertical => inner.size.y,
                };
                if extent > 0.0 {
                    // Clamp to the children's taffy minimums, not the old
                    // hard [0.1, 0.9]: a pane can only be shrunk as far as
                    // its own min size allows.
                    let (lo, hi) = split_ratio_range(split, inner, state);
                    split.ratio = (drag.start_ratio + delta / extent).clamp(lo, hi);
                }
                return;
            }
            let (r0, r1) = split_rects(split, inner, state);
            apply_drag(&mut split.children[0], r0, drag, pos, state);
            apply_drag(&mut split.children[1], r1, drag, pos, state);
        }
        ViewportNode::Tab(tab) => {
            if tab.tabs.is_empty() {
                return;
            }
            let active = tab.active.min(tab.tabs.len() - 1);
            apply_drag(
                &mut tab.tabs[active].1,
                tab_content_rect(rect),
                drag,
                pos,
                state,
            );
        }
        ViewportNode::Widget(_) => {}
        ViewportNode::Container(children) => {
            let Some(rows) = container_table_rects(children, rect, state) else {
                return;
            };
            for ((_, child), (_, editor_rect)) in children.iter_mut().zip(rows.iter()) {
                apply_drag(child, *editor_rect, drag, pos, state);
            }
        }
    }
}

fn rects_equal(a: Rect, b: Rect) -> bool {
    (a.pos.x - b.pos.x).abs() < 0.5
        && (a.pos.y - b.pos.y).abs() < 0.5
        && (a.size.x - b.size.x).abs() < 0.5
        && (a.size.y - b.size.y).abs() < 0.5
}
