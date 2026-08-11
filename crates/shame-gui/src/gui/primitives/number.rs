//! Numeric editor [`Widget`]s for `Port<f32, S>`, `Port<u32, S>`,
//! `Port<i32, S>`, and `Port<usize, S>`: click to edit, Enter commits,
//! Escape cancels.

use crate::graph::DagStructRef;
use crate::graph::port::Port;
use crate::gui::event::{EventResponse, InputEvent, Key};
use crate::gui::style;
use crate::gui::widget::{RenderContext, Widget, WidgetData};
use crate::rect::Rect;
use crate::shader::RectEntry;
use crate::text::TextObject;

/// Cached state for a numeric input: buffer, cursor, error, and editing flag.
pub struct NumberWidgetData {
    /// The raw text being edited (empty when not editing).
    pub buffer: String,
    /// Cursor position within `buffer`.
    pub cursor: usize,
    /// Whether the user is currently typing a value.
    pub editing: bool,
    /// Parse error message from the last Enter press, if any.
    pub error: Option<String>,
}

impl WidgetData for NumberWidgetData {}

impl Default for NumberWidgetData {
    fn default() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            editing: false,
            error: None,
        }
    }
}

fn layout_style() -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Flex,
        flex_direction: taffy::FlexDirection::Row,
        min_size: taffy::Size {
            width: taffy::Dimension::length(80.0),
            height: taffy::Dimension::length(24.0),
        },
        padding: taffy::Rect::length(4.0),
        ..Default::default()
    }
}

fn text_pos(rect: Rect) -> crate::math::Vec2 {
    rect.pos + crate::math::Vec2::new(style::FIELD_PAD_X, style::FIELD_PAD_Y)
}

macro_rules! impl_number {
    ($t:ty) => {
        impl<S: 'static> Widget<S> for Port<$t, S> {
            type Data = NumberWidgetData;

            fn layout_style(&self, _state: &DagStructRef<S>, _data: &Self::Data) -> taffy::Style {
                layout_style()
            }

            fn render(
                &self,
                state: &mut DagStructRef<S>,
                data: &Self::Data,
                rect: Rect,
                ctx: &mut RenderContext,
            ) {
                if data.editing {
                    let (bg, text_color, border) = if data.error.is_some() {
                        (
                            style::ERROR_BG.to_linear(),
                            style::ERROR_TEXT_COLOR,
                            Some(style::ERROR_BORDER),
                        )
                    } else {
                        (style::FIELD_BG.to_linear(), style::TAB_ACTIVE_TEXT, None)
                    };
                    ctx.fills.push(RectEntry {
                        rect,
                        color: bg,
                        z: style::Z_PANEL,
                    });
                    let mut display = data.buffer.clone();
                    let c = data.cursor.min(display.len());
                    display.insert(c, '|');
                    ctx.texts.push(
                        TextObject::new(display)
                            .at_position(text_pos(rect))
                            .with_font_size(style::FIELD_FONT_SIZE)
                            .with_color(text_color)
                            .with_z(style::Z_TEXT),
                    );
                    if let Some(border_color) = border {
                        ctx.outlines.push(RectEntry {
                            rect,
                            color: border_color.to_linear(),
                            z: style::Z_BORDER,
                        });
                    }
                } else {
                    let value = *self.read(state);
                    ctx.fills.push(RectEntry {
                        rect,
                        color: style::FIELD_BG.to_linear(),
                        z: style::Z_PANEL,
                    });
                    ctx.texts.push(
                        TextObject::new(value.to_string())
                            .at_position(text_pos(rect))
                            .with_font_size(style::FIELD_FONT_SIZE)
                            .with_z(style::Z_TEXT),
                    );
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
                    InputEvent::MouseDown { pos, .. } => {
                        if rect.contains(*pos) {
                            data.buffer = self.read(state).to_string();
                            data.cursor = data.buffer.len();
                            data.editing = true;
                            data.error = None;
                            return EventResponse::Consumed;
                        }
                        data.editing = false;
                        data.error = None;
                        EventResponse::Ignored
                    }
                    InputEvent::Char { ch } if data.editing => {
                        if !ch.is_control() {
                            data.buffer.insert(data.cursor, *ch);
                            data.cursor += 1;
                            data.error = None;
                        }
                        EventResponse::Consumed
                    }
                    InputEvent::KeyDown { key } if data.editing => match key {
                        Key::Backspace => {
                            if data.cursor > 0 {
                                data.buffer.remove(data.cursor - 1);
                                data.cursor -= 1;
                                data.error = None;
                            }
                            EventResponse::Consumed
                        }
                        Key::ArrowLeft => {
                            data.cursor = data.cursor.saturating_sub(1);
                            EventResponse::Consumed
                        }
                        Key::ArrowRight => {
                            data.cursor = (data.cursor + 1).min(data.buffer.len());
                            EventResponse::Consumed
                        }
                        Key::Enter => {
                            match data.buffer.parse::<$t>() {
                                Ok(value) => {
                                    self.write(state, value);
                                    data.editing = false;
                                    data.error = None;
                                }
                                Err(e) => {
                                    data.error = Some(e.to_string());
                                }
                            }
                            EventResponse::Consumed
                        }
                        Key::Escape => {
                            data.editing = false;
                            data.error = None;
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
    };
}

impl_number!(u32);
impl_number!(i32);
impl_number!(f32);
impl_number!(usize);
