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
    ///
    /// Stylus/tablet and touch-screen input arrives as winit `Touch` events
    /// (on Windows this is the WM_POINTER path: winit already normalizes pen
    /// pressure to 0.0–1.0 in `force`). It is surfaced as mouse events with
    /// `pressure` set, so widgets that ignore pressure work unchanged and
    /// painting code can treat `pressure == Some(p)` as "a pen/touch stroke".
    pub(crate) fn from_winit(
        event: &winit::event::WindowEvent,
        cursor: Vec2,
    ) -> Option<InputEvent> {
        use winit::event::{ElementState, Force, MouseScrollDelta, TouchPhase, WindowEvent};
        match event {
            WindowEvent::Touch(touch) => {
                let pressure = match touch.force {
                    Some(Force::Normalized(force)) => Some(force.clamp(0.0, 1.0) as f32),
                    Some(Force::Calibrated {
                        force,
                        max_possible_force,
                        ..
                    }) => {
                        let ratio = if max_possible_force > 0.0 {
                            force / max_possible_force
                        } else {
                            0.0
                        };
                        Some(ratio.clamp(0.0, 1.0) as f32)
                    }
                    None => None,
                };
                let pos = Vec2::new(touch.location.x as f32, touch.location.y as f32);
                match touch.phase {
                    TouchPhase::Started => Some(InputEvent::MouseDown {
                        pos,
                        button: MouseButton::Left,
                        pressure,
                    }),
                    TouchPhase::Moved => Some(InputEvent::MouseMove { pos, pressure }),
                    TouchPhase::Ended | TouchPhase::Cancelled => Some(InputEvent::MouseUp {
                        pos,
                        button: MouseButton::Left,
                        pressure,
                    }),
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use winit::event::{DeviceId, Force, Touch, TouchPhase, WindowEvent};

    fn touch(phase: TouchPhase, force: Option<Force>) -> InputEvent {
        InputEvent::from_winit(
            &WindowEvent::Touch(Touch {
                device_id: DeviceId::dummy(),
                phase,
                location: (320.0, 240.0).into(),
                id: 1,
                force,
            }),
            Vec2::new(0.0, 0.0),
        )
        .unwrap()
    }

    #[test]
    fn pen_down_carries_normalized_pressure() {
        let event = touch(TouchPhase::Started, Some(Force::Normalized(0.5)));
        assert_eq!(
            event,
            InputEvent::MouseDown {
                pos: Vec2::new(320.0, 240.0),
                button: MouseButton::Left,
                pressure: Some(0.5),
            }
        );
    }

    #[test]
    fn pen_move_carries_calibrated_pressure() {
        let event = touch(
            TouchPhase::Moved,
            Some(Force::Calibrated {
                force: 2.0,
                max_possible_force: 4.0,
                altitude_angle: None,
            }),
        );
        assert_eq!(
            event,
            InputEvent::MouseMove {
                pos: Vec2::new(320.0, 240.0),
                pressure: Some(0.5),
            }
        );
    }

    #[test]
    fn pen_up_and_cancel_map_to_mouse_up() {
        for phase in [TouchPhase::Ended, TouchPhase::Cancelled] {
            assert_eq!(
                touch(phase, Some(Force::Normalized(0.0))),
                InputEvent::MouseUp {
                    pos: Vec2::new(320.0, 240.0),
                    button: MouseButton::Left,
                    pressure: Some(0.0),
                }
            );
        }
    }

    #[test]
    fn touch_without_force_has_no_pressure() {
        assert_eq!(
            touch(TouchPhase::Moved, None),
            InputEvent::MouseMove {
                pos: Vec2::new(320.0, 240.0),
                pressure: None,
            }
        );
    }
}
