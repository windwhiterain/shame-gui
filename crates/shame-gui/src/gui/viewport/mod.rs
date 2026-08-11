//! The viewport tree driver: [`Gui`] routes input and renders a
//! [`ViewportTree`], with sub-modules for node definitions, layout math,
//! event/render walks, and the right-click split menu.

mod context;
mod layout;
mod node;
mod render;

use crate::canvas::Canvas;
use crate::graph::StateArena;
use crate::gui::event::{InputEvent, MouseButton};
use crate::gui::widget::RenderContext;
use crate::math::{Vec2, Vec2u};
use crate::rect::Rect;
use crate::shader::RectEntry;
use crate::sm;
use crate::text::TextSystem;

pub use context::ContextMenu;
pub use node::{SplitDir, SplitNode, TabNode, ViewportNode, ViewportTree};
use render::DragState;

/// The interactive driver over a viewport tree: routes events and renders.
///
/// Attach one to an [`App`](crate::app::App) with
/// [`App::add_gui`](crate::app::App::add_gui). The GUI renders above the
/// app's custom draws (DAG-produced render slots) and receives all window input. Widget edits report
/// back the arena slots they changed (returned from [`Gui::on_event`]) so
/// the app can mark them dirty on the graph.
pub struct Gui {
    /// The viewport tree being driven.
    pub tree: ViewportTree,
    drag: Option<DragState>,
    /// The node id currently holding keyboard focus, if any.
    pub focus: Option<u64>,
    context_menu: Option<ContextMenu>,
}

impl Gui {
    /// Creates a GUI over the given viewport tree.
    pub fn new(tree: ViewportTree) -> Self {
        Self {
            tree,
            drag: None,
            focus: None,
            context_menu: None,
        }
    }

    /// Walks the tree with text support: text objects are collected then
    /// queued into the text system, and extra DAG-produced fills/outlines
    /// are merged before drawing.
    pub(crate) fn render_with_dag(
        &self,
        gpu: &sm::Gpu,
        canvas: &mut Canvas,
        ts: &mut TextSystem,
        arena: &mut StateArena,
        dag_fills: &[RectEntry],
        dag_outlines: &[RectEntry],
    ) {
        self.render_impl(gpu, canvas, ts, arena, dag_fills, dag_outlines);
    }

    fn render_impl(
        &self,
        gpu: &sm::Gpu,
        canvas: &mut Canvas,
        ts: &mut TextSystem,
        arena: &mut StateArena,
        extra_fills: &[RectEntry],
        extra_outlines: &[RectEntry],
    ) {
        let framebuffer = canvas.framebuffer_size();
        let root_rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::from(framebuffer));
        let mut fills = Vec::new();
        let mut outlines = Vec::new();
        let mut texts = Vec::new();
        render::walk_render(
            &self.tree.root,
            root_rect,
            &mut RenderContext {
                fills: &mut fills,
                outlines: &mut outlines,
                texts: &mut texts,
                framebuffer,
            },
            arena,
        );
        if let Some(menu) = &self.context_menu {
            menu.render(&mut fills, &mut texts);
        }
        for t in &texts {
            ts.queue(t.clone());
        }
        fills.extend_from_slice(extra_fills);
        outlines.extend_from_slice(extra_outlines);
        render::batch_draw(gpu, canvas, framebuffer, fills, outlines);
    }

    /// Routes one input event through the GUI. Returns the `PortId`s of
    /// arena slots whose values changed (widget edits) — the caller should
    /// mark them dirty on the graph before the next tick.
    ///
    /// `framebuffer` is the window size in physical pixels (used to compute
    /// the root rect and hit tests).
    pub fn on_event(
        &mut self,
        event: &InputEvent,
        framebuffer: Vec2u,
        arena: &mut StateArena,
    ) -> Vec<crate::graph::PortId> {
        let root_rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::from(framebuffer));
        let mut dirty = Vec::new();

        // ── Phase 1: Context menu (separate event path) ─────────────────
        if let Some(menu) = &mut self.context_menu {
            match menu.on_event(event, &mut self.tree.root, root_rect, arena) {
                context::ContextMenuOutcome::Consumed => {
                    // Dismiss on left-click outside the menu rect.
                    if let InputEvent::MouseDown {
                        pos,
                        button: MouseButton::Left,
                        ..
                    } = event
                    {
                        if !menu.rect().contains(*pos) {
                            self.context_menu = None;
                        }
                    }
                    return dirty;
                }
                context::ContextMenuOutcome::Ignored => {}
            }
        }
        // Try to open context menu on right-click.
        if let InputEvent::MouseDown {
            pos,
            button: MouseButton::Right,
            ..
        } = event
        {
            self.context_menu = ContextMenu::try_open(&self.tree.root, root_rect, *pos, arena);
            return dirty;
        }

        // ── Phase 2: Drag handling ─────────────────────────────────────
        match event {
            InputEvent::MouseDown { pos, .. } => {
                if let Some((split_rect, dir, ratio)) =
                    render::find_split_divider(&self.tree.root, root_rect, *pos)
                {
                    self.drag = Some(DragState {
                        split_rect,
                        dir,
                        start_ratio: ratio,
                        start_pos: *pos,
                    });
                    return dirty;
                }
            }
            InputEvent::MouseMove { pos, .. } => {
                if let Some(drag) = &self.drag {
                    render::apply_drag(&mut self.tree.root, root_rect, drag, *pos);
                    return dirty;
                }
            }
            InputEvent::MouseUp { .. } => {
                if self.drag.take().is_some() {
                    return dirty;
                }
            }
            _ => {}
        }

        // ── Phase 3: Viewport tree walk ────────────────────────────────
        render::walk_event(
            &mut self.tree.root,
            root_rect,
            event,
            &mut self.focus,
            arena,
            &mut dirty,
        );
        dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::port::Port;
    use crate::gui::primitives::number::NumberWidgetData;
    use crate::gui::widget::WidgetNode;

    fn test_widget(arena: &mut StateArena) -> WidgetNode {
        let port = Port::<u32>::new(arena.alloc::<u32>());
        WidgetNode::new(port, NumberWidgetData::default())
    }

    #[test]
    fn split_rects_horizontal() {
        let rect = Rect::new(Vec2::new(10.0, 20.0), Vec2::new(400.0, 300.0));
        let (a, b) = layout::split_rects(rect, SplitDir::Horizontal, 0.25);
        assert_eq!(a, Rect::new(Vec2::new(10.0, 20.0), Vec2::new(100.0, 300.0)));
        assert_eq!(
            b,
            Rect::new(Vec2::new(110.0, 20.0), Vec2::new(300.0, 300.0))
        );
    }

    #[test]
    fn split_rects_vertical() {
        let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(400.0, 300.0));
        let (a, b) = layout::split_rects(rect, SplitDir::Vertical, 0.5);
        assert_eq!(a, Rect::new(Vec2::new(0.0, 0.0), Vec2::new(400.0, 150.0)));
        assert_eq!(b, Rect::new(Vec2::new(0.0, 150.0), Vec2::new(400.0, 150.0)));
    }

    #[test]
    fn split_rects_clamps_ratio() {
        let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(400.0, 300.0));
        let (a, _) = layout::split_rects(rect, SplitDir::Horizontal, 0.0);
        assert_eq!(a.size.x, 40.0);
    }

    #[test]
    fn divider_hit_test() {
        let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(400.0, 300.0));
        assert!(layout::divider_hit(
            rect,
            SplitDir::Horizontal,
            0.5,
            Vec2::new(200.0, 150.0),
            4.0
        ));
        assert!(layout::divider_hit(
            rect,
            SplitDir::Horizontal,
            0.5,
            Vec2::new(204.0, 150.0),
            4.0
        ));
        assert!(!layout::divider_hit(
            rect,
            SplitDir::Horizontal,
            0.5,
            Vec2::new(205.0, 150.0),
            4.0
        ));
        assert!(!layout::divider_hit(
            rect,
            SplitDir::Horizontal,
            0.5,
            Vec2::new(200.0, 305.0),
            4.0
        ));
    }

    #[test]
    fn drag_updates_ratio() {
        let mut arena = StateArena::new();
        let tree = ViewportTree::new(ViewportNode::split(
            SplitDir::Horizontal,
            0.5,
            ViewportNode::Widget(test_widget(&mut arena)),
            ViewportNode::Widget(test_widget(&mut arena)),
        ));
        let mut gui = Gui::new(tree);
        let fb = Vec2u::new(400, 300);
        gui.on_event(
            &InputEvent::MouseDown {
                pos: Vec2::new(200.0, 150.0),
                button: MouseButton::Left,
                pressure: None,
            },
            fb,
            &mut StateArena::new(),
        );
        gui.on_event(
            &InputEvent::MouseMove {
                pos: Vec2::new(250.0, 150.0),
                pressure: None,
            },
            fb,
            &mut StateArena::new(),
        );
        gui.on_event(
            &InputEvent::MouseUp {
                pos: Vec2::new(250.0, 150.0),
                button: MouseButton::Left,
                pressure: None,
            },
            fb,
            &mut StateArena::new(),
        );
        let ViewportNode::Split(split) = &gui.tree.root else {
            panic!("expected split");
        };
        assert!((split.ratio - 0.625).abs() < 0.01);
    }

    #[test]
    fn drag_clamps_ratio() {
        let mut arena = StateArena::new();
        let tree = ViewportTree::new(ViewportNode::split(
            SplitDir::Vertical,
            0.5,
            ViewportNode::Widget(test_widget(&mut arena)),
            ViewportNode::Widget(test_widget(&mut arena)),
        ));
        let mut gui = Gui::new(tree);
        let fb = Vec2u::new(400, 300);
        gui.on_event(
            &InputEvent::MouseDown {
                pos: Vec2::new(200.0, 150.0),
                button: MouseButton::Left,
                pressure: None,
            },
            fb,
            &mut StateArena::new(),
        );
        // drag way past the top edge
        gui.on_event(
            &InputEvent::MouseMove {
                pos: Vec2::new(200.0, -500.0),
                pressure: None,
            },
            fb,
            &mut StateArena::new(),
        );
        let ViewportNode::Split(split) = &gui.tree.root else {
            panic!("expected split");
        };
        assert!((split.ratio - 0.1).abs() < 0.01);
    }

    fn tab_tree(arena: &mut StateArena) -> ViewportTree {
        ViewportTree::new(ViewportNode::tab(vec![
            ("zero".into(), ViewportNode::Widget(test_widget(arena))),
            ("one".into(), ViewportNode::Widget(test_widget(arena))),
        ]))
    }

    #[test]
    fn tab_click_switches_active() {
        let mut arena = StateArena::new();
        let mut tree = tab_tree(&mut arena);
        let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(400.0, 300.0));
        let mut dirty = Vec::new();
        let mut focus = None;
        render::walk_event(
            &mut tree.root,
            rect,
            &InputEvent::MouseDown {
                pos: Vec2::new(300.0, 10.0),
                button: MouseButton::Left,
                pressure: None,
            },
            &mut focus,
            &mut StateArena::new(),
            &mut dirty,
        );
        let ViewportNode::Tab(tab) = &tree.root else {
            panic!("expected tab");
        };
        assert_eq!(tab.active, 1);
    }

    #[test]
    fn tab_render_only_walks_active_child() {
        let mut arena = StateArena::new();
        let tree = tab_tree(&mut arena);
        let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(400.0, 300.0));
        let mut fills = Vec::new();
        let mut outlines = Vec::new();
        let mut texts = Vec::new();
        render::walk_render(
            &tree.root,
            rect,
            &mut RenderContext {
                fills: &mut fills,
                outlines: &mut outlines,
                texts: &mut texts,
                framebuffer: Vec2u::new(400, 300),
            },
            &mut arena,
        );
        // 2 header fills + 2 tab name texts + active child (u32: bg fill + value text)
        assert_eq!(fills.len() + texts.len(), 6);
    }

    #[test]
    fn tab_content_click_goes_to_active_child() {
        let mut arena = StateArena::new();
        let mut tree = tab_tree(&mut arena);
        let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(400.0, 300.0));
        let mut dirty = Vec::new();
        let mut focus = None;
        let response = render::walk_event(
            &mut tree.root,
            rect,
            &InputEvent::MouseDown {
                pos: Vec2::new(100.0, 100.0),
                button: MouseButton::Left,
                pressure: None,
            },
            &mut focus,
            &mut arena,
            &mut dirty,
        );
        assert_eq!(response, crate::gui::event::EventResponse::Consumed);
    }

    #[test]
    fn tab_hides_inactive_split_from_drag() {
        let mut arena = StateArena::new();
        let tree = ViewportTree::new(ViewportNode::tab(vec![
            ("zero".into(), ViewportNode::Widget(test_widget(&mut arena))),
            (
                "one".into(),
                ViewportNode::split(
                    SplitDir::Horizontal,
                    0.5,
                    ViewportNode::Widget(test_widget(&mut arena)),
                    ViewportNode::Widget(test_widget(&mut arena)),
                ),
            ),
        ]));
        let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(400.0, 300.0));
        // the split's divider would sit at x=200 in tab 1's content; while tab
        // 0 is active the divider must not be found
        assert!(
            render::find_split_divider(&tree.root, rect, Vec2::new(200.0, 150.0)).is_none(),
            "hidden tab's divider must not be draggable"
        );
    }
}
