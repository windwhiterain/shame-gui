//! Global visual constants. A proper theme system comes later.
//! All colors are sRGB (the CPU-side authoring space; `Color::to_linear()`
//! converts before instances go to the GPU).

use crate::color::Color;

/// Gap between a widget's label and its editor.
pub const FIELD_GAP: f32 = 8.0;
/// Padding inside an editor field.
pub const FIELD_PADDING: f32 = 6.0;

/// Table mode: label column width and the gap between label and editor.
pub const LABEL_WIDTH: f32 = 96.0;
/// Vertical gap between table rows.
pub const ROW_GAP: f32 = 8.0;

/// Tab bar height (derive tab mode and tree TabNode).
pub const TAB_BAR: f32 = 28.0;

/// Panel background.
pub const PANEL: Color = Color::rgb(0.4100, 0.4100, 0.4366);
/// Editor field background.
pub const FIELD_BG: Color = Color::rgb(0.3318, 0.3318, 0.3656);
/// Split divider line color.
pub const DIVIDER: Color = Color::rgb(0.2478, 0.2478, 0.2478);
/// Focus/hover border color.
pub const BORDER: Color = Color::rgb(0.7674, 0.7674, 0.8267);
/// Checkbox checkmark color.
pub const CHECK_ON: Color = Color::rgb(0.5838, 0.7977, 0.9547);
/// Inactive tab background.
pub const TAB_INACTIVE: Color = Color::rgb(0.3492, 0.3492, 0.3811);
/// Active tab background.
pub const TAB_ACTIVE: Color = Color::rgb(0.4845, 0.5371, 0.6262);

/// Z layers: **smaller z = closer** (shared depth pass uses `less_equal`).
/// The GUI draws in the 0.3–0.5 band; custom render slots / DAG objects
/// should stay below 0.3 to render behind the widgets.
/// Panel background layer.
pub const Z_PANEL: f32 = 0.5;
/// Widget border layer.
pub const Z_BORDER: f32 = 0.45;
/// Split divider layer.
pub const Z_DIVIDER: f32 = 0.4;

/// Text rendering constants.
/// Text layer (shared with user text objects).
pub const Z_TEXT: f32 = 0.3;
/// Label font size in pixels.
pub const LABEL_FONT_SIZE: f32 = 13.0;
/// Editor field font size in pixels.
pub const FIELD_FONT_SIZE: f32 = 14.0;
/// Tab name font size in pixels.
pub const TAB_FONT_SIZE: f32 = 13.0;
/// Label horizontal padding.
pub const LABEL_PAD_X: f32 = 4.0;
/// Label vertical padding.
pub const LABEL_PAD_Y: f32 = 2.0;
/// Field horizontal padding.
pub const FIELD_PAD_X: f32 = 4.0;
/// Field vertical padding.
pub const FIELD_PAD_Y: f32 = 2.0;
/// Tab name horizontal padding.
pub const TAB_PAD_X: f32 = 6.0;
/// Tab name vertical padding.
pub const TAB_PAD_Y: f32 = 2.0;
/// Label text color.
pub const LABEL_TEXT_COLOR: Color = Color::rgb(0.6, 0.6, 0.65);
/// Active tab name color.
pub const TAB_ACTIVE_TEXT: Color = Color::rgb(0.9, 0.9, 0.95);
/// Inactive tab name color.
pub const TAB_INACTIVE_TEXT: Color = Color::rgb(0.5, 0.5, 0.55);
/// Error field background.
pub const ERROR_BG: Color = Color::rgb(0.45, 0.12, 0.12);
/// Error field border.
pub const ERROR_BORDER: Color = Color::rgb(0.75, 0.18, 0.18);
/// Error message text color.
pub const ERROR_TEXT_COLOR: Color = Color::rgb(0.85, 0.35, 0.35);

/// Left margin indent per nesting level, so parent nodes remain clickable.
pub const INDENT: f32 = 8.0;

/// Node outline: a wireframe rect drawn around every ViewportNode so the
/// tree structure is visible.
pub const NODE_OUTLINE: Color = Color::rgb(0.3, 0.3, 0.35);
/// Outline layer.
pub const Z_NODE_OUTLINE: f32 = 0.38;

/// Context menu popup.
/// Menu background.
pub const MENU_BG: Color = Color::rgb(0.2, 0.2, 0.24);
/// Hovered menu item background.
pub const MENU_HOVER: Color = Color::rgb(0.35, 0.5, 0.65);
/// Menu item text color.
pub const MENU_TEXT: Color = Color::rgb(0.85, 0.85, 0.9);
/// Menu item height.
pub const MENU_ITEM_H: f32 = 22.0;
/// Menu horizontal padding.
pub const MENU_PAD_X: f32 = 8.0;
/// Menu vertical padding.
pub const MENU_PAD_Y: f32 = 4.0;
/// Menu font size.
pub const MENU_FONT_SIZE: f32 = 13.0;
/// Menu layer — the smallest z (closest), above everything else.
pub const Z_MENU: f32 = 0.05;
