use crate::graph::StateArena;
use crate::gui::style;
use crate::math::Vec2;
use crate::rect::Rect;

use super::node::{SplitDir, ViewportNode};

// ── Dimension arithmetic for layout hints ──────────────────────────────

fn add_dim(a: taffy::Dimension, b: taffy::Dimension) -> taffy::Dimension {
    if a.is_auto() || b.is_auto() {
        taffy::Dimension::auto()
    } else {
        taffy::Dimension::length(a.value() + b.value())
    }
}

fn max_dim(a: taffy::Dimension, b: taffy::Dimension) -> taffy::Dimension {
    if a.is_auto() || b.is_auto() {
        taffy::Dimension::auto()
    } else {
        let x = a.value();
        let y = b.value();
        taffy::Dimension::length(if x > y { x } else { y })
    }
}

/// Resolves a dimension, preferring `size` over `min_size`.
fn dim(sz: taffy::Dimension, min: taffy::Dimension) -> taffy::Dimension {
    if !sz.is_auto() {
        sz
    } else if !min.is_auto() {
        min
    } else {
        taffy::Dimension::auto()
    }
}

// ── ViewportNode layout hint ───────────────────────────────────────────

impl ViewportNode {
    /// Returns a `taffy::Style` describing this node's preferred size.
    /// Used by parent Containers to lay out children correctly.
    pub fn layout_style(&self, arena: &StateArena) -> taffy::Style {
        match self {
            ViewportNode::Widget(w) => w.widget.layout_style(arena),
            ViewportNode::Split(split) => {
                let a = split.children[0].layout_style(arena);
                let b = split.children[1].layout_style(arena);
                let aw = dim(a.size.width, a.min_size.width);
                let ah = dim(a.size.height, a.min_size.height);
                let bw = dim(b.size.width, b.min_size.width);
                let bh = dim(b.size.height, b.min_size.height);
                match split.dir {
                    SplitDir::Horizontal => taffy::Style {
                        size: taffy::Size {
                            width: add_dim(aw, bw),
                            height: add_dim(
                                max_dim(ah, bh),
                                taffy::Dimension::length(style::INDENT),
                            ),
                        },
                        ..Default::default()
                    },
                    SplitDir::Vertical => taffy::Style {
                        size: taffy::Size {
                            width: add_dim(
                                max_dim(aw, bw),
                                taffy::Dimension::length(style::INDENT),
                            ),
                            height: add_dim(ah, bh),
                        },
                        ..Default::default()
                    },
                }
            }
            ViewportNode::Tab(tab) => {
                if tab.tabs.is_empty() {
                    return taffy::Style::default();
                }
                let active = tab.active.min(tab.tabs.len() - 1);
                let child = tab.tabs[active].1.layout_style(arena);
                let cw = dim(child.size.width, child.min_size.width);
                let ch = dim(child.size.height, child.min_size.height);
                taffy::Style {
                    size: taffy::Size {
                        width: cw,
                        height: add_dim(ch, taffy::Dimension::length(style::TAB_BAR)),
                    },
                    ..Default::default()
                }
            }
            ViewportNode::Container(_children) => taffy::Style::default(),
        }
    }
}

// ── Split rect math ────────────────────────────────────────────────────

/// Splits `rect` by `dir` and `ratio` into the two child rects.
pub fn split_rects(rect: Rect, dir: SplitDir, ratio: f32) -> (Rect, Rect) {
    let ratio = ratio.clamp(0.1, 0.9);
    match dir {
        SplitDir::Horizontal => {
            let w0 = rect.size.x * ratio;
            (
                Rect::new(rect.pos, Vec2::new(w0, rect.size.y)),
                Rect::new(
                    Vec2::new(rect.pos.x + w0, rect.pos.y),
                    Vec2::new(rect.size.x - w0, rect.size.y),
                ),
            )
        }
        SplitDir::Vertical => {
            let h0 = rect.size.y * ratio;
            (
                Rect::new(rect.pos, Vec2::new(rect.size.x, h0)),
                Rect::new(
                    Vec2::new(rect.pos.x, rect.pos.y + h0),
                    Vec2::new(rect.size.x, rect.size.y - h0),
                ),
            )
        }
    }
}

/// The divider line of a split, as a 1px-thick rect for rendering.
pub fn divider_rect(rect: Rect, dir: SplitDir, ratio: f32) -> Rect {
    match dir {
        SplitDir::Horizontal => {
            let x = rect.pos.x + rect.size.x * ratio;
            Rect::new(Vec2::new(x - 0.5, rect.pos.y), Vec2::new(1.0, rect.size.y))
        }
        SplitDir::Vertical => {
            let y = rect.pos.y + rect.size.y * ratio;
            Rect::new(Vec2::new(rect.pos.x, y - 0.5), Vec2::new(rect.size.x, 1.0))
        }
    }
}

/// True when `pos` is within `tolerance` pixels of the divider line.
pub fn divider_hit(rect: Rect, dir: SplitDir, ratio: f32, pos: Vec2, tolerance: f32) -> bool {
    match dir {
        SplitDir::Horizontal => {
            let x = rect.pos.x + rect.size.x * ratio;
            (pos.x - x).abs() <= tolerance
                && pos.y >= rect.pos.y - tolerance
                && pos.y <= rect.pos.y + rect.size.y + tolerance
        }
        SplitDir::Vertical => {
            let y = rect.pos.y + rect.size.y * ratio;
            (pos.y - y).abs() <= tolerance
                && pos.x >= rect.pos.x - tolerance
                && pos.x <= rect.pos.x + rect.size.x + tolerance
        }
    }
}

/// The tab header bar of a tabbed node: full width, `TAB_BAR` tall.
pub fn tab_bar_rect(rect: Rect) -> Rect {
    Rect::new(rect.pos, Vec2::new(rect.size.x, style::TAB_BAR))
}

/// The content area below the tab header bar.
pub fn tab_content_rect(rect: Rect) -> Rect {
    Rect::new(
        Vec2::new(rect.pos.x, rect.pos.y + style::TAB_BAR),
        Vec2::new(rect.size.x, (rect.size.y - style::TAB_BAR).max(0.0)),
    )
}

/// The rect available to children of a non-leaf node, after reserving
/// margin pixels on x / y for the parent's clickable strip.
pub fn margin_rect(rect: Rect, mx: f32, my: f32) -> Rect {
    Rect::new(
        Vec2::new(rect.pos.x + mx, rect.pos.y + my),
        Vec2::new(rect.size.x - mx, rect.size.y - my),
    )
}

/// Margin rect for a split node: horizontal split → margin on top,
/// vertical split → margin on left.
pub fn split_margin_rect(rect: Rect, dir: SplitDir) -> Rect {
    match dir {
        SplitDir::Horizontal => margin_rect(rect, 0.0, style::INDENT),
        SplitDir::Vertical => margin_rect(rect, style::INDENT, 0.0),
    }
}

/// Computes `[label_rect | editor_rect]` row pairs for a Container node.
/// Both render and event walks use this to avoid duplicating the taffy layout.
pub fn container_table_rects(
    children: &[(String, ViewportNode)],
    rect: Rect,
    arena: &StateArena,
) -> Option<Vec<(Rect, Rect)>> {
    if children.is_empty() {
        return None;
    }
    let styles: Vec<taffy::Style> = children
        .iter()
        .map(|(_, child)| child.layout_style(arena))
        .collect();
    let container_style = taffy::Style {
        display: taffy::Display::Flex,
        flex_direction: taffy::FlexDirection::Column,
        gap: taffy::Size::length(style::FIELD_GAP),
        padding: taffy::Rect::length(style::FIELD_PADDING),
        ..Default::default()
    };
    Some(crate::gui::layout::table_rects(
        &container_style,
        &styles,
        style::LABEL_WIDTH,
        style::ROW_GAP,
        rect,
    ))
}
