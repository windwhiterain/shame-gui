//! A two-field editor [`Widget`] for `Vec2Ports`
//! (edits the x and y components independently).

use crate::graph::StateArena;
use crate::gui::event::{EventResponse, InputEvent, Key};
use crate::gui::style;
use crate::gui::widget::{RenderContext, Widget, WidgetData};
use crate::math::Vec2Ports;
use crate::rect::Rect;
use crate::shader::RectEntry;
use crate::text::TextObject;

/// Cached state for a single numeric field editor.
pub struct EditingField {
    /// The raw text being edited.
    pub buffer: String,
    /// Cursor position within `buffer`.
    pub cursor: usize,
    /// Whether the user is currently typing.
    pub editing: bool,
}

impl EditingField {
    fn start(&mut self, value: f32) {
        self.buffer = value.to_string();
        self.cursor = self.buffer.len();
        self.editing = true;
    }

    fn insert_char(&mut self, ch: char) {
        self.buffer.insert(self.cursor, ch);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.buffer.remove(self.cursor - 1);
            self.cursor -= 1;
        }
    }

    fn cursor_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn cursor_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.buffer.len());
    }

    fn commit<F: FnOnce(f32)>(&mut self, set: F) {
        if let Ok(val) = self.buffer.parse::<f32>() {
            set(val);
            self.editing = false;
        }
    }

    fn cancel(&mut self) {
        self.editing = false;
    }

    fn display_text(&self, value: f32) -> String {
        if self.editing {
            let mut s = if self.buffer.is_empty() {
                value.to_string()
            } else {
                self.buffer.clone()
            };
            let c = self.cursor.min(s.len());
            s.insert(c, '|');
            s
        } else {
            format!("{:.1}", value)
        }
    }
}

impl Default for EditingField {
    fn default() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            editing: false,
        }
    }
}

/// Cached state for a Vec2 editor (x and y sub-editors).
pub struct Vec2WidgetData {
    /// The x component editor.
    pub x: EditingField,
    /// The y component editor.
    pub y: EditingField,
}

impl WidgetData for Vec2WidgetData {}

impl Default for Vec2WidgetData {
    fn default() -> Self {
        Self {
            x: EditingField::default(),
            y: EditingField::default(),
        }
    }
}

fn field_rect(rect: Rect) -> (Rect, Rect) {
    let half_w = rect.size.x * 0.5 - 4.0;
    (
        Rect::new(rect.pos, crate::math::Vec2::new(half_w, rect.size.y)),
        Rect::new(
            rect.pos + crate::math::Vec2::new(half_w + 8.0, 0.0),
            crate::math::Vec2::new(half_w, rect.size.y),
        ),
    )
}

fn text_pos(rect: Rect) -> crate::math::Vec2 {
    rect.pos + crate::math::Vec2::new(style::FIELD_PAD_X, style::FIELD_PAD_Y)
}

impl Widget for Vec2Ports {
    type Data = Vec2WidgetData;

    fn layout_style(&self, _arena: &StateArena, _data: &Self::Data) -> taffy::Style {
        taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Row,
            min_size: taffy::Size {
                width: taffy::Dimension::length(160.0),
                height: taffy::Dimension::length(24.0),
            },
            gap: taffy::Size::length(8.0),
            ..Default::default()
        }
    }

    fn render(
        &self,
        arena: &mut StateArena,
        data: &Self::Data,
        rect: Rect,
        ctx: &mut RenderContext,
    ) {
        let (rx, ry) = field_rect(rect);
        let vx = *self.x.read(arena);
        let vy = *self.y.read(arena);

        ctx.fills.push(RectEntry {
            rect: rx,
            color: style::FIELD_BG.to_linear(),
            z: style::Z_PANEL,
        });
        ctx.texts.push(
            TextObject::new(data.x.display_text(vx))
                .at_position(text_pos(rx))
                .with_font_size(style::FIELD_FONT_SIZE)
                .with_z(style::Z_TEXT),
        );

        ctx.fills.push(RectEntry {
            rect: ry,
            color: style::FIELD_BG.to_linear(),
            z: style::Z_PANEL,
        });
        ctx.texts.push(
            TextObject::new(data.y.display_text(vy))
                .at_position(text_pos(ry))
                .with_font_size(style::FIELD_FONT_SIZE)
                .with_z(style::Z_TEXT),
        );
    }

    fn on_event(
        &self,
        arena: &mut StateArena,
        data: &mut Self::Data,
        event: &InputEvent,
        rect: Rect,
    ) -> EventResponse {
        let (rx, ry) = field_rect(rect);

        match event {
            InputEvent::MouseDown { pos, .. } => {
                if rx.contains(*pos) {
                    data.x.start(*self.x.read(arena));
                    data.y.cancel();
                    return EventResponse::Consumed;
                }
                if ry.contains(*pos) {
                    data.y.start(*self.y.read(arena));
                    data.x.cancel();
                    return EventResponse::Consumed;
                }
                data.x.cancel();
                data.y.cancel();
                EventResponse::Ignored
            }
            InputEvent::Char { ch } if data.x.editing || data.y.editing => {
                if !ch.is_control() {
                    if data.x.editing {
                        data.x.insert_char(*ch);
                    } else {
                        data.y.insert_char(*ch);
                    }
                }
                EventResponse::Consumed
            }
            InputEvent::KeyDown { key } if data.x.editing || data.y.editing => match key {
                Key::Backspace => {
                    if data.x.editing {
                        data.x.backspace();
                    } else {
                        data.y.backspace();
                    }
                    EventResponse::Consumed
                }
                Key::ArrowLeft => {
                    if data.x.editing {
                        data.x.cursor_left();
                    } else {
                        data.y.cursor_left();
                    }
                    EventResponse::Consumed
                }
                Key::ArrowRight => {
                    if data.x.editing {
                        data.x.cursor_right();
                    } else {
                        data.y.cursor_right();
                    }
                    EventResponse::Consumed
                }
                Key::Enter => {
                    if data.x.editing {
                        data.x.commit(|v| self.x.write(arena, v));
                    } else {
                        data.y.commit(|v| self.y.write(arena, v));
                    }
                    EventResponse::Consumed
                }
                Key::Escape => {
                    data.x.cancel();
                    data.y.cancel();
                    EventResponse::Consumed
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn selectable(&self) -> bool {
        true
    }
}
