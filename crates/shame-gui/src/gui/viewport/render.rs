use crate::canvas::Canvas;
use crate::graph::StateArena;
use crate::gui::event::{EventResponse, InputEvent, Key, RoutingMode, event_pos};
use crate::gui::style;
use crate::gui::widget::{RenderContext, WidgetNode};
use crate::math::{Vec2, Vec2u};
use crate::rect::Rect;
use crate::shader::{RectEntry, RectInstance, RectMaterial, ViewportParams, WireframeMaterial};
use crate::sm;
use crate::text::TextObject;

use super::layout::{
    container_table_rects, divider_hit, divider_rect, split_margin_rect, split_rects, tab_bar_rect,
    tab_content_rect,
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
pub fn walk_render(
    node: &ViewportNode,
    rect: Rect,
    ctx: &mut RenderContext<'_>,
    arena: &mut StateArena,
) {
    match node {
        ViewportNode::Widget(widget) => {
            render_node_outline(ctx, rect);
            widget.widget.render(arena, rect, ctx);
        }
        ViewportNode::Split(split) => {
            render_node_outline(ctx, rect);
            let inner = split_margin_rect(rect, split.dir);
            ctx.fills.push(RectEntry {
                rect: divider_rect(inner, split.dir, split.ratio),
                color: style::DIVIDER.to_linear(),
                z: style::Z_DIVIDER,
            });
            let (r0, r1) = split_rects(inner, split.dir, split.ratio);
            walk_render(&split.children[0], r0, ctx, arena);
            walk_render(&split.children[1], r1, ctx, arena);
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
            walk_render(&tab.tabs[active].1, tab_content_rect(rect), ctx, arena);
        }
        ViewportNode::Container(children) => {
            render_node_outline(ctx, rect);
            let Some(rows) = container_table_rects(children, rect, arena) else {
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
                walk_render(child, *editor_rect, ctx, arena);
            }
        }
    }
}

// ── Event walk ──────────────────────────────────────────────────────────

/// Routes one event through the tree. The `focus` parameter tracks which
/// WidgetNode is currently focused (set on MouseDown, used for keyboard
/// routing). `dirty` collects PortIds of widgets whose value changed.
pub fn walk_event(
    node: &mut ViewportNode,
    rect: Rect,
    event: &InputEvent,
    focus: &mut Option<u64>,
    arena: &mut StateArena,
    dirty: &mut Vec<crate::graph::PortId>,
) -> EventResponse {
    let visit_all = event.routing() == RoutingMode::Broadcast;
    let pos = event_pos(event).unwrap_or(Vec2::new(0.0, 0.0));
    let is_focused = event.routing() == RoutingMode::Focused;
    match node {
        ViewportNode::Widget(widget) => {
            // Focused events (keyboard/char): only route to the focused WidgetNode.
            if is_focused && *focus != Some(widget.id) {
                return EventResponse::Ignored;
            }
            if !visit_all && !rect.contains(pos) {
                return EventResponse::Ignored;
            }
            // MouseDown on a selectable WidgetNode claims focus.
            if let InputEvent::MouseDown { .. } = event {
                if widget.widget.selectable() {
                    *focus = Some(widget.id);
                }
            }
            let response = widget.widget.on_event(arena, event, rect);
            maybe_dirty(widget, event, dirty);
            if response == EventResponse::Consumed && !visit_all {
                return EventResponse::Consumed;
            }
            EventResponse::Ignored
        }
        ViewportNode::Split(split) => {
            let inner = split_margin_rect(rect, split.dir);
            let (r0, r1) = split_rects(inner, split.dir, split.ratio);
            if walk_event(&mut split.children[0], r0, event, focus, arena, dirty)
                == EventResponse::Consumed
                && !visit_all
            {
                return EventResponse::Consumed;
            }
            walk_event(&mut split.children[1], r1, event, focus, arena, dirty)
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
                arena,
                dirty,
            )
        }
        ViewportNode::Container(children) => {
            if children.is_empty() {
                return EventResponse::Ignored;
            }
            let is_focused = event.routing() == RoutingMode::Focused;
            let Some(rows) = container_table_rects(children, rect, arena) else {
                return EventResponse::Ignored;
            };
            if is_focused {
                for (_, child) in children.iter_mut() {
                    if let ViewportNode::Widget(w) = child {
                        if *focus == Some(w.id) {
                            let r = w.widget.on_event(arena, event, rect);
                            if r == EventResponse::Consumed
                                && matches!(
                                    event,
                                    InputEvent::MouseDown { .. }
                                        | InputEvent::KeyDown {
                                            key: Key::Enter,
                                            ..
                                        }
                                )
                            {
                                for &pid in &w.port_ids {
                                    dirty.push(pid);
                                }
                            }
                            return r;
                        }
                    }
                }
                return EventResponse::Ignored;
            }
            if event.routing() == RoutingMode::Broadcast {
                for ((_, child), (_, editor_rect)) in children.iter_mut().zip(rows.iter()) {
                    if let ViewportNode::Widget(w) = child {
                        w.widget.on_event(arena, event, *editor_rect);
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
                    let response = w.widget.on_event(arena, event, *editor_rect);
                    if response == EventResponse::Consumed
                        && matches!(
                            event,
                            InputEvent::MouseDown { .. }
                                | InputEvent::KeyDown {
                                    key: Key::Enter,
                                    ..
                                }
                        )
                    {
                        for &pid in &w.port_ids {
                            dirty.push(pid);
                        }
                    }
                    if response == EventResponse::Consumed {
                        return EventResponse::Consumed;
                    }
                } else {
                    let response = walk_event(child, *editor_rect, event, focus, arena, dirty);
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

fn maybe_dirty(widget: &WidgetNode, event: &InputEvent, dirty: &mut Vec<crate::graph::PortId>) {
    if !widget.port_ids.is_empty()
        && matches!(
            event,
            InputEvent::MouseDown { .. }
                | InputEvent::KeyDown {
                    key: Key::Enter,
                    ..
                }
        )
    {
        dirty.extend_from_slice(&widget.port_ids);
    }
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
pub fn batch_draw(
    gpu: &sm::Gpu,
    canvas: &mut Canvas,
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
pub fn find_split_divider(
    node: &ViewportNode,
    rect: Rect,
    pos: Vec2,
) -> Option<(Rect, SplitDir, f32)> {
    match node {
        ViewportNode::Split(split) => {
            let inner = split_margin_rect(rect, split.dir);
            if divider_hit(inner, split.dir, split.ratio, pos, 4.0) {
                return Some((inner, split.dir, split.ratio));
            }
            let (r0, r1) = split_rects(inner, split.dir, split.ratio);
            if r0.contains(pos) {
                if let Some(found) = find_split_divider(&split.children[0], r0, pos) {
                    return Some(found);
                }
            }
            if r1.contains(pos) {
                return find_split_divider(&split.children[1], r1, pos);
            }
            None
        }
        ViewportNode::Tab(tab) => {
            if tab.tabs.is_empty() {
                return None;
            }
            let active = tab.active.min(tab.tabs.len() - 1);
            find_split_divider(&tab.tabs[active].1, tab_content_rect(rect), pos)
        }
        ViewportNode::Widget(_) => None,
        ViewportNode::Container(children) => {
            for (_, child) in children {
                if let Some(found) = find_split_divider(child, rect, pos) {
                    return Some(found);
                }
            }
            None
        }
    }
}

/// Applies a drag to the split whose rect matches `drag.split_rect`.
pub fn apply_drag(node: &mut ViewportNode, rect: Rect, drag: &DragState, pos: Vec2) {
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
                    split.ratio = (drag.start_ratio + delta / extent).clamp(0.1, 0.9);
                }
                return;
            }
            let (r0, r1) = split_rects(inner, split.dir, split.ratio);
            apply_drag(&mut split.children[0], r0, drag, pos);
            apply_drag(&mut split.children[1], r1, drag, pos);
        }
        ViewportNode::Tab(tab) => {
            if tab.tabs.is_empty() {
                return;
            }
            let active = tab.active.min(tab.tabs.len() - 1);
            apply_drag(&mut tab.tabs[active].1, tab_content_rect(rect), drag, pos);
        }
        ViewportNode::Widget(_) => {}
        ViewportNode::Container(children) => {
            for (_, child) in children.iter_mut() {
                apply_drag(child, rect, drag, pos);
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
