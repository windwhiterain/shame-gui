use std::collections::{HashMap, HashSet, VecDeque};

use shame_wgpu as sm;

use crate::graph::arena::StateArena;
use crate::graph::port::{Port, PortGroup, PortId, PortValue};
use crate::graph::source::SourcePorts;
use crate::shader::RectEntry;
use crate::text::TextObject;

/// Built-in render output ports. After each [`Graph::tick`], the app reads
/// the values written here and draws them: `fills` and `outlines` become
/// batched rects, `texts` become text objects queued into the text system.
///
/// Access via [`GraphBuilder::render`] while building the graph, or via
/// `graph.render` on a [`Graph`].
pub struct RenderPorts {
    /// Filled rectangles (see [`RectEntry`](crate::shader::RectEntry)).
    pub fills: Port<Vec<RectEntry>>,
    /// Outlined rectangles (wireframes).
    pub outlines: Port<Vec<RectEntry>>,
    /// Text objects rendered in the text pass.
    pub texts: Port<Vec<TextObject>>,
}

/// A node in the graph.
struct NodeEntry {
    eval: Box<dyn FnMut(&mut StateArena, Option<&sm::Gpu>)>,
    ports: Box<dyn PortGroup>,
    /// Source port IDs this node reads from (dirty-triggers for tick).
    source_input_ids: Vec<PortId>,
}

/// An edge from a source port to a downstream node.
#[derive(Clone)]
struct Edge {
    dest_node: usize,
}

/// The DAG engine. The arena is external — callers pass it to `tick()`.
///
/// A [`Graph`] owns the topology (nodes, edges, topological order). The
/// values live in a [`StateArena`] that the caller supplies to
/// [`Graph::tick`]. The graph is configured through [`Graph::builder`] and
/// sealed with [`Graph::finalize`] before the first tick.
///
/// [`SourcePorts`] and [`RenderPorts`] are fixed at construction:
/// `source` is written by the framework each frame, `render` is read back
/// after each tick. In practice an [`App`](crate::app::App) owns both the
/// arena and the graph, so most users never touch `Graph` directly.
pub struct Graph {
    nodes: Vec<NodeEntry>,
    edges: HashMap<PortId, Vec<Edge>>,
    topo_order: Vec<usize>,
    /// The framework-written source ports (framebuffer, mouse, timing).
    pub source: SourcePorts,
    /// The framework-read render output ports (fills, outlines, texts).
    pub render: RenderPorts,
    finalized: bool,
}

impl Graph {
    /// Creates an empty graph with the given built-in port groups.
    pub fn new(source: SourcePorts, render: RenderPorts) -> Self {
        Self {
            nodes: Vec::new(),
            edges: HashMap::new(),
            topo_order: Vec::new(),
            source,
            render,
            finalized: false,
        }
    }

    /// Opens the builder interface for adding nodes and wiring ports.
    pub fn builder(&mut self) -> GraphBuilder<'_> {
        GraphBuilder { graph: self }
    }

    /// Seals the graph: computes the topological order and flips it into
    /// tick-able state. Panics on cycles.
    pub fn finalize(&mut self) {
        assert!(!self.finalized, "graph already finalized");
        self.finalized = true;
        let n = self.nodes.len();
        if n == 0 {
            return;
        }
        let node_outputs: HashSet<PortId> = self
            .nodes
            .iter()
            .flat_map(|node| node.ports.port_ids())
            .collect();

        let mut in_degree = vec![0u32; n];
        for (src_port, edges) in &self.edges {
            if node_outputs.contains(src_port) {
                for edge in edges {
                    in_degree[edge.dest_node] += 1;
                }
            }
        }
        let mut queue: VecDeque<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        self.topo_order.clear();
        while let Some(idx) = queue.pop_front() {
            self.topo_order.push(idx);
            for out_id in self.nodes[idx].ports.port_ids() {
                if let Some(es) = self.edges.get(&out_id) {
                    for e in es {
                        in_degree[e.dest_node] -= 1;
                        if in_degree[e.dest_node] == 0 {
                            queue.push_back(e.dest_node);
                        }
                    }
                }
            }
        }
        assert_eq!(self.topo_order.len(), n, "graph has a cycle");
    }

    /// True once the graph has been finalized (nodes may be empty).
    pub fn is_finalized(&self) -> bool {
        self.finalized
    }

    /// True if the graph has been finalized and contains at least one node.
    pub fn is_active(&self) -> bool {
        self.finalized && !self.nodes.is_empty()
    }

    /// Per-frame execution. Reads dirty flags from the arena (set by
    /// source port writes, widget edits, and born-dirty allocations).
    /// Every node whose input ports are dirty is evaluated in topological
    /// order. Does NOT clear dirty — that is the caller's responsibility
    /// after Canvas has also consumed them.
    ///
    /// `gpu` is `Some` during render frames (GPU upload nodes need it)
    /// and `None` for CPU-only ticks (tests and `App::step()`).
    pub fn tick(&mut self, arena: &mut StateArena, gpu: Option<&sm::Gpu>) {
        assert!(self.finalized, "call finalize() before tick()");
        let n = self.nodes.len();
        if n == 0 {
            return;
        }

        for &idx in &self.topo_order {
            let node = &mut self.nodes[idx];
            let should_run = node.source_input_ids.is_empty()
                || node.source_input_ids.iter().any(|id| arena.is_dirty(*id));
            if !should_run {
                continue;
            }
            (node.eval)(arena, gpu);
        }
    }
}

/// A [`PortGroup`] backed by a plain `Vec<PortId>`. Used internally by
/// [`GraphBuilder::add_node`] to wrap output port lists, and exposed for
/// test helpers.
pub struct IdGroup {
    pub ids: Vec<PortId>,
}

impl PortGroup for IdGroup {
    fn leaf_count(&self) -> usize {
        self.ids.len()
    }
    fn port_ids(&self) -> Vec<PortId> {
        self.ids.clone()
    }
    fn extend_ids(&self, out: &mut Vec<PortId>) {
        out.extend_from_slice(&self.ids);
    }
    fn set_port_ids(&mut self, _ids: &[PortId]) {}
    fn alloc_slots(_arena: &mut StateArena) -> Self {
        unimplemented!("IdGroup::alloc_slots")
    }
}

/// Build-time interface for constructing the DAG.
///
/// Obtain one from [`App::graph_builder`](crate::app::App::graph_builder).
/// The builder borrows the graph mutably; drop it (end the scope) before
/// calling [`App::finalize_graph`](crate::app::App::finalize_graph).
///
/// Nodes are added with [`add_node`](GraphBuilder::add_node); ports are
/// allocated with [`port`](GraphBuilder::port) /
/// [`port_with`](GraphBuilder::port_with) and can be re-wired with
/// [`connect`](GraphBuilder::connect) / [`connect_group`](GraphBuilder::connect_group).
pub struct GraphBuilder<'a> {
    graph: &'a mut Graph,
}

impl GraphBuilder<'_> {
    /// Allocate a new slot in the arena and return a Port handle to it.
    pub fn port<T: PortValue>(&mut self, arena: &mut StateArena) -> Port<T> {
        let id = arena.alloc::<T>();
        Port::new(id)
    }

    /// Allocate a slot with an initial value.
    pub fn port_with<T: PortValue>(&mut self, arena: &mut StateArena, value: T) -> Port<T> {
        let id = arena.alloc_with(value);
        Port::new(id)
    }

    /// Connect: copies the PortId from src to dst, so both reference
    /// the same arena slot.
    pub fn connect<T: PortValue>(&mut self, src: &Port<T>, dst: &mut Port<T>) {
        dst.set_id(src.id());
    }

    /// Connects two port groups positionally: leaf 0 → leaf 0, leaf 1 → leaf 1.
    pub fn connect_group(&mut self, src: &dyn PortGroup, dst: &mut dyn PortGroup) {
        assert_eq!(
            src.leaf_count(),
            dst.leaf_count(),
            "connect_group: leaf count mismatch"
        );
        dst.set_port_ids(&src.port_ids());
    }

    /// The built-in source ports (framebuffer, mouse, timing).
    pub fn source(&self) -> &SourcePorts {
        &self.graph.source
    }

    /// The built-in render output ports (fills, outlines, texts).
    pub fn render(&self) -> &RenderPorts {
        &self.graph.render
    }

    /// Registers a compute node.
    ///
    /// - `eval` — runs on tick when any input port is dirty; reads inputs
    ///   and writes outputs through the arena. Receives `Option<&sm::Gpu>`
    ///   (`Some` during render frames) for creating GPU resources.
    /// - `inputs` — the ports this node reads from; dirtiness on any of
    ///   them triggers `eval`.
    /// - `outputs` — the ports this node writes; provided for topology
    ///   and dirty propagation.
    ///
    /// Panics if the graph is already finalized.
    pub fn add_node(
        &mut self,
        eval: impl FnMut(&mut StateArena, Option<&sm::Gpu>) + 'static,
        inputs: impl PortGroup,
        outputs: impl PortGroup,
    ) {
        assert!(!self.graph.finalized, "graph is finalized");
        let index = self.graph.nodes.len();
        let input_ids = inputs.ids();
        let output_ids = outputs.ids();

        let e = Edge { dest_node: index };
        for id in &input_ids {
            self.graph.edges.entry(*id).or_default().push(e.clone());
        }

        self.graph.nodes.push(NodeEntry {
            eval: Box::new(eval),
            ports: Box::new(IdGroup { ids: output_ids }),
            source_input_ids: input_ids,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    fn eval_counter() -> (
        Rc<Cell<usize>>,
        Box<dyn FnMut(&mut StateArena, Option<&sm::Gpu>)>,
    ) {
        let c = Rc::new(Cell::new(0usize));
        let c2 = c.clone();
        (c, Box::new(move |_, _| c2.set(c2.get() + 1)))
    }

    fn add_test_node(
        builder: &mut GraphBuilder,
        inputs: impl PortGroup,
        outputs: impl PortGroup,
    ) -> Rc<Cell<usize>> {
        let (counter, mut eval) = eval_counter();
        let out_ids = outputs.ids();
        let out_ids_for_closure = out_ids.clone();
        builder.add_node(
            move |arena, gpu| {
                eval(arena, gpu);
                for id in &out_ids_for_closure {
                    arena.mark_dirty(*id);
                }
            },
            inputs,
            IdGroup { ids: out_ids },
        );
        counter
    }

    fn test_arena() -> StateArena {
        StateArena::new()
    }

    fn test_graph(arena: &mut StateArena) -> Graph {
        let source = SourcePorts {
            framebuffer_size: Port::new(arena.alloc::<crate::math::Vec2u>()),
            mouse_pos: Port::new(arena.alloc::<crate::math::Vec2>()),
            mouse_down: Port::new(arena.alloc::<bool>()),
            scroll_delta: Port::new(arena.alloc::<f32>()),
            delta_time: Port::new(arena.alloc::<f32>()),
            elapsed: Port::new(arena.alloc::<f32>()),
        };
        let render = RenderPorts {
            fills: Port::new(arena.alloc::<Vec<RectEntry>>()),
            outlines: Port::new(arena.alloc::<Vec<RectEntry>>()),
            texts: Port::new(arena.alloc::<Vec<TextObject>>()),
        };
        Graph::new(source, render)
    }

    #[test]
    fn clean_sibling_skipped() {
        let mut arena = test_arena();
        let port_a = Port::<f32>::new(arena.alloc::<f32>());
        let port_b = Port::<f32>::new(arena.alloc::<f32>());
        let mut graph = test_graph(&mut arena);
        let mut builder = graph.builder();

        let ca = add_test_node(&mut builder, port_a, ());
        let cb = add_test_node(&mut builder, port_b, ());

        graph.finalize();
        graph.tick(&mut arena, None);
        assert_eq!(ca.get(), 1);
        assert_eq!(cb.get(), 1);
        arena.clear_dirty();

        arena.mark_dirty(port_a.id());
        graph.tick(&mut arena, None);
        assert_eq!(ca.get(), 2);
        assert_eq!(cb.get(), 1);
    }

    #[test]
    fn dirty_propagates_through_chain() {
        let mut arena = test_arena();
        let port_src = Port::<f32>::new(arena.alloc::<f32>());
        let port_mid = Port::<f32>::new(arena.alloc::<f32>());
        let mut graph = test_graph(&mut arena);
        let mut builder = graph.builder();

        let ca = add_test_node(&mut builder, port_src, port_mid);
        let cb = add_test_node(&mut builder, port_mid, ());

        graph.finalize();
        graph.tick(&mut arena, None);
        assert_eq!(ca.get(), 1);
        assert_eq!(cb.get(), 1);
        arena.clear_dirty();

        arena.mark_dirty(port_src.id());
        graph.tick(&mut arena, None);
        assert_eq!(ca.get(), 2);
        assert_eq!(cb.get(), 2);
    }

    #[test]
    fn node_runs_when_any_input_dirty() {
        let mut arena = test_arena();
        let port_a = Port::<f32>::new(arena.alloc::<f32>());
        let port_b = Port::<f32>::new(arena.alloc::<f32>());
        let mut graph = test_graph(&mut arena);
        let mut builder = graph.builder();

        let counter = add_test_node(&mut builder, (port_a, port_b), ());

        graph.finalize();
        graph.tick(&mut arena, None);
        assert_eq!(counter.get(), 1);
        arena.clear_dirty();

        arena.mark_dirty(port_a.id());
        graph.tick(&mut arena, None);
        assert_eq!(counter.get(), 2);
    }

    #[test]
    fn connect_group_works() {
        let mut arena = test_arena();
        let src = Port::<f32>::new(arena.alloc::<f32>());
        let mut dst = Port::<f32>::new(arena.alloc::<f32>());
        let mut graph = test_graph(&mut arena);
        let mut builder = graph.builder();

        builder.connect_group(&src, &mut dst);
        assert_eq!(src.port_ids(), dst.port_ids());
    }

    #[test]
    fn shared_input_runs_all_consumers() {
        let mut arena = test_arena();
        let port_src = Port::<f32>::new(arena.alloc::<f32>());
        let mut graph = test_graph(&mut arena);
        let mut builder = graph.builder();

        let c1 = add_test_node(&mut builder, port_src, ());
        let c2 = add_test_node(&mut builder, port_src, ());

        graph.finalize();
        graph.tick(&mut arena, None);
        assert_eq!(c1.get(), 1);
        assert_eq!(c2.get(), 1);
        arena.clear_dirty();

        arena.mark_dirty(port_src.id());
        graph.tick(&mut arena, None);
        assert_eq!(c1.get(), 2);
        assert_eq!(c2.get(), 2);
    }
}
