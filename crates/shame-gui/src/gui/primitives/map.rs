//! A collapsible list [`Widget`] for `Port<HashMap<String, T>, S>`.
//!
//! Renders one header row per key (expander box + key label + delete
//! button); clicking the header expands the entry into its element form —
//! the labelled field widgets of `T` (from
//! [`WidgetElement::into_viewport_nodes`]). A bottom add row inserts a new
//! key with `T::default()`.
//!
//! Dirty boundaries: the map widget is the *only* writer of the map, and it
//! never wakes the DAG for a read. `insert` records `added` (the map node
//! processes exactly the new element), `remove` records `removed` (no
//! reprocess, downstream re-derives), and element edits through the
//! per-element borrow record only that key's ports. Rendering and layout
//! borrow elements with [`MapEntry::dagref_unmarked`] and mark the map port
//! only when a sub-widget actually wrote — a clean tick stays clean.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::graph::port::Port;
use crate::graph::{DagStructRef, PortId};
use crate::gui::event::{EventResponse, InputEvent, Key, MouseButton, RoutingMode, event_pos};
use crate::gui::style;
use crate::gui::viewport::context::deep_duplicate_viewport;
use crate::gui::viewport::layout::container_table_rects;
use crate::gui::viewport::node::ViewportNode;
use crate::gui::viewport::render::{walk_event, walk_render};
use crate::gui::widget::{RenderContext, Widget, WidgetData, WidgetElement};
use crate::math::Vec2;
use crate::rect::Rect;
use crate::shader::RectEntry;
use crate::text::TextObject;

/// Cached state for a `HashMap<String, T>` map widget.
pub struct MapWidgetData<T> {
    /// The labelled child-widget template built from `T::into_viewport_nodes()`.
    pub template: Vec<(String, ViewportNode<T>)>,
    /// Expanded keys, each with a per-element copy of the template (fresh
    /// widget data, so simultaneously expanded elements never share
    /// interaction state).
    pub expanded: HashMap<String, Vec<(String, ViewportNode<T>)>>,
    /// Measured expanded-section heights per key, refreshed every render.
    /// `layout_style` reads this so the assigned rect matches the real
    /// content height (falls back to [`template_height`] before the first
    /// render).
    pub heights: RefCell<HashMap<String, f32>>,
    /// The WidgetNode id inside an expanded element holding keyboard focus.
    pub sub_focus: Option<u64>,
    /// Add-row key input buffer.
    pub new_key: String,
    /// Cursor within `new_key`.
    pub new_cursor: usize,
    /// Whether the add-row input is active (keyboard goes to it).
    pub adding: bool,
}

impl<T: 'static> WidgetData for MapWidgetData<T> {}

impl<T: WidgetElement> Default for MapWidgetData<T> {
    fn default() -> Self {
        Self {
            template: T::into_viewport_nodes(),
            expanded: HashMap::new(),
            heights: RefCell::new(HashMap::new()),
            sub_focus: None,
            new_key: String::new(),
            new_cursor: 0,
            adding: false,
        }
    }
}

// ── Rect math ───────────────────────────────────────────────────────────

/// The bottom add row: full width, anchored to the bottom of `rect`.
fn add_row_rect(rect: Rect) -> Rect {
    Rect::new(
        Vec2::new(rect.pos.x, rect.pos.y + rect.size.y - style::MAP_ADD_H),
        Vec2::new(rect.size.x, style::MAP_ADD_H),
    )
}

/// The header row for one key at vertical offset `y` (full width).
fn header_rect(rect: Rect, y: f32) -> Rect {
    Rect::new(
        Vec2::new(rect.pos.x, y),
        Vec2::new(rect.size.x, style::MAP_HEADER_H),
    )
}

/// The expanded element form below a header: indented, `h` tall.
fn section_rect(rect: Rect, y: f32, h: f32) -> Rect {
    Rect::new(
        Vec2::new(rect.pos.x + style::MAP_INDENT, y),
        Vec2::new((rect.size.x - style::MAP_INDENT).max(0.0), h),
    )
}

/// The expander box inside a header row.
fn expander_rect(header: Rect) -> Rect {
    Rect::new(
        Vec2::new(
            header.pos.x + 4.0,
            header.pos.y + (header.size.y - style::MAP_EXPANDER) * 0.5,
        ),
        Vec2::new(style::MAP_EXPANDER, style::MAP_EXPANDER),
    )
}

/// The delete button at the right edge of a header row.
fn delete_rect(header: Rect) -> Rect {
    Rect::new(
        Vec2::new(
            header.pos.x + header.size.x - style::MAP_BTN_W,
            header.pos.y,
        ),
        Vec2::new(style::MAP_BTN_W, header.size.y),
    )
}

/// Fallback expanded-section height: every template row at the primitive
/// row height, plus container gaps and padding. Exact for uniform templates;
/// replaced by measured heights once rendered.
fn template_height<T>(data: &MapWidgetData<T>) -> f32 {
    super::template_height(&data.template)
}

/// The measured content height of an expanded section, from the row rects
/// `container_table_rects` computed (padding bottom included).
fn measure_section(rows: &[(Rect, Rect)], section: Rect) -> f32 {
    let last = &rows[rows.len() - 1];
    (last.1.pos.y + last.1.size.y) - section.pos.y + style::FIELD_PADDING
}

/// The expanded-section height for one key: measured at render, falling
/// back to the template estimate before the first render.
fn section_height<T>(data: &MapWidgetData<T>, key: &str) -> f32 {
    data.heights
        .borrow()
        .get(key)
        .copied()
        .unwrap_or_else(|| template_height::<T>(data))
}

impl<S: 'static, T: WidgetElement + Clone + Default + 'static> Widget<S>
    for Port<HashMap<String, T>, S>
{
    type Data = MapWidgetData<T>;

    fn layout_style(&self, state: &DagStructRef<S>, data: &Self::Data) -> taffy::Style {
        let mut h = style::MAP_ADD_H;
        let heights = data.heights.borrow();
        for key in self.read(state).keys() {
            h += style::MAP_HEADER_H;
            if data.expanded.contains_key(key) {
                h += heights
                    .get(key)
                    .copied()
                    .unwrap_or_else(|| template_height(data));
            }
        }
        taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Column,
            min_size: taffy::Size {
                width: taffy::Dimension::auto(),
                height: taffy::Dimension::length(h),
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
        let keys = self.sorted_keys(state);
        let mut heights = data.heights.borrow_mut();
        heights.clear();
        let mut y = rect.pos.y;
        for key in &keys {
            let header = header_rect(rect, y);
            draw_header(ctx, header, key, data.expanded.contains_key(key));
            y += style::MAP_HEADER_H;
            if !data.expanded.contains_key(key) {
                continue;
            }
            let section_h = heights
                .get(key)
                .copied()
                .unwrap_or_else(|| template_height(data));
            let section = section_rect(rect, y, section_h);
            let nodes = data.expanded.get(key).unwrap();
            self.with_element_ref(state, key, |eref, _ports| {
                if let Some(rows) = container_table_rects(nodes, section, eref) {
                    for ((label, child), (label_rect, editor_rect)) in nodes.iter().zip(rows.iter())
                    {
                        ctx.texts.push(
                            TextObject::new(label.clone())
                                .at_position(
                                    label_rect.pos
                                        + Vec2::new(style::LABEL_PAD_X, style::LABEL_PAD_Y),
                                )
                                .with_font_size(style::LABEL_FONT_SIZE)
                                .with_color(style::LABEL_TEXT_COLOR)
                                .with_z(style::Z_TEXT),
                        );
                        walk_render(child, *editor_rect, ctx, eref);
                    }
                    heights.insert(key.clone(), measure_section(&rows, section));
                }
            });
            y += section_h;
        }
        draw_add_row(ctx, rect, data);
    }

    fn on_event(
        &self,
        state: &mut DagStructRef<S>,
        data: &mut Self::Data,
        event: &InputEvent,
        rect: Rect,
    ) -> EventResponse {
        match event {
            InputEvent::MouseDown { pos, button, .. } => {
                if *button != MouseButton::Left || !rect.contains(*pos) {
                    return EventResponse::Ignored;
                }
                // Bottom add row: input field or + button.
                let add = add_row_rect(rect);
                if add.contains(*pos) {
                    let input_w = (add.size.x - style::MAP_BTN_W - 4.0).max(0.0);
                    let input = Rect::new(add.pos, Vec2::new(input_w, add.size.y));
                    let btn = Rect::new(
                        Vec2::new(add.pos.x + input_w + 4.0, add.pos.y),
                        Vec2::new(style::MAP_BTN_W, add.size.y),
                    );
                    if input.contains(*pos) {
                        data.adding = true;
                    } else if btn.contains(*pos) {
                        self.commit_add(state, data);
                        data.adding = true;
                    }
                    return EventResponse::Consumed;
                }
                // Header rows (top-down); the expanded section of one key
                // sits between its header and the next.
                let keys = self.sorted_keys(state);
                let mut y = rect.pos.y;
                for key in keys {
                    let header = header_rect(rect, y);
                    if header.contains(*pos) {
                        if delete_rect(header).contains(*pos) {
                            self.remove_key(state, data, &key);
                        } else {
                            self.toggle_expand(data, &key);
                        }
                        data.adding = false;
                        return EventResponse::Consumed;
                    }
                    y += style::MAP_HEADER_H;
                    if !data.expanded.contains_key(&key) {
                        continue;
                    }
                    let section = section_rect(rect, y, section_height(data, &key));
                    if section.contains(*pos) {
                        let response = self.with_element_ref(state, &key, |eref, _ports| {
                            let mut response = EventResponse::Ignored;
                            let nodes = data.expanded.get_mut(&key).unwrap();
                            if let Some(rows) = container_table_rects(nodes, section, eref) {
                                for ((_, child), (_, editor_rect)) in
                                    nodes.iter_mut().zip(rows.iter())
                                {
                                    let r = walk_event(
                                        child,
                                        *editor_rect,
                                        event,
                                        &mut data.sub_focus,
                                        eref,
                                    );
                                    if r == EventResponse::Consumed {
                                        response = EventResponse::Consumed;
                                        break;
                                    }
                                }
                            }
                            response
                        });
                        if response == EventResponse::Consumed {
                            data.adding = false;
                            return EventResponse::Consumed;
                        }
                    }
                    y += section_height(data, &key);
                }
                data.adding = false;
                EventResponse::Consumed
            }
            InputEvent::Char { .. } | InputEvent::KeyDown { .. } => {
                if data.adding {
                    self.edit_add_input(state, data, event);
                    EventResponse::Consumed
                } else {
                    self.forward_to_elements(state, data, event, rect)
                }
            }
            _ => self.forward_to_elements(state, data, event, rect),
        }
    }

    fn selectable(&self) -> bool {
        true
    }
}

impl<S: 'static, T: WidgetElement + Clone + Default + 'static> Port<HashMap<String, T>, S> {
    /// Keys in sorted order — render, events and layout all iterate the same
    /// stable order.
    fn sorted_keys(&self, state: &DagStructRef<S>) -> Vec<String> {
        let mut keys: Vec<String> = self.read(state).keys().cloned().collect();
        keys.sort();
        keys
    }

    /// Runs `f` with one element's tracked ref for a read-mostly widget
    /// walk: per-key tracking is shared with the graph, but the map port is
    /// not woken. Afterwards the map port is marked dirty if any element
    /// port was actually written through the ref (a sub-widget wrote during
    /// the walk) — a read-only pass never dirties the graph.
    fn with_element_ref<'s, R>(
        &self,
        state: &'s mut DagStructRef<'_, S>,
        key: &str,
        f: impl for<'a> FnOnce(&'a mut DagStructRef<'a, T>, &Rc<RefCell<HashSet<PortId>>>) -> R,
    ) -> R {
        let dirty = state.dirty_rc();
        let ports = state.ensure_elem_ports::<String>(self.id(), key.to_string());
        let mut entry = self
            .get(state, key.to_string())
            .expect("expanded key must exist in the map");
        let mut eref = entry.dagref_unmarked();
        let out = f(&mut eref, &ports);
        if !ports.borrow().is_empty() {
            dirty.borrow_mut().insert(self.id());
        }
        out
    }

    /// Forwards a non-add-row event to every expanded element's children.
    /// `walk_event` handles positional hit tests, broadcast visits, and
    /// focused routing via `data.sub_focus`.
    fn forward_to_elements(
        &self,
        state: &mut DagStructRef<S>,
        data: &mut MapWidgetData<T>,
        event: &InputEvent,
        rect: Rect,
    ) -> EventResponse {
        let visit_all = event.routing() == RoutingMode::Broadcast;
        let keys = self.sorted_keys(state);
        let mut y = rect.pos.y;
        for key in keys {
            y += style::MAP_HEADER_H;
            if !data.expanded.contains_key(&key) {
                continue;
            }
            let section_h = section_height(data, &key);
            let section = section_rect(rect, y, section_h);
            y += section_h;
            if !visit_all && event_pos(event).is_some_and(|p| !section.contains(p)) {
                continue;
            }
            let consumed = self.with_element_ref(state, &key, |eref, _ports| {
                let mut consumed = false;
                let nodes = data.expanded.get_mut(&key).unwrap();
                if let Some(rows) = container_table_rects(nodes, section, eref) {
                    for ((_, child), (_, editor_rect)) in nodes.iter_mut().zip(rows.iter()) {
                        let r = walk_event(child, *editor_rect, event, &mut data.sub_focus, eref);
                        if r == EventResponse::Consumed && !visit_all {
                            consumed = true;
                            break;
                        }
                    }
                }
                consumed
            });
            if consumed {
                return EventResponse::Consumed;
            }
        }
        EventResponse::Ignored
    }

    /// Toggles an entry: collapse drops its duplicated nodes (fresh data is
    /// rebuilt on re-expand), expand clones the template per element.
    fn toggle_expand(&self, data: &mut MapWidgetData<T>, key: &str) {
        if data.expanded.remove(key).is_some() {
            data.heights.borrow_mut().remove(key);
            data.sub_focus = None;
        } else {
            let nodes = data
                .template
                .iter()
                .map(|(label, node)| (label.clone(), deep_duplicate_viewport(node)))
                .collect();
            data.expanded.insert(key.to_string(), nodes);
        }
    }

    /// Removes a key, recording `removed` so the map node re-runs downstream
    /// readers but never reprocesses the element itself.
    fn remove_key(&self, state: &mut DagStructRef<S>, data: &mut MapWidgetData<T>, key: &str) {
        self.remove(state, key.to_string());
        data.expanded.remove(key);
        data.heights.borrow_mut().remove(key);
    }

    /// Inserts a new key with `T::default()` (no-op for empty or existing
    /// keys — an overwrite would silently discard the element).
    fn commit_add(&self, state: &mut DagStructRef<S>, data: &mut MapWidgetData<T>) {
        let key = data.new_key.trim().to_string();
        if !key.is_empty() && !self.read(state).contains_key(&key) {
            self.insert(state, key, T::default());
        }
        data.new_key.clear();
        data.new_cursor = 0;
    }

    /// Text-editing loop for the add-row key input (mirrors the string
    /// widget; the buffer lives in `data`, not in state).
    fn edit_add_input(
        &self,
        state: &mut DagStructRef<S>,
        data: &mut MapWidgetData<T>,
        event: &InputEvent,
    ) {
        match event {
            InputEvent::Char { ch } => {
                if !ch.is_control() {
                    data.new_key.insert(data.new_cursor, *ch);
                    data.new_cursor += ch.len_utf8();
                }
            }
            InputEvent::KeyDown { key } => match key {
                Key::Backspace => {
                    if data.new_cursor > 0 {
                        let start = data.new_key.floor_char_boundary(data.new_cursor - 1);
                        data.new_key.remove(start);
                        data.new_cursor = start;
                    }
                }
                Key::ArrowLeft => {
                    data.new_cursor = data
                        .new_key
                        .floor_char_boundary(data.new_cursor.saturating_sub(1));
                }
                Key::ArrowRight => {
                    data.new_cursor = data
                        .new_key
                        .ceil_char_boundary((data.new_cursor + 1).min(data.new_key.len()));
                }
                Key::Enter => {
                    self.commit_add(state, data);
                }
                Key::Escape => {
                    data.new_key.clear();
                    data.new_cursor = 0;
                    data.adding = false;
                }
                _ => {}
            },
            _ => {}
        }
    }
}

// ── Drawing ─────────────────────────────────────────────────────────────

fn draw_header(ctx: &mut RenderContext, header: Rect, key: &str, expanded: bool) {
    ctx.fills.push(RectEntry {
        rect: header,
        color: style::MAP_HEADER_BG.to_linear(),
        z: style::Z_PANEL,
    });
    // Expander box: outline plus a horizontal bar (collapsed) or a plus
    // (expanded) — drawn with rects, no font glyphs required.
    let ex = expander_rect(header);
    ctx.outlines.push(RectEntry {
        rect: ex,
        color: style::BORDER.to_linear(),
        z: style::Z_BORDER,
    });
    let cx = ex.pos.x + ex.size.x * 0.5;
    let cy = ex.pos.y + ex.size.y * 0.5;
    let bar = 6.0;
    ctx.fills.push(RectEntry {
        rect: Rect::new(Vec2::new(cx - bar * 0.5, cy - 0.5), Vec2::new(bar, 1.0)),
        color: style::BORDER.to_linear(),
        z: style::Z_BORDER,
    });
    if expanded {
        ctx.fills.push(RectEntry {
            rect: Rect::new(Vec2::new(cx - 0.5, cy - bar * 0.5), Vec2::new(1.0, bar)),
            color: style::BORDER.to_linear(),
            z: style::Z_BORDER,
        });
    }
    ctx.texts.push(
        TextObject::new(key.to_string())
            .at_position(Vec2::new(
                header.pos.x + 4.0 + style::MAP_EXPANDER + 8.0,
                header.pos.y + style::LABEL_PAD_Y,
            ))
            .with_font_size(style::LABEL_FONT_SIZE)
            .with_color(style::LABEL_TEXT_COLOR)
            .with_z(style::Z_TEXT),
    );
    // Delete button at the right edge.
    let del = delete_rect(header);
    ctx.outlines.push(RectEntry {
        rect: del,
        color: style::BORDER.to_linear(),
        z: style::Z_BORDER,
    });
    ctx.texts.push(
        TextObject::new("x".to_string())
            .at_position(Vec2::new(
                del.pos.x + (del.size.x - 8.0) * 0.5,
                del.pos.y + style::LABEL_PAD_Y,
            ))
            .with_font_size(style::LABEL_FONT_SIZE)
            .with_color(style::MAP_DELETE_TEXT)
            .with_z(style::Z_TEXT),
    );
}

fn draw_add_row(ctx: &mut RenderContext, rect: Rect, data: &MapWidgetData<impl WidgetElement>) {
    let add = add_row_rect(rect);
    let input_w = (add.size.x - style::MAP_BTN_W - 4.0).max(0.0);
    let input = Rect::new(add.pos, Vec2::new(input_w, add.size.y));
    ctx.fills.push(RectEntry {
        rect: input,
        color: style::FIELD_BG.to_linear(),
        z: style::Z_PANEL,
    });
    let mut display = data.new_key.clone();
    if data.adding {
        let c = data.new_cursor.min(display.len());
        display.insert(c, '|');
    }
    ctx.texts.push(
        TextObject::new(display)
            .at_position(input.pos + Vec2::new(style::FIELD_PAD_X, style::FIELD_PAD_Y))
            .with_font_size(style::FIELD_FONT_SIZE)
            .with_z(style::Z_TEXT),
    );
    let btn = Rect::new(
        Vec2::new(add.pos.x + input_w + 4.0, add.pos.y),
        Vec2::new(style::MAP_BTN_W, add.size.y),
    );
    ctx.fills.push(RectEntry {
        rect: btn,
        color: style::CHECK_ON.to_linear(),
        z: style::Z_PANEL,
    });
    ctx.texts.push(
        TextObject::new("+".to_string())
            .at_position(Vec2::new(
                btn.pos.x + (btn.size.x - 8.0) * 0.5,
                btn.pos.y + style::LABEL_PAD_Y,
            ))
            .with_font_size(style::FIELD_FONT_SIZE)
            .with_color(style::TAB_ACTIVE_TEXT)
            .with_z(style::Z_TEXT),
    );
}
