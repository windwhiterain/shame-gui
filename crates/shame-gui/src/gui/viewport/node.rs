//! The viewport tree: [`ViewportNode`], its layout containers, and the
//! [`ViewportTree`] root.
//!
//! A viewport tree is a hierarchy of panels — splits, tabs, containers —
//! with [`WidgetNode`]s as leaves. Layout rects are computed per frame
//! during render/event walks; the tree itself stores no layout state.

use crate::gui::widget::WidgetNode;

/// Split direction. `Horizontal` stacks children left/right (vertical
/// divider), `Vertical` stacks them top/bottom (horizontal divider).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitDir {
    /// Children side by side (vertical divider).
    Horizontal,
    /// Children stacked (horizontal divider).
    Vertical,
}

/// A user-resizable split: two children divided by a draggable divider line.
/// `ratio` is the first child's share of the extent (0.0–1.0).
///
/// Construct via [`ViewportNode::split`].
pub struct SplitNode {
    /// The axis along which the children are divided.
    pub dir: SplitDir,
    /// The first child's share of the extent (0.0–1.0).
    pub ratio: f32,
    pub(crate) children: [Box<ViewportNode>; 2],
}

/// A tabbed panel: an arbitrary list of named nodes, one visible at a time.
///
/// Construct via [`ViewportNode::tab`].
pub struct TabNode {
    /// The named tab pages, in order.
    pub tabs: Vec<(String, ViewportNode)>,
    /// The index of the visible tab.
    pub active: usize,
}

/// One node of the viewport tree: a widget, a split, a tabbed panel,
/// or a labelled container of widgets stacked vertically.
///
/// Use the constructor helpers ([`ViewportNode::widget`],
/// [`ViewportNode::split`], [`ViewportNode::tab`],
/// [`ViewportNode::container`]) rather than the raw variants where possible.
pub enum ViewportNode {
    /// A leaf: one widget.
    Widget(WidgetNode),
    /// Two children divided by a draggable divider.
    Split(SplitNode),
    /// Tabbed pages, one visible at a time.
    Tab(TabNode),
    /// A labelled vertical stack of children.
    Container(Vec<(String, ViewportNode)>),
}

impl ViewportNode {
    /// A leaf node wrapping a single widget.
    pub fn widget(node: WidgetNode) -> Self {
        Self::Widget(node)
    }

    /// Two children divided by a draggable divider; `ratio` (0.0–1.0) is
    /// the first child's share of the extent along `dir`.
    pub fn split(dir: SplitDir, ratio: f32, a: ViewportNode, b: ViewportNode) -> Self {
        Self::Split(SplitNode {
            dir,
            ratio,
            children: [Box::new(a), Box::new(b)],
        })
    }

    /// A tabbed panel over arbitrary nodes; the first tab is active.
    pub fn tab(tabs: Vec<(String, ViewportNode)>) -> Self {
        Self::Tab(TabNode { tabs, active: 0 })
    }

    /// A vertical stack of labelled nodes. Layout uses taffy table_rects.
    pub fn container(children: Vec<(String, ViewportNode)>) -> Self {
        Self::Container(children)
    }
}

/// The root of a viewport tree. Rects are computed per frame during
/// render/event walks; nothing here stores layout.
///
/// Wrap in [`Gui::new`](crate::gui::Gui::new) and attach to an
/// [`App`](crate::app::App) with
/// [`App::add_gui`](crate::app::App::add_gui).
pub struct ViewportTree {
    /// The root node of the tree.
    pub root: ViewportNode,
}

impl ViewportTree {
    /// Creates a tree with the given root.
    pub fn new(root: ViewportNode) -> Self {
        Self { root }
    }
}
