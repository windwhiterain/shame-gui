//! A key-selector [`Widget`]: edits a `Port<String, S>` key value constrained
//! to the keys of a `Port<HashMap<String, T>, S>`.
//!
//! A header row shows the current value (a "(none)" placeholder when empty);
//! clicking it expands the sorted key list inline (the widget's layout
//! height grows, like the map widget), and clicking a row writes that key
//! into the `String` port. The map is a read-only constraint source — the
//! selector never writes it.
//!
//! Constraint semantics: the empty value is the valid "unset" state; any
//! other value must be a key of the map. `render` enforces this by resetting
//! a stale value (its key was removed elsewhere) to `""`, so the invariant
//! "empty or a key" holds after every frame.
//!
//! Dirty boundaries: `render`/`layout_style` only read both ports (reads are
//! untracked — a clean tick stays clean); the stale-value reset writes the
//! key port once and converges the next frame; a selection writes the key
//! port only when it actually changes. The map port is never marked dirty.

use std::collections::HashMap;

use crate::graph::DagStructRef;
use crate::graph::port::Port;
use crate::gui::event::{EventResponse, InputEvent, MouseButton};
use crate::gui::style;
use crate::gui::widget::{RenderContext, Widget, WidgetData};
use crate::math::Vec2;
use crate::rect::Rect;
use crate::shader::RectEntry;
use crate::text::TextObject;

/// Cached interaction state for a [`MapSelector`].
pub struct SelectorData {
    /// Whether the key list is expanded.
    pub open: bool,
    /// The option row under the mouse (`None` when closed or outside the
    /// list) — drawn highlighted like a context-menu item.
    pub hover: Option<usize>,
}

impl WidgetData for SelectorData {}

impl Default for SelectorData {
    fn default() -> Self {
        Self {
            open: false,
            hover: None,
        }
    }
}

/// A dropdown that edits a `Port<String, S>` key value, constrained to the
/// keys of a `Port<HashMap<String, T>, S>`.
///
/// A manual handle — it binds two ports, so the `Widget` derive cannot emit
/// it; construct it and wrap it in a [`WidgetNode`](crate::gui::WidgetNode)
/// by hand.
pub struct MapSelector<S, T>
where
    T: Clone + 'static,
{
    /// The constraint source: valid values are its keys. Read-only.
    map: Port<HashMap<String, T>, S>,
    /// The key value being edited.
    key: Port<String, S>,
}

// Manual Clone — the ports are Copy fn-pointer handles, so `S` needs no
// `Clone` bound (a derive would add one).
impl<S, T: Clone + 'static> Clone for MapSelector<S, T> {
    fn clone(&self) -> Self {
        Self {
            map: self.map,
            key: self.key,
        }
    }
}

impl<S, T: Clone + 'static> MapSelector<S, T> {
    /// Binds a key value to the keys of `map`.
    pub fn new(map: Port<HashMap<String, T>, S>, key: Port<String, S>) -> Self {
        Self { map, key }
    }
}

impl<S: 'static, T: Clone + 'static> Widget<S> for MapSelector<S, T> {
    type Data = SelectorData;

    fn layout_style(&self, state: &DagStructRef<S>, data: &Self::Data) -> taffy::Style {
        let list_h = if data.open {
            self.map.read(state).len() as f32 * style::SELECTOR_ROW_H
        } else {
            0.0
        };
        taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Row,
            min_size: taffy::Size {
                width: taffy::Dimension::auto(),
                height: taffy::Dimension::length(style::SELECTOR_HEADER_H + list_h),
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
        // Enforce the constraint: a non-empty value that is not a key of the
        // map is stale (its key was removed elsewhere) — reset it to the
        // valid "unset" state. Writes once, converges the next frame.
        let current = self.key.read(state).clone();
        if !current.is_empty() && !self.map.read(state).contains_key(&current) {
            self.key.write(state, String::new());
        }

        let header = header_rect(rect);
        ctx.fills.push(RectEntry {
            rect: header,
            color: style::MAP_HEADER_BG.to_linear(),
            z: style::Z_PANEL,
        });
        let value = self.key.read(state).clone();
        let pos = header.pos + Vec2::new(style::FIELD_PAD_X, style::FIELD_PAD_Y);
        if value.is_empty() {
            ctx.texts.push(
                TextObject::new("(none)")
                    .at_position(pos)
                    .with_font_size(style::FIELD_FONT_SIZE)
                    .with_color(style::LABEL_TEXT_COLOR)
                    .with_z(style::Z_TEXT),
            );
        } else {
            ctx.texts.push(
                TextObject::new(value)
                    .at_position(pos)
                    .with_font_size(style::FIELD_FONT_SIZE)
                    .with_z(style::Z_TEXT),
            );
        }
        draw_expander(ctx, header, data.open);
        if !data.open {
            return;
        }

        let keys = self.sorted_keys(state);
        if keys.is_empty() {
            let row = Rect::new(
                Vec2::new(rect.pos.x, rect.pos.y + style::SELECTOR_HEADER_H),
                Vec2::new(rect.size.x, style::SELECTOR_ROW_H),
            );
            ctx.fills.push(RectEntry {
                rect: row,
                color: style::MENU_BG.to_linear(),
                z: style::Z_PANEL,
            });
            ctx.texts.push(
                TextObject::new("(no options)")
                    .at_position(row.pos + Vec2::new(style::FIELD_PAD_X, style::FIELD_PAD_Y))
                    .with_font_size(style::FIELD_FONT_SIZE)
                    .with_color(style::LABEL_TEXT_COLOR)
                    .with_z(style::Z_TEXT),
            );
            return;
        }
        let mut y = rect.pos.y + style::SELECTOR_HEADER_H;
        for (index, key) in keys.iter().enumerate() {
            let row = Rect::new(
                Vec2::new(rect.pos.x, y),
                Vec2::new(rect.size.x, style::SELECTOR_ROW_H),
            );
            let bg = if data.hover == Some(index) {
                style::MENU_HOVER
            } else {
                style::MENU_BG
            };
            ctx.fills.push(RectEntry {
                rect: row,
                color: bg.to_linear(),
                z: style::Z_PANEL,
            });
            let color = if self.key.read(state) == key {
                style::CHECK_ON
            } else {
                style::MENU_TEXT
            };
            ctx.texts.push(
                TextObject::new(key.clone())
                    .at_position(row.pos + Vec2::new(style::FIELD_PAD_X, style::FIELD_PAD_Y))
                    .with_font_size(style::FIELD_FONT_SIZE)
                    .with_color(color)
                    .with_z(style::Z_TEXT),
            );
            y += style::SELECTOR_ROW_H;
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
            InputEvent::MouseDown { pos, button, .. } => {
                if *button != MouseButton::Left {
                    return EventResponse::Ignored;
                }
                let header = header_rect(rect);
                if data.open {
                    // An option row: write the key and close.
                    let keys = self.sorted_keys(state);
                    let mut y = rect.pos.y + style::SELECTOR_HEADER_H;
                    for key in &keys {
                        let row = Rect::new(
                            Vec2::new(rect.pos.x, y),
                            Vec2::new(rect.size.x, style::SELECTOR_ROW_H),
                        );
                        if row.contains(*pos) {
                            if self.key.read(state) != key {
                                self.key.write(state, key.clone());
                            }
                            data.open = false;
                            data.hover = None;
                            return EventResponse::Consumed;
                        }
                        y += style::SELECTOR_ROW_H;
                    }
                    // Anywhere else inside the widget (the header, or space
                    // below a short list): close without selecting.
                    if rect.contains(*pos) {
                        data.open = false;
                        data.hover = None;
                        return EventResponse::Consumed;
                    }
                    EventResponse::Ignored
                } else if header.contains(*pos) {
                    data.open = true;
                    EventResponse::Consumed
                } else {
                    EventResponse::Ignored
                }
            }
            InputEvent::MouseMove { pos, .. } => {
                // Hover highlight for the open list. Broadcast events fire
                // every frame, so this only touches `data`, never the state.
                let list_top = rect.pos.y + style::SELECTOR_HEADER_H;
                data.hover = if data.open && rect.contains(*pos) && pos.y >= list_top {
                    let index = ((pos.y - list_top) / style::SELECTOR_ROW_H) as usize;
                    if index < self.map.read(state).len() {
                        Some(index)
                    } else {
                        None
                    }
                } else {
                    None
                };
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn selectable(&self) -> bool {
        false
    }
}

impl<S: 'static, T: Clone + 'static> MapSelector<S, T> {
    /// Keys in sorted order — render and events iterate the same stable order.
    fn sorted_keys(&self, state: &DagStructRef<S>) -> Vec<String> {
        let mut keys: Vec<String> = self.map.read(state).keys().cloned().collect();
        keys.sort();
        keys
    }
}

/// The click-to-open header row (full width, [`style::SELECTOR_HEADER_H`] tall).
fn header_rect(rect: Rect) -> Rect {
    Rect::new(rect.pos, Vec2::new(rect.size.x, style::SELECTOR_HEADER_H))
}

/// Right-edge expander box: outline plus a horizontal bar (closed) or a plus
/// (open) — drawn with rects, no font glyphs (mirrors the map widget).
fn draw_expander(ctx: &mut RenderContext, header: Rect, open: bool) {
    let ex = Rect::new(
        Vec2::new(
            header.pos.x + header.size.x - style::MAP_EXPANDER - 6.0,
            header.pos.y + (header.size.y - style::MAP_EXPANDER) * 0.5,
        ),
        Vec2::new(style::MAP_EXPANDER, style::MAP_EXPANDER),
    );
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
    if open {
        ctx.fills.push(RectEntry {
            rect: Rect::new(Vec2::new(cx - 0.5, cy - bar * 0.5), Vec2::new(1.0, bar)),
            color: style::BORDER.to_linear(),
            z: style::Z_BORDER,
        });
    }
}
