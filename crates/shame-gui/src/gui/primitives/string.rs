//! A text-input [`Widget`] for `Port<String, S>`.

use crate::graph::DagStructRef;
use crate::graph::port::Port;
use crate::gui::event::{EventResponse, InputEvent, Key};
use crate::gui::style;
use crate::gui::widget::{RenderContext, Widget, WidgetData};
use crate::rect::Rect;
use crate::shader::RectEntry;
use crate::text::TextObject;

/// Cached state for a text input.
pub struct StringWidgetData {
    /// The raw text being edited (empty when not editing).
    pub buffer: String,
    /// Cursor position within `buffer`.
    pub cursor: usize,
    /// Whether the user is currently typing.
    pub editing: bool,
}

impl WidgetData for StringWidgetData {}

impl Default for StringWidgetData {
    fn default() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            editing: false,
        }
    }
}

fn text_pos(rect: Rect) -> crate::math::Vec2 {
    rect.pos + crate::math::Vec2::new(style::FIELD_PAD_X, style::FIELD_PAD_Y)
}

impl<S: 'static> Widget<S> for Port<String, S> {
    type Data = StringWidgetData;

    fn layout_style(&self, _state: &DagStructRef<S>, _data: &Self::Data) -> taffy::Style {
        taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Row,
            min_size: taffy::Size {
                width: taffy::Dimension::auto(),
                height: taffy::Dimension::length(24.0),
            },
            padding: taffy::Rect::length(4.0),
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
        ctx.fills.push(RectEntry {
            rect,
            color: style::FIELD_BG.to_linear(),
            z: style::Z_PANEL,
        });
        let display = if data.editing {
            let mut s = data.buffer.clone();
            let c = data.cursor.min(s.len());
            s.insert(c, '|');
            s
        } else {
            self.read(state).clone()
        };
        ctx.texts.push(
            TextObject::new(display)
                .at_position(text_pos(rect))
                .with_font_size(style::FIELD_FONT_SIZE)
                .with_z(style::Z_TEXT),
        );
    }

    fn on_event(
        &self,
        state: &mut DagStructRef<S>,
        data: &mut Self::Data,
        event: &InputEvent,
        rect: Rect,
    ) -> EventResponse {
        match event {
            InputEvent::MouseDown { pos, .. } => {
                if rect.contains(*pos) {
                    data.buffer = self.read(state).clone();
                    data.cursor = data.buffer.len();
                    data.editing = true;
                    return EventResponse::Consumed;
                }
                data.editing = false;
                EventResponse::Ignored
            }
            InputEvent::Char { ch } if data.editing => {
                if !ch.is_control() {
                    data.buffer.insert(data.cursor, *ch);
                    // Advance by the char's byte length, not 1 — a multi-byte
                    // char would otherwise leave the cursor mid-char.
                    data.cursor += ch.len_utf8();
                }
                EventResponse::Consumed
            }
            InputEvent::KeyDown { key } if data.editing => match key {
                Key::Backspace => {
                    if data.cursor > 0 {
                        // Remove the whole character before the cursor (the
                        // cursor always sits on a char boundary, but
                        // `cursor - 1` may be inside a multi-byte char).
                        let start = data.buffer.floor_char_boundary(data.cursor - 1);
                        data.buffer.remove(start);
                        data.cursor = start;
                    }
                    EventResponse::Consumed
                }
                Key::ArrowLeft => {
                    // Step back one whole character, not one byte — byte
                    // offsets inside a multi-byte char panic on insert/remove.
                    data.cursor = data
                        .buffer
                        .floor_char_boundary(data.cursor.saturating_sub(1));
                    EventResponse::Consumed
                }
                Key::ArrowRight => {
                    data.cursor = data
                        .buffer
                        .ceil_char_boundary((data.cursor + 1).min(data.buffer.len()));
                    EventResponse::Consumed
                }
                Key::Enter => {
                    let old = self.read(state).clone();
                    let new = data.buffer.clone();
                    if new != old {
                        self.write(state, new);
                    }
                    data.editing = false;
                    EventResponse::Consumed
                }
                Key::Escape => {
                    data.editing = false;
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
