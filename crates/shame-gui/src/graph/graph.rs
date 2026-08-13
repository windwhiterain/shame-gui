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
use std::rc::Rc;

use shame_wgpu as sm;

use crate::graph::element::DagStructRef;
use crate::graph::port::{DagStruct, Port, PortGroup, PortId};

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
    /// set is shared with the graph**, and runs `eval` against it together
    /// with the global state. The result is written back into the map.
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
    /// Dirty is **write-based**, mirroring the global graph: a whole-map
    /// `Port::write`/`read_mut` reprocesses every element; `Port::insert`/
    /// `remove` and `MapEntry::read_mut` affect only the relevant key; and
    /// per-element-port writes propagate through chained map stages.
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
        let mut known_keys: HashSet<K> = HashSet::new();
        let mut eval = eval;

        self.add_node(
            move |gref: &mut DagStructRef<S>, gpu: Option<&sm::Gpu>| {
                let global_refresh = global_in_ids.iter().any(|id| gref.is_dirty(*id));

                // Snapshot the map's dirty record (consumed conceptually; the
                // record itself is cleared at end of tick).
                let md = gref.snapshot_map_dirty::<K>(map_in.id());

                // Phase A: decide which keys to process / remove (borrows the
                // map, then releases before writing back).
                let (to_process, to_remove): (Vec<(K, E)>, Vec<K>) = {
                    let cur: &HashMap<K, E> = map_in.read(gref);
                    let mut to_process = Vec::new();
                    let mut to_remove = Vec::new();

                    if md.full || global_refresh {
                        // Whole-map write or global refresh → reprocess every
                        // current key.
                        to_process.extend(cur.iter().map(|(k, v)| (k.clone(), v.clone())));
                    } else {
                        // Structural diff: new keys → process, removed keys →
                        // drop.
                        for k in cur.keys() {
                            if !known_keys.contains(k) {
                                to_process.push((k.clone(), cur[k].clone()));
                            }
                        }
                        for k in &known_keys {
                            if !cur.contains_key(k) {
                                to_remove.push(k.clone());
                            }
                        }
                        // Per-key dirty records: whole-element mutation, or an
                        // element-port write that overlaps this node's reads.
                        for (k, ed) in &md.keys {
                            if !cur.contains_key(k) {
                                continue;
                            }
                            if ed.full {
                                to_process.push((k.clone(), cur[k].clone()));
                            } else {
                                let ports = ed.ports.borrow();
                                if elem_in_ids.iter().any(|id| ports.contains(id)) {
                                    to_process.push((k.clone(), cur[k].clone()));
                                }
                            }
                        }
                    }
                    (to_process, to_remove)
                };

                // Phase B: process changed elements, sharing this key's
                // element-port dirty set with the eval so its writes propagate.
                let mut wrote = false;
                for (k, e) in to_process {
                    let ports_rc = gref.ensure_elem_ports::<K>(map_in.id(), k.clone());
                    let mut e2 = e;
                    {
                        // Fired + element map-dirty are isolated (no consumer
                        // yet); only the element-port dirty set is shared.
                        let fired = Rc::new(RefCell::new(HashSet::new()));
                        let emd: Rc<RefCell<HashMap<PortId, Box<dyn std::any::Any>>>> =
                            Rc::new(RefCell::new(HashMap::new()));
                        let mut eref = DagStructRef::new_with(&mut e2, ports_rc, fired, emd);
                        eval(&mut *gref, gpu, &k, &mut eref);
                    }
                    map_out.read_mut_state(gref.inner_mut()).insert(k, e2);
                    wrote = true;
                }

                // Phase C: drop removed keys.
                for k in to_remove {
                    map_out.read_mut_state(gref.inner_mut()).remove(&k);
                    wrote = true;
                }

                // Phase D: refresh the known key set to the current map.
                known_keys.clear();
                known_keys.extend(map_in.read_state(gref.inner()).keys().cloned());

                // Propagate the map change to downstream (non-map) readers.
                // Deliberately avoids `map_out.write`/`read_mut` so the map
                // node's own write-back does not re-record a full change.
                if wrote {
                    gref.mark_dirty(map_out.id());
                }
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
