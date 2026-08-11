use crate::graph::DagStructRef;
use crate::gui::event::{InputEvent, MouseButton};
use crate::gui::style;
use crate::math::Vec2;
use crate::rect::Rect;
use crate::shader::RectEntry;
use crate::text::TextObject;

use super::layout::{container_table_rects, split_margin_rect, split_rects, tab_content_rect};
use super::node::{SplitDir, SplitNode, TabNode, ViewportNode};

/// Outcome of handling an event while the context menu is open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContextMenuOutcome {
    /// Event was handled by the menu (hover change, item click).
    Consumed,
    /// Event was ignored; let it pass through to normal routing.
    Ignored,
}

/// A right-click context menu for splitting a ViewportNode.
pub struct ContextMenu {
    /// The menu's top-left corner in physical pixels.
    pub pos: Vec2,
    /// The rect of the node the menu was opened on.
    pub target_rect: Rect,
    /// The hovered menu item index (0 = split horizontal, 1 = split vertical).
    pub hover: usize,
}

impl ContextMenu {
    /// Try to open at the given position. Returns `Some` if a viewport node
    /// was found under the cursor.
    pub fn try_open<S>(
        node: &ViewportNode<S>,
        root_rect: Rect,
        pos: Vec2,
        state: &DagStructRef<S>,
    ) -> Option<Self> {
        find_context_target(node, root_rect, pos, state)
    }

    /// Handle an event while the menu is open.
    pub fn on_event<S>(
        &mut self,
        event: &InputEvent,
        tree: &mut ViewportNode<S>,
        root_rect: Rect,
        state: &mut DagStructRef<S>,
    ) -> ContextMenuOutcome {
        match event {
            InputEvent::MouseMove { pos, .. } => {
                let mr = self.rect();
                if mr.contains(*pos) {
                    let rel_y = pos.y - mr.pos.y;
                    if rel_y >= 0.0 && rel_y < style::MENU_ITEM_H {
                        self.hover = 0;
                    } else if rel_y >= style::MENU_ITEM_H && rel_y < 2.0 * style::MENU_ITEM_H {
                        self.hover = 1;
                    }
                }
                ContextMenuOutcome::Consumed
            }
            InputEvent::MouseDown { pos, button, .. } => {
                if *button == MouseButton::Left {
                    if let Some(action) = self.hit(*pos) {
                        let target_rect = self.target_rect;
                        let dir = match action {
                            0 => SplitDir::Horizontal,
                            _ => SplitDir::Vertical,
                        };
                        execute_split(tree, root_rect, target_rect, dir, state);
                    }
                }
                ContextMenuOutcome::Consumed
            }
            InputEvent::MouseUp { .. } => ContextMenuOutcome::Consumed,
            _ => ContextMenuOutcome::Ignored,
        }
    }

    /// Push render data for the menu popup.
    pub fn render(&self, fills: &mut Vec<RectEntry>, texts: &mut Vec<TextObject>) {
        let w = 140.0;
        let item_h = style::MENU_ITEM_H;
        let h = 2.0 * item_h;
        let bg = Rect::new(self.pos, Vec2::new(w, h));
        fills.push(RectEntry {
            rect: bg,
            color: style::MENU_BG.to_linear(),
            z: style::Z_MENU,
        });
        for (i, label) in ["Split Horizontal", "Split Vertical"].iter().enumerate() {
            let item_rect = Rect::new(
                self.pos + Vec2::new(0.0, i as f32 * item_h),
                Vec2::new(w, item_h),
            );
            if i == self.hover {
                fills.push(RectEntry {
                    rect: item_rect,
                    color: style::MENU_HOVER.to_linear(),
                    z: style::Z_MENU,
                });
            }
            texts.push(
                TextObject::new(label.to_string())
                    .at_position(
                        self.pos
                            + Vec2::new(style::MENU_PAD_X, i as f32 * item_h + style::MENU_PAD_Y),
                    )
                    .with_font_size(style::MENU_FONT_SIZE)
                    .with_color(style::MENU_TEXT)
                    .with_z(style::Z_MENU),
            );
        }
    }

    /// Bounding rect of the menu popup.
    pub fn rect(&self) -> Rect {
        let w = 140.0;
        let h = 2.0 * style::MENU_ITEM_H;
        Rect::new(self.pos, Vec2::new(w, h))
    }

    /// Returns the action index (0 = horizontal, 1 = vertical) if `pos`
    /// hits a menu item, or `None`.
    fn hit(&self, pos: Vec2) -> Option<usize> {
        let mr = self.rect();
        if !mr.contains(pos) {
            return None;
        }
        let rel_y = pos.y - mr.pos.y;
        if rel_y >= 0.0 && rel_y < style::MENU_ITEM_H {
            Some(0)
        } else if rel_y < 2.0 * style::MENU_ITEM_H {
            Some(1)
        } else {
            None
        }
    }
}

// ── Private helpers ──────────────────────────────────────────────────────

/// Traverses the tree to find the deepest node containing `pos` and returns
/// a `ContextMenu` targeting that node, or `None`.
fn find_context_target<S>(
    node: &ViewportNode<S>,
    rect: Rect,
    pos: Vec2,
    state: &DagStructRef<S>,
) -> Option<ContextMenu> {
    if !rect.contains(pos) {
        return None;
    }
    match node {
        ViewportNode::Widget(_widget) => Some(ContextMenu {
            pos,
            target_rect: rect,
            hover: 0,
        }),
        ViewportNode::Split(split) => {
            let inner = split_margin_rect(rect, split.dir);
            let (r0, r1) = split_rects(inner, split.dir, split.ratio);
            if let Some(menu) = find_context_target(&split.children[0], r0, pos, state) {
                return Some(menu);
            }
            if let Some(menu) = find_context_target(&split.children[1], r1, pos, state) {
                return Some(menu);
            }
            if rect.contains(pos) {
                return Some(ContextMenu {
                    pos,
                    target_rect: rect,
                    hover: 0,
                });
            }
            None
        }
        ViewportNode::Tab(tab) => {
            if tab.tabs.is_empty() {
                return None;
            }
            let active = tab.active.min(tab.tabs.len() - 1);
            if let Some(menu) =
                find_context_target(&tab.tabs[active].1, tab_content_rect(rect), pos, state)
            {
                return Some(menu);
            }
            if rect.contains(pos) {
                return Some(ContextMenu {
                    pos,
                    target_rect: rect,
                    hover: 0,
                });
            }
            None
        }
        ViewportNode::Container(children) => {
            if children.is_empty() {
                return None;
            }
            if let Some(rows) = container_table_rects(children, rect, state) {
                for ((_, child), (_, editor_rect)) in children.iter().zip(rows.iter()) {
                    if editor_rect.contains(pos) {
                        return find_context_target(child, *editor_rect, pos, state);
                    }
                }
            }
            if rect.contains(pos) {
                return Some(ContextMenu {
                    pos,
                    target_rect: rect,
                    hover: 0,
                });
            }
            None
        }
    }
}

/// Replaces the ViewportNode whose rect matches `target_rect` with a
/// `SplitNode` containing the original node and a duplicate.
fn execute_split<S>(
    node: &mut ViewportNode<S>,
    rect: Rect,
    target_rect: Rect,
    dir: SplitDir,
    state: &DagStructRef<S>,
) {
    if rects_equal(rect, target_rect) {
        let duplicate = deep_duplicate_viewport(node);
        let ptr = node as *mut ViewportNode<S>;
        let old = unsafe { std::ptr::read(ptr) };
        unsafe {
            std::ptr::write(
                ptr,
                ViewportNode::Split(SplitNode {
                    dir,
                    ratio: 0.5,
                    children: [Box::new(old), Box::new(duplicate)],
                }),
            );
        }
        return;
    }
    match node {
        ViewportNode::Split(split) => {
            let inner = split_margin_rect(rect, split.dir);
            let (r0, r1) = split_rects(inner, split.dir, split.ratio);
            execute_split(&mut split.children[0], r0, target_rect, dir, state);
            execute_split(&mut split.children[1], r1, target_rect, dir, state);
        }
        ViewportNode::Tab(tab) => {
            if tab.tabs.is_empty() {
                return;
            }
            let active = tab.active.min(tab.tabs.len() - 1);
            execute_split(
                &mut tab.tabs[active].1,
                tab_content_rect(rect),
                target_rect,
                dir,
                state,
            );
        }
        ViewportNode::Widget(_) => {}
        ViewportNode::Container(children) => {
            if let Some(rows) = container_table_rects(children, rect, state) {
                for ((_, child), (_, editor_rect)) in children.iter_mut().zip(rows.iter()) {
                    execute_split(child, *editor_rect, target_rect, dir, state);
                }
            }
        }
    }
}

/// Deep-clones a ViewportNode tree. WidgetNodes are duplicated (sharing
/// port accessors); Split/Tab/Container trees are cloned recursively.
fn deep_duplicate_viewport<S>(node: &ViewportNode<S>) -> ViewportNode<S> {
    match node {
        ViewportNode::Widget(widget) => ViewportNode::Widget(widget.duplicate()),
        ViewportNode::Split(split) => ViewportNode::Split(SplitNode {
            dir: split.dir,
            ratio: split.ratio,
            children: [
                Box::new(deep_duplicate_viewport(&split.children[0])),
                Box::new(deep_duplicate_viewport(&split.children[1])),
            ],
        }),
        ViewportNode::Tab(tab) => ViewportNode::Tab(TabNode {
            tabs: tab
                .tabs
                .iter()
                .map(|(name, child)| (name.clone(), deep_duplicate_viewport(child)))
                .collect(),
            active: tab.active,
        }),
        ViewportNode::Container(children) => ViewportNode::Container(
            children
                .iter()
                .map(|(label, child)| (label.clone(), deep_duplicate_viewport(child)))
                .collect(),
        ),
    }
}

fn rects_equal(a: Rect, b: Rect) -> bool {
    (a.pos.x - b.pos.x).abs() < 0.5
        && (a.pos.y - b.pos.y).abs() < 0.5
        && (a.size.x - b.size.x).abs() < 0.5
        && (a.size.y - b.size.y).abs() < 0.5
}
