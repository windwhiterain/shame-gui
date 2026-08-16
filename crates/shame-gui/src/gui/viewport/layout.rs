use crate::graph::DagStructRef;
use crate::gui::style;
use crate::math::Vec2;
use crate::rect::Rect;

use super::node::{SplitDir, SplitNode, ViewportNode};

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

impl<S> ViewportNode<S> {
    /// Returns a `taffy::Style` describing this node's preferred size.
    /// Used by parent Containers to lay out children correctly.
    pub fn layout_style(&self, state: &DagStructRef<S>) -> taffy::Style {
        match self {
            ViewportNode::Widget(w) => w.widget.layout_style(state),
            ViewportNode::Split(split) => {
                let a = split.children[0].layout_style(state);
                let b = split.children[1].layout_style(state);
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
                // Only the *active* tab's child contributes its preferred
                // size — switching tabs can change the node's size. That is
                // intentional: inactive tabs are never laid out.
                let active = tab.active.min(tab.tabs.len() - 1);
                let child = tab.tabs[active].1.layout_style(state);
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

/// The minimum extent each child needs along the split axis, from its
/// taffy layout hint. `auto` (no declared minimum — containers, string
/// fields) counts as 0: such children are stretchy and may be squeezed.
fn split_child_min<S>(split: &SplitNode<S>, state: &DagStructRef<S>) -> (f32, f32) {
    let s0 = split.children[0].layout_style(state);
    let s1 = split.children[1].layout_style(state);
    let axis = |style: &taffy::Style| match split.dir {
        SplitDir::Horizontal => dim(style.size.width, style.min_size.width),
        SplitDir::Vertical => dim(style.size.height, style.min_size.height),
    };
    let value = |d: taffy::Dimension| if d.is_auto() { 0.0 } else { d.value() };
    (value(axis(&s0)), value(axis(&s1)))
}

/// The ratio range the children's taffy minimums allow: child 0 gets at
/// least `min0 / extent`, child 1 at least `min1 / extent`. When both
/// minimums cannot fit in `rect` (`lo > hi`), the range degrades to
/// `[0.1, 0.9]` so both panes stay visible.
pub fn split_ratio_range<S>(
    split: &SplitNode<S>,
    rect: Rect,
    state: &DagStructRef<S>,
) -> (f32, f32) {
    let extent = match split.dir {
        SplitDir::Horizontal => rect.size.x,
        SplitDir::Vertical => rect.size.y,
    };
    if extent > 0.0 {
        let (min0, min1) = split_child_min(split, state);
        let lo = min0 / extent;
        let hi = 1.0 - min1 / extent;
        if lo <= hi {
            return (lo, hi);
        }
    }
    (0.1, 0.9)
}

/// The split's stored ratio clamped into [`split_ratio_range`] — the value
/// used for all split rect math. A stored ratio outside the range (a
/// default that leaves one pane below its taffy minimum) is auto-adjusted
/// here, per frame, without mutating the stored value.
pub fn effective_ratio<S>(split: &SplitNode<S>, rect: Rect, state: &DagStructRef<S>) -> f32 {
    let (lo, hi) = split_ratio_range(split, rect, state);
    split.ratio.clamp(lo, hi)
}

/// Splits `rect` by `dir` at the effective ratio into the two child rects.
pub fn split_rects<S>(split: &SplitNode<S>, rect: Rect, state: &DagStructRef<S>) -> (Rect, Rect) {
    let ratio = effective_ratio(split, rect, state);
    match split.dir {
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
pub fn divider_rect<S>(split: &SplitNode<S>, rect: Rect, state: &DagStructRef<S>) -> Rect {
    let ratio = effective_ratio(split, rect, state);
    match split.dir {
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
pub fn divider_hit<S>(
    split: &SplitNode<S>,
    rect: Rect,
    pos: Vec2,
    tolerance: f32,
    state: &DagStructRef<S>,
) -> bool {
    let ratio = effective_ratio(split, rect, state);
    match split.dir {
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
pub fn container_table_rects<S>(
    children: &[(String, ViewportNode<S>)],
    rect: Rect,
    state: &DagStructRef<S>,
) -> Option<Vec<(Rect, Rect)>> {
    if children.is_empty() {
        return None;
    }
    let styles: Vec<taffy::Style> = children
        .iter()
        .map(|(_, child)| child.layout_style(state))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::DagStructRef;
    use crate::gui::primitives::number::NumberWidgetData;
    use crate::gui::primitives::string::StringWidgetData;
    use crate::gui::widget::WidgetNode;

    // A small standalone state for split layout tests (no graph needed).
    #[derive(Clone, Default, crate::DagStruct)]
    struct SplitState {
        a: f32,
        b: f32,
        s: String,
    }

    /// A number field: taffy min 80x24.
    fn number_widget(port: crate::graph::port::Port<f32, SplitState>) -> ViewportNode<SplitState> {
        ViewportNode::widget(WidgetNode::new(port, NumberWidgetData::default()))
    }

    /// A string field: no declared min width (stretchy), min height 24.
    fn string_widget(
        port: crate::graph::port::Port<String, SplitState>,
    ) -> ViewportNode<SplitState> {
        ViewportNode::widget(WidgetNode::new(port, StringWidgetData::default()))
    }

    #[test]
    fn split_range_honors_child_mins() {
        let mut state = SplitState {
            a: 0.0,
            b: 0.0,
            s: String::new(),
        };
        let r = DagStructRef::new(&mut state);
        let ports = SplitState::ports();
        let split = ViewportNode::split(
            SplitDir::Horizontal,
            0.5,
            number_widget(ports.a),
            number_widget(ports.b),
        );
        let ViewportNode::Split(split) = &split else {
            unreachable!()
        };
        let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(1000.0, 400.0));
        // Both children need 80px of the 1000px width.
        assert_eq!(split_ratio_range(split, rect, &r), (0.08, 0.92));
        assert_eq!(effective_ratio(split, rect, &r), 0.5);
    }

    #[test]
    fn split_auto_adjusts_default_ratio_to_min() {
        let mut state = SplitState {
            a: 0.0,
            b: 0.0,
            s: String::new(),
        };
        let r = DagStructRef::new(&mut state);
        let ports = SplitState::ports();
        // First child: a nested split of two number fields → min width 160.
        let wide = ViewportNode::split(
            SplitDir::Horizontal,
            0.5,
            number_widget(ports.a),
            number_widget(ports.b),
        );
        let split = ViewportNode::split(SplitDir::Horizontal, 0.2, wide, string_widget(ports.s));
        let ViewportNode::Split(split) = &split else {
            unreachable!()
        };
        let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(500.0, 400.0));
        // Stored 0.2 would leave the wide child at 100px < its 160px min:
        // the effective ratio is auto-adjusted to 160 / 500.
        assert_eq!(split_ratio_range(split, rect, &r), (0.32, 1.0));
        assert!((effective_ratio(split, rect, &r) - 0.32).abs() < 1e-6);
        let (r0, _) = split_rects(split, rect, &r);
        assert!(
            (r0.size.x - 160.0).abs() < 1e-6,
            "child 0 gets its min width"
        );
        assert_eq!(split.ratio, 0.2, "stored ratio is not mutated");
    }

    #[test]
    fn split_mins_cannot_fit_falls_back_to_visibility() {
        let mut state = SplitState {
            a: 0.0,
            b: 0.0,
            s: String::new(),
        };
        let r = DagStructRef::new(&mut state);
        let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(100.0, 400.0));
        // 80 + 80 > 100: the minimums cannot both fit; the range degrades to
        // the always-visible clamp instead of collapsing a pane.
        let split = ViewportNode::split(
            SplitDir::Horizontal,
            0.5,
            number_widget(SplitState::ports().a),
            number_widget(SplitState::ports().b),
        );
        let ViewportNode::Split(split) = &split else {
            unreachable!()
        };
        assert_eq!(split_ratio_range(split, rect, &r), (0.1, 0.9));
        assert_eq!(effective_ratio(split, rect, &r), 0.5);
        let split_hi = ViewportNode::split(
            SplitDir::Horizontal,
            0.99,
            number_widget(SplitState::ports().a),
            number_widget(SplitState::ports().b),
        );
        let ViewportNode::Split(split_hi) = &split_hi else {
            unreachable!()
        };
        assert_eq!(effective_ratio(split_hi, rect, &r), 0.9);
    }
}
