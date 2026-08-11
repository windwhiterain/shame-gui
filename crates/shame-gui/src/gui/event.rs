//! Input events, platform-independent key/mouse types, and routing
//! responses. Winit events are converted by `InputEvent::from_winit`.

use crate::math::Vec2;

/// Low-level input events flowing through the viewport tree. Widgets interpret
/// these themselves; application-level actions live on the widgets as
/// callbacks, not in a global message enum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    /// A mouse button was pressed at `pos` (physical pixels, y-down).
    MouseDown {
        /// Cursor position in physical pixels, y-down.
        pos: Vec2,
        /// The pressed button.
        button: MouseButton,
        /// Stylus/tablet pressure, 0.0–1.0. None if not available.
        pressure: Option<f32>,
    },
    /// A mouse button was released at `pos`.
    MouseUp {
        /// Cursor position in physical pixels, y-down.
        pos: Vec2,
        /// The released button.
        button: MouseButton,
        /// Stylus/tablet pressure, 0.0–1.0. None if not available.
        pressure: Option<f32>,
    },
    /// The cursor moved to `pos`.
    MouseMove {
        /// Cursor position in physical pixels, y-down.
        pos: Vec2,
        /// Stylus/tablet pressure, 0.0–1.0. None if not available.
        pressure: Option<f32>,
    },
    /// The scroll wheel moved by `delta` at `pos`.
    Scroll {
        /// Cursor position in physical pixels, y-down.
        pos: Vec2,
        /// Scroll delta (pixels; line deltas are pre-scaled by 16).
        delta: Vec2,
    },
    /// A named key was pressed.
    KeyDown {
        /// The pressed key.
        key: Key,
    },
    /// A named key was released.
    KeyUp {
        /// The released key.
        key: Key,
    },
    /// Printable text was input (letters, digits, punctuation).
    Char {
        /// The input character.
        ch: char,
    },
}

impl InputEvent {
    /// Returns the routing mode for this event.
    pub fn routing(&self) -> RoutingMode {
        match self {
            InputEvent::MouseDown { .. }
            | InputEvent::MouseUp { .. }
            | InputEvent::Scroll { .. } => RoutingMode::Positional,
            InputEvent::MouseMove { .. } => RoutingMode::Broadcast,
            InputEvent::KeyDown { .. } | InputEvent::KeyUp { .. } | InputEvent::Char { .. } => {
                RoutingMode::Focused
            }
        }
    }
}

/// Mouse buttons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseButton {
    /// The primary (left) button.
    Left,
    /// The secondary (right) button — also opens the context menu.
    Right,
    /// The middle button.
    Middle,
}

/// Platform-independent key codes. MVP: only what the example needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    /// Delete the previous character.
    Backspace,
    /// Commit the current edit.
    Enter,
    /// Cancel the current edit.
    Escape,
    /// Move focus to the next widget.
    Tab,
    /// Move the cursor left.
    ArrowLeft,
    /// Move the cursor right.
    ArrowRight,
    /// Move the cursor up.
    ArrowUp,
    /// Move the cursor down.
    ArrowDown,
}

/// How an event is routed through the viewport tree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoutingMode {
    /// Routed to the widget under the cursor position.
    Positional,
    /// Routed to the currently focused widget.
    Focused,
    /// Sent to every widget (e.g. for hover-state clearing).
    Broadcast,
}

/// A widget's verdict on an event: `Consumed` stops routing, `Ignored` lets
/// it keep walking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventResponse {
    /// Stop routing the event to deeper widgets.
    Consumed,
    /// Keep walking the tree with this event.
    Ignored,
}

/// The position of a positional event; `None` for key events.
pub fn event_pos(event: &InputEvent) -> Option<Vec2> {
    match event {
        InputEvent::MouseDown { pos, .. }
        | InputEvent::MouseUp { pos, .. }
        | InputEvent::MouseMove { pos, .. }
        | InputEvent::Scroll { pos, .. } => Some(*pos),
        _ => None,
    }
}

impl InputEvent {
    /// Converts a winit window event. `cursor` is the last known cursor
    /// position (winit's mouse-button events carry no position).
    pub(crate) fn from_winit(
        event: &winit::event::WindowEvent,
        cursor: Vec2,
    ) -> Option<InputEvent> {
        use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
        match event {
            WindowEvent::CursorMoved { position, .. } => Some(InputEvent::MouseMove {
                pos: Vec2::new(position.x as f32, position.y as f32),
                pressure: None,
            }),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    winit::event::MouseButton::Left => MouseButton::Left,
                    winit::event::MouseButton::Right => MouseButton::Right,
                    _ => MouseButton::Middle,
                };
                let event = match state {
                    ElementState::Pressed => InputEvent::MouseDown {
                        pos: cursor,
                        button,
                        pressure: None,
                    },
                    ElementState::Released => InputEvent::MouseUp {
                        pos: cursor,
                        button,
                        pressure: None,
                    },
                };
                Some(event)
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(x * 16.0, y * 16.0),
                    MouseScrollDelta::PixelDelta(pos) => Vec2::new(pos.x as f32, pos.y as f32),
                };
                Some(InputEvent::Scroll { pos: cursor, delta })
            }
            WindowEvent::KeyboardInput { event, .. } => {
                use winit::keyboard::NamedKey;
                // Printable text arrives via `event.text` in winit 0.30.
                // Control characters (backspace '\u{8}', etc.) are NOT emitted
                // as Char — they fall through to the named-key match below.
                if let Some(text) = &event.text {
                    if event.state == ElementState::Pressed {
                        if let Some(ch) = text.chars().next() {
                            if !ch.is_control() {
                                return Some(InputEvent::Char { ch });
                            }
                        }
                    }
                }
                let key = match &event.logical_key {
                    winit::keyboard::Key::Named(named) => match named {
                        NamedKey::Backspace => Key::Backspace,
                        NamedKey::Enter => Key::Enter,
                        NamedKey::Escape => Key::Escape,
                        NamedKey::Tab => Key::Tab,
                        NamedKey::ArrowLeft => Key::ArrowLeft,
                        NamedKey::ArrowRight => Key::ArrowRight,
                        NamedKey::ArrowUp => Key::ArrowUp,
                        NamedKey::ArrowDown => Key::ArrowDown,
                        _ => return None,
                    },
                    // Printable characters arrive via `event.text` (the `Char`
                    // event above); everything else produces no key event.
                    _ => return None,
                };
                let event = match event.state {
                    ElementState::Pressed => InputEvent::KeyDown { key },
                    ElementState::Released => InputEvent::KeyUp { key },
                };
                Some(event)
            }
            _ => None,
        }
    }
}
