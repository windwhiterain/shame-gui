//! The viewport tree driver: [`Gui`] routes input and renders a
//! [`ViewportTree`], with sub-modules for node definitions, layout math,
//! event/render walks, and the right-click split menu.

pub(crate) mod context;
pub(crate) mod layout;
pub(crate) mod node;
pub(crate) mod render;

use crate::canvas::Canvas;
use crate::graph::DagStructRef;
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
/// app's custom draws (DAG-produced render slots) and receives all window
/// input. Widget edits write through the [`DagStructRef`] and mark ports
/// dirty automatically.
pub struct Gui<S> {
    /// The viewport tree being driven.
    pub tree: ViewportTree<S>,
    drag: Option<DragState>,
    /// The node id currently holding keyboard focus, if any.
    pub focus: Option<u64>,
    context_menu: Option<ContextMenu>,
}

impl<S> Gui<S> {
    /// Creates a GUI over the given viewport tree.
    pub fn new(tree: ViewportTree<S>) -> Self {
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
        canvas: &mut Canvas<S>,
        ts: &mut TextSystem,
        state: &mut DagStructRef<S>,
        dag_fills: &[RectEntry],
        dag_outlines: &[RectEntry],
    ) {
        self.render_impl(gpu, canvas, ts, state, dag_fills, dag_outlines);
    }

    fn render_impl(
        &self,
        gpu: &sm::Gpu,
        canvas: &mut Canvas<S>,
        ts: &mut TextSystem,
        state: &mut DagStructRef<S>,
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
            state,
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

    /// Routes one input event through the GUI. Widget edits write through
    /// `state` (a [`DagStructRef`]) and mark the corresponding ports dirty
    /// automatically.
    pub fn on_event(
        &mut self,
        event: &InputEvent,
        framebuffer: Vec2u,
        state: &mut DagStructRef<S>,
    ) {
        let root_rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::from(framebuffer));

        // ── Phase 1: Context menu (separate event path) ─────────────────
        if let Some(menu) = &mut self.context_menu {
            match menu.on_event(event, &mut self.tree.root, root_rect, state) {
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
                    return;
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
            self.context_menu = ContextMenu::try_open(&self.tree.root, root_rect, *pos, state);
            return;
        }

        // ── Phase 2: Drag handling ─────────────────────────────────────
        match event {
            InputEvent::MouseDown { pos, .. } => {
                if let Some((split_rect, dir, ratio)) =
                    render::find_split_divider(&self.tree.root, root_rect, *pos, &*state)
                {
                    self.drag = Some(DragState {
                        split_rect,
                        dir,
                        start_ratio: ratio,
                        start_pos: *pos,
                    });
                    return;
                }
            }
            InputEvent::MouseMove { pos, .. } => {
                if let Some(drag) = &self.drag {
                    render::apply_drag(&mut self.tree.root, root_rect, drag, *pos, &*state);
                    return;
                }
            }
            InputEvent::MouseUp { .. } => {
                if self.drag.take().is_some() {
                    return;
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
            state,
        );
    }
}
