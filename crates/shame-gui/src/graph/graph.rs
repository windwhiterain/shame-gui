//! The DAG engine: nodes over a typed state `S`.
//!
//! A [`Graph<S>`] organizes nodes whose `eval` closures read and write the
//! state `S` through [`DagStructRef<S>`] + [`Port`](crate::graph::Port)
//! handles. Dirty and fired tracking live on the graph (never on the state,
//! and never visible to user code): a node re-runs when any of its input
//! ports is dirty, and a condition node runs only when its condition port is
//! fired.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::marker::PhantomData;
use std::rc::Rc;

use shame_wgpu as sm;

use crate::graph::element::DagStructRef;
use crate::graph::port::{DagStruct, Port, PortGroup, PortId, PortValue};

/// A node in the graph.
struct NodeEntry<S> {
    eval: Box<dyn FnMut(&mut DagStructRef<S>, Option<&sm::Gpu>)>,
    /// Output port ids (for topology ordering).
    output_ids: Vec<PortId>,
    /// Input port ids (dirty-triggers for tick).
    source_input_ids: Vec<PortId>,
    /// Optional event-condition port. When set, the node is purely
    /// event-driven: it runs only when this port's value is `true`, and the
    /// dirty mechanism on `source_input_ids` is completely bypassed.
    condition: Option<Port<bool, S>>,
    /// For map nodes: the id of the map port this node reads/writes. Used to
    /// break map↔map cycles (two map nodes over the same map) during topo
    /// sort, so chained map stages can coexist without a cycle.
    map_port: Option<PortId>,
}

/// An edge from a source port to a downstream node.
#[derive(Clone)]
struct Edge {
    dest_node: usize,
}

/// The DAG engine, generic over the state type `S`. Values live in the state
/// (a plain `#[derive(DagStruct)]` struct); the graph owns only topology and
/// dirty/fired bookkeeping.
///
/// Nodes can be added at any time via [`Graph::add_node`]. New nodes run
/// unconditionally on their first tick, then only when their input ports are
/// dirty. The topological order is recomputed automatically on the next tick
/// after any node addition.
pub struct Graph<S: DagStruct> {
    nodes: Vec<NodeEntry<S>>,
    edges: HashMap<PortId, Vec<Edge>>,
    topo_order: Vec<usize>,
    /// Node indices that need an unconditional run on the next tick.
    pending_first_run: HashSet<usize>,
    /// True when nodes have been added since the last topo sort.
    topo_stale: bool,
    /// Ports written since the last tick (dirty-triggered evaluation).
    dirty: Rc<RefCell<HashSet<PortId>>>,
    /// Condition ports fired this tick (reset to `false` at the end).
    fired: Rc<RefCell<HashSet<PortId>>>,
    /// Fired condition port → its `Port` handle (for the end-of-tick reset).
    condition_ports: HashMap<PortId, Port<bool, S>>,
    /// Maps condition ports to the node indices that are triggered when the
    /// port value is `true`.
    pub condition_map: HashMap<PortId, Vec<usize>>,
    /// Per-port map-level dirty records (key ops / full flag), shared with
    /// [`DagStructRef`] and consumed by `add_map_node` for incremental
    /// reprocessing. Stored type-erased per map key type.
    map_dirty: Rc<RefCell<HashMap<PortId, Box<dyn std::any::Any>>>>,
}

impl<S: DagStruct> Graph<S> {
    /// Creates an empty graph.
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: HashMap::new(),
            topo_order: Vec::new(),
            pending_first_run: HashSet::new(),
            topo_stale: false,
            dirty: Rc::new(RefCell::new(HashSet::new())),
            fired: Rc::new(RefCell::new(HashSet::new())),
            condition_ports: HashMap::new(),
            condition_map: HashMap::new(),
            map_dirty: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    fn recompute_topo(&mut self) {
        let n = self.nodes.len();
        if n == 0 {
            self.topo_order.clear();
            self.topo_stale = false;
            return;
        }

        // in_degree[dest] = number of (producer, port) pairs where the
        // producer (other than `dest`) outputs a port that `dest` reads.
        // A node reading its own output (in-place update) does not depend
        // on itself, so self-edges contribute nothing. Two map nodes over the
        // same map also do not depend on each other (they are chained
        // pipeline stages ordered by insertion), so those edges are skipped
        // too — this removes the map↔map cycle while keeping edges from map
        // nodes to non-map readers intact.
        let mut in_degree = vec![0u32; n];
        for p in 0..n {
            for port in self.nodes[p].output_ids.clone() {
                if let Some(edges) = self.edges.get(&port) {
                    for e in edges {
                        if e.dest_node != p
                            && !(self.nodes[p].map_port == Some(port)
                                && self.nodes[e.dest_node].map_port == Some(port))
                        {
                            in_degree[e.dest_node] += 1;
                        }
                    }
                }
            }
        }

        let mut queue: VecDeque<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        self.topo_order.clear();
        while let Some(p) = queue.pop_front() {
            self.topo_order.push(p);
            for port in self.nodes[p].output_ids.clone() {
                if let Some(edges) = self.edges.get(&port) {
                    for e in edges {
                        if e.dest_node != p
                            && !(self.nodes[p].map_port == Some(port)
                                && self.nodes[e.dest_node].map_port == Some(port))
                        {
                            in_degree[e.dest_node] -= 1;
                            if in_degree[e.dest_node] == 0 {
                                queue.push_back(e.dest_node);
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(self.topo_order.len(), n, "graph has a cycle");
        self.topo_stale = false;
    }

    /// True if the graph contains at least one node.
    pub fn is_active(&self) -> bool {
        !self.nodes.is_empty()
    }

    /// Borrows the state through a [`DagStructRef`] that routes writes into
    /// this graph's dirty/fired sets. Used by the framework to write source
    /// fields (marking them dirty) and by the GUI render walk.
    pub fn with_state<'a>(&'a self, state: &'a mut S) -> DagStructRef<'a, S> {
        DagStructRef::new_with(
            state,
            self.dirty.clone(),
            self.fired.clone(),
            self.map_dirty.clone(),
        )
    }

    /// Registers a compute node.
    ///
    /// - `eval` — runs on tick when any input port is dirty; reads inputs
    ///   and writes outputs through the [`DagStructRef`] + `Port` handles.
    /// - `inputs` — the ports this node reads from; dirtiness on any of
    ///   them triggers `eval` (unless `condition` is set, which bypasses dirty).
    /// - `outputs` — the ports this node writes; provided for topology and
    ///   dirty propagation.
    /// - `condition` — an optional event trigger. When set, the node is
    ///   purely event-driven: it runs only when the condition port's value
    ///   is `true`, and the dirty mechanism on `inputs` is ignored.
    pub fn add_node(
        &mut self,
        eval: impl FnMut(&mut DagStructRef<S>, Option<&sm::Gpu>) + 'static,
        inputs: impl PortGroup<S>,
        outputs: impl PortGroup<S>,
        condition: Option<Port<bool, S>>,
    ) {
        let index = self.nodes.len();
        let input_ids = inputs.ids();
        let output_ids = outputs.ids();

        let e = Edge { dest_node: index };
        for id in &input_ids {
            self.edges.entry(*id).or_default().push(e.clone());
        }

        if let Some(ref cond) = condition {
            self.edges.entry(cond.id()).or_default().push(e.clone());
            self.condition_map.entry(cond.id()).or_default().push(index);
            self.condition_ports.insert(cond.id(), *cond);
        }

        self.nodes.push(NodeEntry {
            eval: Box::new(eval),
            output_ids,
            source_input_ids: input_ids,
            condition,
            map_port: None,
        });
        self.pending_first_run.insert(index);
        self.topo_stale = true;
    }

    /// Registers a per-element fan-out node over a `HashMap<K, E>`.
    ///
    /// The node reads `map` and, for each element that needs reprocessing,
    /// clones it, wraps it in a `DagStructRef<E>` whose **element-port dirty
    /// set and nested map-dirty store are shared with the graph**, and runs
    /// `eval` against it together with the global state. The result is
    /// written back into the map.
    ///
    /// - `global_in` — global (`S`) ports the eval reads. A dirty global
    ///   input triggers a **full refresh** (every key reprocessed).
    /// - `global_out` — global (`S`) ports the eval writes (topology).
    /// - `elem_in` / `elem_out` — element (`E`) ports the eval reads/writes.
    ///   An element is reprocessed when any of its `elem_in` ports was marked
    ///   dirty (by a prior map stage's `elem_out` write, or a whole-element
    ///   `MapEntry::read_mut`); writing an `elem_out` port marks it dirty for
    ///   downstream map stages.
    /// - `eval` — `FnMut(&mut DagStructRef<S>, Option<&Gpu>, &K, &mut DagStructRef<E>)`.
    ///
    /// Dirty is **write-based and record-driven**, mirroring the global
    /// graph: a whole-map `Port::write`/`read_mut` reprocesses every element;
    /// `Port::insert`/`remove` record the exact key (new insert → process the
    /// element once; remove → no reprocessing, downstream readers re-run);
    /// `MapEntry::read_mut` affects only its key; and per-element-port writes
    /// propagate through chained map stages. There is no structural diff —
    /// the dirty records are the single source of truth (the node reprocesses
    /// every current key on its first run).
    pub fn add_map_node<K, E, F>(
        &mut self,
        map: Port<HashMap<K, E>, S>,
        global_in: impl PortGroup<S>,
        global_out: impl PortGroup<S>,
        elem_in: impl PortGroup<E>,
        elem_out: impl PortGroup<E>,
        eval: F,
    ) where
        K: Clone + Eq + std::hash::Hash + 'static,
        E: Clone + 'static,
        F: FnMut(&mut DagStructRef<S>, Option<&sm::Gpu>, &K, &mut DagStructRef<E>) + 'static,
    {
        let map_id = map.id();
        let global_in_ids = global_in.ids();
        let elem_in_ids = elem_in.ids();
        let map_in = map;
        let map_out = map;
        let mut started = false;
        let mut eval = eval;

        self.add_node(
            move |gref: &mut DagStructRef<S>, gpu: Option<&sm::Gpu>| {
                let global_refresh = global_in_ids.iter().any(|id| gref.is_dirty(*id));
                let full = global_refresh || !started;
                started = true;
                run_map_fanout(gref, gpu, map_in, &elem_in_ids, full, &mut eval);
            },
            (global_in, map_in),
            (global_out, map_out),
            None,
        );

        // Tag this node as a map node over `map`, so topo sort can break
        // map↔map cycles between chained stages.
        self.nodes.last_mut().expect("just added a node").map_port = Some(map_id);

        // `elem_out` is consumed only for its type/group at the call site; the
        // actual per-element-port dirty propagation happens through the shared
        // element-port set inside the eval.
        let _ = elem_out;
    }

    /// Registers a **nested** fan-out: the elements of an outer map are
    /// states that hold their own `HashMap` field, and this node fans out
    /// over that inner map per element.
    ///
    /// The graph stays flat — this registers one more node over the outer
    /// map whose eval iterates the outer map and drives the same fan-out
    /// phases per element over the element's inner map. The element's nested
    /// dirty records (shared into the element's `DagStructRef` by the outer
    /// map node) tell the node exactly which inner keys changed, so only
    /// those reprocess.
    ///
    /// - `outer_map` — the outer `Port<HashMap<A, E>, S>`; typically also
    ///   registered as a map node. Register this node **before** that one, so
    ///   its per-element marks reach the outer node's `elem_in` in the same
    ///   tick (map↔map edges are skipped in topo sort, so insertion order
    ///   governs).
    /// - `inner_map` — a `Port<HashMap<B, L>, E>` map field of the element
    ///   state.
    /// - `elem_in` / `elem_out` — element (`L`) ports the eval reads/writes.
    /// - `eval` — `FnMut(&mut DagStructRef<E>, Option<&Gpu>, &B,
    ///   &mut DagStructRef<L>)`; the first argument is the element itself.
    ///
    /// An element gets a **full inner pass** when it is fresh — newly
    /// inserted, overwritten, whole-element `MapEntry::read_mut`,
    /// whole-outer-map write, or this node's first run; otherwise only the
    /// inner keys recorded by the element's nested dirty store are
    /// reprocessed. The outer map port is marked dirty on any write, so
    /// non-map readers re-derive in the same tick.
    pub fn add_map_node_nested<A, E, B, L, F>(
        &mut self,
        outer_map: Port<HashMap<A, E>, S>,
        inner_map: Port<HashMap<B, L>, E>,
        elem_in: impl PortGroup<L>,
        elem_out: impl PortGroup<L>,
        eval: F,
    ) where
        A: Clone + Eq + std::hash::Hash + 'static,
        E: Clone + 'static,
        B: Clone + Eq + std::hash::Hash + 'static,
        L: Clone + 'static,
        F: FnMut(&mut DagStructRef<E>, Option<&sm::Gpu>, &B, &mut DagStructRef<L>) + 'static,
    {
        let outer_id = outer_map.id();
        let inner_id = inner_map.id();
        let elem_in_ids = elem_in.ids();
        let outer_in = outer_map;
        let outer_out = outer_map;
        let mut started = false;
        let mut eval = eval;

        self.add_node(
            move |gref: &mut DagStructRef<S>, gpu: Option<&sm::Gpu>| {
                // The outer record's clone shares the per-element `Rc`
                // handles, so `keys[a].nested` reads the live nested records.
                let md = gref.snapshot_map_dirty::<A>(outer_id);

                let mut wrote = false;

                if !started || md.full {
                    // First run, or the whole outer map was rewritten → full
                    // inner pass over every current element.
                    started = true;
                    for a in outer_in.read(gref).keys().cloned().collect::<Vec<_>>() {
                        wrote |= process_nested_element(
                            gref,
                            gpu,
                            outer_in,
                            &a,
                            inner_map,
                            &elem_in_ids,
                            true,
                            &mut eval,
                        );
                    }
                } else {
                    // Record-driven: elements that are fresh per the outer
                    // record (insert / overwrite / whole-element mutation),
                    // or whose nested record has changes for our inner map.
                    let keys: Vec<(A, bool)> = {
                        let cur = outer_in.read(gref);
                        md.keys
                            .iter()
                            .filter(|(a, ed)| {
                                cur.contains_key(a)
                                    && (ed.added
                                        || ed.full
                                        || ed.nested.borrow().contains_key(&inner_id))
                            })
                            .map(|(a, ed)| (a.clone(), ed.added || ed.full))
                            .collect()
                    };
                    for (a, fresh) in keys {
                        wrote |= process_nested_element(
                            gref,
                            gpu,
                            outer_in,
                            &a,
                            inner_map,
                            &elem_in_ids,
                            fresh,
                            &mut eval,
                        );
                    }
                }

                // Propagate to downstream (non-map) readers.
                if wrote {
                    gref.mark_dirty(outer_id);
                }
            },
            outer_in,
            outer_out,
            None,
        );

        // Tag this node as a map node over the outer map, so topo sort can
        // break map↔map cycles with the outer map node (insertion order
        // governs their run order).
        self.nodes.last_mut().expect("just added a node").map_port = Some(outer_id);

        // `elem_out` is consumed only for its type/group at the call site.
        let _ = elem_out;
    }

    /// Registers a **render-tree** node over `map`: a single flat node that
    /// descends an arbitrarily deep `HashMap` hierarchy via `path` — one
    /// [`MapPath`] per map level and/or one [`FieldPath`] per plain struct
    /// field, ending in [`LeafMarker`] — running `leaf` once per bottom-level
    /// element that needs reprocessing.
    ///
    /// The graph stays flat (one node for the whole tree). Every level fans
    /// out with the same record-driven dirty machinery as [`Graph::add_map_node`],
    /// consuming the element's nested dirty records recursively, so only the
    /// changed subtrees reprocess at any depth. A fresh element (new insert,
    /// overwrite, whole-element mutation, whole-map write, or first run)
    /// gets a full-subtree pass.
    ///
    /// Unlike [`Graph::add_map_node`] — whose eval receives a cloned element
    /// and a write-back — the tree node processes changed elements **in
    /// place**: it borrows each element from the map through the same
    /// [`MapEntry`] path widget code uses, so a per-cell write costs no
    /// element clone and no write-back at any level of the path. The leaf
    /// eval's writes land directly in the live element, and the map port is
    /// marked dirty (per processed key, and when keys were removed) so
    /// downstream readers re-run.
    ///
    /// - `path` — the descent from `map`'s element type `E` down to the
    ///   leaves. `path.map_id()` drives the nested-record check: an element
    ///   whose inner map was written by an earlier stage in the same tick
    ///   reprocesses even when the element itself is not fresh.
    /// - `trigger` — a leaf port id checked against each element's port-dirty
    ///   set: at the top level it re-triggers an element a chained stage
    ///   wrote (leaf-at-top paths pass the leaf read port, e.g. the cpu
    ///   buffer); at the bottom level it is checked against each leaf's
    ///   ports, so a leaf read-port write (chained stage or widget-side
    ///   [`MapEntry::dagref`] access) reprocesses exactly that leaf.
    /// - `leaf` — the bottom-level eval; receives the leaf element ref.
    pub fn add_map_tree_node<K, E, P, F>(
        &mut self,
        map: Port<HashMap<K, E>, S>,
        path: P,
        trigger: PortId,
        leaf: F,
    ) where
        K: Clone + Eq + std::hash::Hash + 'static,
        E: Clone + 'static,
        P: RenderPath<E>,
        F: FnMut(&mut DagStructRef<P::Leaf>, Option<&sm::Gpu>) + 'static,
    {
        let map_id = map.id();
        let path_map_id = path.map_id();
        let mut started = false;
        let mut leaf = leaf;

        self.add_node(
            move |gref: &mut DagStructRef<S>, gpu: Option<&sm::Gpu>| {
                let md = gref.snapshot_map_dirty::<K>(map_id);
                let full = !started || md.full;
                started = true;

                // Decide which keys to process — keys only, never element
                // clones: the elements are mutated **in place** below, so the
                // whole-element clone + write-back is gone from the per-event
                // path (a paint event used to deep-clone the whole layer and
                // drop it again, O(document), even though one cell changed).
                let (keys, removed_keys): (Vec<K>, Vec<K>) = {
                    let cur: &HashMap<K, E> = map.read(gref);
                    let mut keys = Vec::new();
                    let mut removed_keys = Vec::new();
                    if full {
                        keys.extend(cur.keys().cloned());
                    } else {
                        for (k, ed) in &md.keys {
                            if !cur.contains_key(k) {
                                if ed.removed {
                                    removed_keys.push(k.clone());
                                }
                                continue;
                            }
                            if ed.added
                                || ed.full
                                || path_map_id
                                    .is_some_and(|id| ed.nested.borrow().contains_key(&id))
                                || ed.ports.borrow().contains(&trigger)
                            {
                                keys.push(k.clone());
                            }
                        }
                    }
                    (keys, removed_keys)
                };

                // Process in place through the widget-side `MapEntry` path:
                // `dagref` borrows the live element, shares its port-dirty set
                // and nested store with the graph, and marks the map port
                // dirty — the same marks the old write-back produced, minus
                // the clone and the write-back.
                for k in &keys {
                    let Some(mut entry) = map.get(gref, k.clone()) else { continue };
                    let mut eref = entry.dagref();
                    let fresh = full || md.keys.get(k).map_or(false, |ed| ed.added || ed.full);
                    path.descend(&mut eref, gpu, fresh, trigger, &mut leaf);
                }
                if !removed_keys.is_empty() {
                    gref.mark_dirty(map_id);
                }
            },
            map,
            map,
            None,
        );

        // Tag this node as a map node over `map`, so topo sort can break
        // map↔map cycles with chained stages over the same map (insertion
        // order governs their run order).
        self.nodes.last_mut().expect("just added a node").map_port = Some(map_id);
    }

    /// Per-frame execution. Runs nodes whose input ports are dirty, in
    /// topological order. Dirty marks set by node writes propagate to later
    /// nodes within the same tick; fired conditions are reset at the end.
    ///
    /// `gpu` is `Some` during render frames (GPU upload nodes need it) and
    /// `None` for CPU-only ticks (tests and `App::step()`).
    pub fn tick(&mut self, state: &mut S, gpu: Option<&sm::Gpu>) {
        if self.nodes.is_empty() {
            self.dirty.borrow_mut().clear();
            self.fired.borrow_mut().clear();
            self.map_dirty.borrow_mut().clear();
            return;
        }

        if self.topo_stale {
            self.recompute_topo();
        }

        // Run nodes against the state through a shared dirty/fired ref.
        {
            let dirty = self.dirty.clone();
            let fired = self.fired.clone();
            let map_dirty = self.map_dirty.clone();
            let mut dagref = DagStructRef::new_with(state, dirty, fired, map_dirty);

            for &idx in self.topo_order.iter() {
                let is_new = self.pending_first_run.remove(&idx);
                let node = &mut self.nodes[idx];

                if let Some(cond) = node.condition {
                    if *cond.read(&dagref) {
                        (node.eval)(&mut dagref, gpu);
                    }
                    continue;
                }
                if is_new {
                    (node.eval)(&mut dagref, gpu);
                    continue;
                }
                if !node.source_input_ids.iter().any(|id| dagref.is_dirty(*id)) {
                    continue;
                }
                (node.eval)(&mut dagref, gpu);
            }
        }

        // Reset fired conditions: a condition fired this tick is `true` for
        // the duration of this tick only, and auto-clears for the next.
        let fired_ids: Vec<PortId> = self.fired.borrow_mut().drain().collect();
        for id in fired_ids {
            if let Some(p) = self.condition_ports.get(&id) {
                p.write_state(state, false);
            }
        }

        self.dirty.borrow_mut().clear();
        self.map_dirty.borrow_mut().clear();
    }
}

/// The descent from a map's element type down to the leaves of a render tree.
///
/// Implemented by [`MapPath`] (one more map level), [`FieldPath`] (a plain
/// struct field level), and [`LeafMarker`] (the bottom map's elements are
/// the leaves). [`Graph::add_map_tree_node`] drives the whole path from one
/// flat node: each level fans out with the same record-driven dirty
/// machinery as [`Graph::add_map_node`], so only changed subtrees reprocess
/// at any depth.
pub trait RenderPath<T>: Clone + 'static {
    /// The bottom element type — what the leaf eval runs on.
    type Leaf: Clone + 'static;

    /// `Some(map id)` when this path starts with a map level. The parent
    /// level checks an element's nested dirty record against this id, so an
    /// element whose inner map was written by an earlier stage in the same
    /// tick reprocesses even when the element itself is not fresh.
    fn map_id(&self) -> Option<PortId>;

    /// Fans out over this path's subtree below `gref`, running `leaf` once
    /// per changed bottom-level element. `full` forces a full-subtree pass
    /// (fresh element, whole-map write, or first run). `trigger` is the leaf
    /// read port id: at the bottom level, an element whose ports contain it
    /// (a chained stage or widget-side [`MapEntry::dagref`] write) is
    /// reprocessed even when the element itself is not fresh. Elements are
    /// processed **in place** — borrowed from their map, mutated by `leaf`,
    /// never cloned or written back. Returns true when any element was
    /// processed or any key was removed.
    fn descend(
        &self,
        gref: &mut DagStructRef<T>,
        gpu: Option<&sm::Gpu>,
        full: bool,
        trigger: PortId,
        leaf: &mut impl FnMut(&mut DagStructRef<Self::Leaf>, Option<&sm::Gpu>),
    ) -> bool;

    /// Walks `e`'s subtree, calling `leaf_collect` on every leaf element
    /// (used to gather the leaf GPU slots that form one draw batch).
    fn collect(
        &self,
        e: &T,
        out: &mut Vec<u32>,
        leaf_collect: &mut impl FnMut(&Self::Leaf, &mut Vec<u32>),
    );
}

/// One map level of a [`RenderPath`]: `map` is a field of the parent element
/// type `T`, and `next` describes the descent from its elements.
pub struct MapPath<T, K, E, P: RenderPath<E>>
where
    T: 'static,
    K: Clone + Eq + std::hash::Hash + 'static,
    E: Clone + 'static,
{
    pub map: Port<HashMap<K, E>, T>,
    pub next: P,
}

// `Port` is Copy, so only the next level needs cloning.
impl<
    T: 'static,
    K: Clone + Eq + std::hash::Hash + 'static,
    E: Clone + 'static,
    P: RenderPath<E> + Clone,
> Clone for MapPath<T, K, E, P>
{
    fn clone(&self) -> Self {
        Self {
            map: self.map,
            next: self.next.clone(),
        }
    }
}

impl<T: 'static, K, E, P: RenderPath<E>> RenderPath<T> for MapPath<T, K, E, P>
where
    K: Clone + Eq + std::hash::Hash + 'static,
    E: Clone + 'static,
{
    type Leaf = P::Leaf;

    fn map_id(&self) -> Option<PortId> {
        Some(self.map.id())
    }

    fn descend(
        &self,
        gref: &mut DagStructRef<T>,
        gpu: Option<&sm::Gpu>,
        full: bool,
        trigger: PortId,
        leaf: &mut impl FnMut(&mut DagStructRef<Self::Leaf>, Option<&sm::Gpu>),
    ) -> bool {
        // One fan-out level — the same record-driven decision as
        // `run_map_fanout`, plus the nested-record check: an element whose
        // inner map was written by an earlier stage in this tick reprocesses
        // even when the element itself is not fresh. At the bottom level the
        // trigger port is checked against the element's ports set, so a leaf
        // read-port write (chained stage, or widget-side `dagref` access)
        // reprocesses exactly that leaf.
        let md = gref.snapshot_map_dirty::<K>(self.map.id());
        let full = full || md.full;
        let next_map_id = self.next.map_id();
        let bottom = next_map_id.is_none();

        // Keys only — the elements are processed in place below, so each map
        // level costs O(keys) clones instead of O(element) clones.
        let (keys, removed_keys): (Vec<K>, Vec<K>) = {
            let cur: &HashMap<K, E> = self.map.read(gref);
            let mut keys = Vec::new();
            let mut removed_keys = Vec::new();
            if full {
                keys.extend(cur.keys().cloned());
            } else {
                for (k, ed) in &md.keys {
                    if !cur.contains_key(k) {
                        if ed.removed {
                            removed_keys.push(k.clone());
                        }
                        continue;
                    }
                    if ed.added
                        || ed.full
                        || next_map_id.is_some_and(|id| ed.nested.borrow().contains_key(&id))
                        || (bottom && ed.ports.borrow().contains(&trigger))
                    {
                        keys.push(k.clone());
                    }
                }
            }
            (keys, removed_keys)
        };

        for k in &keys {
            let Some(mut entry) = self.map.get(gref, k.clone()) else { continue };
            let mut eref = entry.dagref();
            let fresh = full || md.keys.get(k).map_or(false, |ed| ed.added || ed.full);
            self.next.descend(&mut eref, gpu, fresh, trigger, leaf);
        }
        if !removed_keys.is_empty() {
            gref.mark_dirty(self.map.id());
        }
        !keys.is_empty() || !removed_keys.is_empty()
    }

    fn collect(
        &self,
        e: &T,
        out: &mut Vec<u32>,
        leaf_collect: &mut impl FnMut(&Self::Leaf, &mut Vec<u32>),
    ) {
        for el in self.map.read_state(e).values() {
            self.next.collect(el, out, leaf_collect);
        }
    }
}

/// One struct-field level of a [`RenderPath`]: `field` is a plain struct
/// field of the parent element type `T`, and `next` describes the descent
/// from its value. Use it to cross a struct level between maps (e.g.
/// `groups → audio (struct) → presets (map)`).
///
/// The field's dirty record (an `ElemDirty`, keyed by the field's id) is
/// shared with the widget-side
/// [`Port::with_field_ref`](crate::graph::Port::with_field_ref), so a leaf
/// edit under the field reprocesses exactly that leaf, a whole-field write
/// (`Port<T, N>::write`/`read_mut` — the derive marks the record `full`)
/// reprocesses the whole subtree, and the field level has no add/remove
/// semantics.
pub struct FieldPath<T, N, P: RenderPath<N>>
where
    T: 'static,
    N: PortValue + 'static,
{
    /// The struct field of `T` whose value the next level descends.
    pub field: Port<N, T>,
    /// The descent from the field's value.
    pub next: P,
}

// `Port` is Copy, so only the next level needs cloning.
impl<T: 'static, N: PortValue + 'static, P: RenderPath<N> + Clone> Clone for FieldPath<T, N, P> {
    fn clone(&self) -> Self {
        Self {
            field: self.field,
            next: self.next.clone(),
        }
    }
}

impl<T: 'static, N: PortValue + 'static, P: RenderPath<N>> RenderPath<T> for FieldPath<T, N, P> {
    type Leaf = P::Leaf;

    fn map_id(&self) -> Option<PortId> {
        Some(self.field.id())
    }

    fn descend(
        &self,
        gref: &mut DagStructRef<T>,
        gpu: Option<&sm::Gpu>,
        full: bool,
        trigger: PortId,
        leaf: &mut impl FnMut(&mut DagStructRef<Self::Leaf>, Option<&sm::Gpu>),
    ) -> bool {
        // The field record decides whether the subtree reprocesses: a
        // whole-field write sets `full`; leaf edits under it record into the
        // shared nested store (checked against the next level's id) or into
        // the record's ports (the trigger check at the bottom level).
        let fd = gref.snapshot_field_dirty(self.field.id());
        let full = full || fd.full;
        let next_id = self.next.map_id();
        let bottom = next_id.is_none();
        let dirty = next_id.is_some_and(|id| fd.nested.borrow().contains_key(&id))
            || (bottom && fd.ports.borrow().contains(&trigger));
        if !full && !dirty {
            return false;
        }
        // Get-or-create the shared record so writes through the field ref
        // stay visible to the record machinery (mirrors `ensure_elem_ports`).
        let (ports, nested) = gref.ensure_field_ref(self.field.id());
        let fired = Rc::new(RefCell::new(HashSet::new()));
        let value = self.field.read_mut_state(gref.inner_mut());
        let mut fref = DagStructRef::new_with(value, ports, fired, nested);
        self.next.descend(&mut fref, gpu, full, trigger, leaf)
    }

    fn collect(
        &self,
        e: &T,
        out: &mut Vec<u32>,
        leaf_collect: &mut impl FnMut(&Self::Leaf, &mut Vec<u32>),
    ) {
        self.next
            .collect(self.field.read_state(e), out, leaf_collect);
    }
}

/// The end of a [`RenderPath`]: the bottom map's elements are the leaves —
/// the leaf eval runs directly on them.
#[derive(Clone, Copy, Default)]
pub struct LeafMarker<L>(PhantomData<fn() -> L>);

impl<L> LeafMarker<L> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<L: Clone + 'static> RenderPath<L> for LeafMarker<L> {
    type Leaf = L;

    fn map_id(&self) -> Option<PortId> {
        None
    }

    fn descend(
        &self,
        gref: &mut DagStructRef<L>,
        gpu: Option<&sm::Gpu>,
        _full: bool,
        _trigger: PortId,
        leaf: &mut impl FnMut(&mut DagStructRef<Self::Leaf>, Option<&sm::Gpu>),
    ) -> bool {
        leaf(gref, gpu);
        true
    }

    fn collect(
        &self,
        e: &L,
        out: &mut Vec<u32>,
        leaf_collect: &mut impl FnMut(&Self::Leaf, &mut Vec<u32>),
    ) {
        leaf_collect(e, out);
    }
}

/// One level of map fan-out, shared by [`Graph::add_map_node`] and nested
/// nodes ([`Graph::add_map_node_nested`]).
///
/// Decides which keys to reprocess from the map port's dirty record — no
/// structural diff: `Port::insert`/`remove`/`read_mut` and whole-map writes
/// all record the exact keys — then runs `eval` per changed element and
/// writes the results back. Returns true if anything was written.
///
/// `full` forces a full refresh (every current key reprocessed); the record's
/// own `full` flag is OR-ed in. When anything is written, the map port is
/// marked dirty so downstream (non-map) readers re-run.
fn run_map_fanout<S, K, E, F>(
    gref: &mut DagStructRef<S>,
    gpu: Option<&sm::Gpu>,
    map: Port<HashMap<K, E>, S>,
    elem_in_ids: &[PortId],
    full: bool,
    eval: &mut F,
) -> bool
where
    K: Clone + Eq + std::hash::Hash + 'static,
    E: Clone + 'static,
    F: FnMut(&mut DagStructRef<S>, Option<&sm::Gpu>, &K, &mut DagStructRef<E>),
{
    // Snapshot the map's dirty record (consumed conceptually; the record
    // itself is cleared at end of tick). The clone shares the per-key `Rc`
    // handles, so element-port sets and nested stores are read live.
    let md = gref.snapshot_map_dirty::<K>(map.id());
    let full = full || md.full;

    // Phase A: decide which keys to process / which were removed (borrows
    // the map, then releases before writing back). Record-driven:
    // `Port::insert`/`remove`/`read_mut` and whole-map writes all record the
    // exact keys in `md`, so no structural diff is needed.
    let (to_process, removed_keys): (Vec<(K, E)>, Vec<K>) = {
        let cur: &HashMap<K, E> = map.read(gref);
        let mut to_process = Vec::new();
        let mut removed_keys = Vec::new();

        if full {
            // Whole-map write, global refresh, or first run → reprocess every
            // current key.
            to_process.extend(cur.iter().map(|(k, v)| (k.clone(), v.clone())));
        } else {
            // Per-key dirty records: new inserts, whole-element mutations,
            // removed keys, or an element-port write that overlaps this
            // node's reads.
            for (k, ed) in &md.keys {
                if !cur.contains_key(k) {
                    if ed.removed {
                        removed_keys.push(k.clone());
                    }
                    continue;
                }
                if ed.added || ed.full {
                    to_process.push((k.clone(), cur[k].clone()));
                } else {
                    let ports = ed.ports.borrow();
                    if elem_in_ids.iter().any(|id| ports.contains(id)) {
                        to_process.push((k.clone(), cur[k].clone()));
                    }
                }
            }
        }
        (to_process, removed_keys)
    };

    // Phase B: process changed elements, sharing this key's element-port
    // dirty set and nested map-dirty store with the eval so its writes (and
    // nested-map writes) propagate.
    let mut wrote = false;
    for (k, e) in to_process {
        let ports_rc = gref.ensure_elem_ports::<K>(map.id(), k.clone());
        let nested = gref.ensure_elem_nested::<K>(map.id(), k.clone());
        let mut e2 = e;
        {
            // Fired is isolated (no consumer yet); the element shares the
            // port dirty set and the nested store.
            let fired = Rc::new(RefCell::new(HashSet::new()));
            let mut eref = DagStructRef::new_with(&mut e2, ports_rc, fired, nested);
            eval(gref, gpu, &k, &mut eref);
        }
        map.read_mut_state(gref.inner_mut()).insert(k, e2);
        wrote = true;
    }

    // Phase C: removed keys are already gone from the map; signal downstream
    // so non-map readers re-derive.
    if !removed_keys.is_empty() {
        wrote = true;
    }

    // Propagate the map change to downstream (non-map) readers. Deliberately
    // avoids `map.write`/`read_mut` so the write-back does not re-record a
    // full change. (For a nested call this marks the inner map port in the
    // element's port set, so the outer map node re-runs that element.)
    if wrote {
        gref.mark_dirty(map.id());
    }

    wrote
}

/// Runs one inner-level fan-out pass over a single element of an outer map:
/// clones the element, runs [`run_map_fanout`] against the element's shared
/// element-port set and nested store, and writes the element back. Returns
/// true if the element was written back.
fn process_nested_element<S, A, E, B, L, F>(
    gref: &mut DagStructRef<S>,
    gpu: Option<&sm::Gpu>,
    outer: Port<HashMap<A, E>, S>,
    a: &A,
    inner: Port<HashMap<B, L>, E>,
    elem_in_ids: &[PortId],
    fresh: bool,
    eval: &mut F,
) -> bool
where
    A: Clone + Eq + std::hash::Hash + 'static,
    E: Clone + 'static,
    B: Clone + Eq + std::hash::Hash + 'static,
    L: Clone + 'static,
    F: FnMut(&mut DagStructRef<E>, Option<&sm::Gpu>, &B, &mut DagStructRef<L>),
{
    let mut e2 = outer.read(gref).get(a).cloned().unwrap();
    let wrote = {
        let ports_rc = gref.ensure_elem_ports::<A>(outer.id(), a.clone());
        let nested = gref.ensure_elem_nested::<A>(outer.id(), a.clone());
        let fired = Rc::new(RefCell::new(HashSet::new()));
        let mut eref = DagStructRef::new_with(&mut e2, ports_rc, fired, nested);
        run_map_fanout(&mut eref, gpu, inner, elem_in_ids, fresh, eval)
    };
    if wrote {
        outer.read_mut_state(gref.inner_mut()).insert(a.clone(), e2);
        true
    } else {
        false
    }
}
