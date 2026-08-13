//! Dynamic HashMap fan-out: `Graph::add_map_node` processes a
//! `HashMap<K, E>` element-by-element, reprocessing only the elements whose
//! inputs became dirty.
//!
//! This exercises the core feature without a winit loop: a state struct
//! holding a `HashMap<u32, Element>`, plus an upstream node that seeds the
//! map, and a map node that squares each element's input into its output.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use shame_gui::DagStruct;
use shame_gui::graph::DagStructRef;
use shame_gui::state;

/// The per-element state: `x` is the input, `y` the computed output.
#[derive(Clone, Default, PartialEq, Debug, DagStruct)]
pub struct Element {
    pub x: f32,
    pub y: f32,
}

/// App state: built-in fields (injected by `#[state]`) + the map being fanned out.
#[state]
#[derive(Clone, Default, DagStruct)]
pub struct MapState {
    pub elements: HashMap<u32, Element>,
}

pub fn build() -> (
    shame_gui::graph::Graph<MapState>,
    shame_gui::graph::Port<HashMap<u32, Element>, MapState>,
    Rc<Cell<usize>>,
) {
    let mut graph = shame_gui::graph::Graph::new();
    let counter = Rc::new(Cell::new(0usize));

    let p = MapState::ports();
    let map = p.elements;

    // Upstream: seed the map with three elements (run once on first tick).
    {
        let m = map;
        graph.add_node(
            move |gref: &mut DagStructRef<MapState>, _gpu| {
                let mut cur = m.read(gref).clone();
                if cur.is_empty() {
                    cur.insert(1, Element { x: 2.0, y: 0.0 });
                    cur.insert(2, Element { x: 3.0, y: 0.0 });
                    cur.insert(3, Element { x: 4.0, y: 0.0 });
                    m.write(gref, cur);
                }
            },
            (),
            map,
            None,
        );
    }

    // Fan-out: square x into y, per element.
    {
        let c = counter.clone();
        let el = Element::ports();
        graph.add_map_node(
            map,
            (),
            (),
            el.x,
            el.y,
            move |_gref, _gpu, _key, e: &mut DagStructRef<Element>| {
                c.set(c.get() + 1);
                let x = *el.x.read(e);
                el.y.write(e, x * x);
            },
        );
    }

    (graph, map, counter)
}

#[test]
fn map_node_first_tick_processes_all_seeded() {
    let (mut graph, map, counter) = build();
    let mut state = MapState::default();

    graph.tick(&mut state, None);
    let elements = map.read_state(&state);
    assert_eq!(counter.get(), 3, "all three seeded keys processed once");
    assert_eq!(elements[&1].y, 4.0);
    assert_eq!(elements[&2].y, 9.0);
    assert_eq!(elements[&3].y, 16.0);
}

#[test]
fn whole_map_write_reprocesses_all_keys() {
    let (mut graph, map, counter) = build();
    let mut state = MapState::default();
    graph.tick(&mut state, None); // seed + first fan-out
    assert_eq!(counter.get(), 3);

    // A whole-map `read_mut` is coarse: every element reprocesses.
    {
        let mut r = graph.with_state(&mut state);
        map.read_mut(&mut r).get_mut(&2).unwrap().x = 10.0;
    }

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 6, "whole-map mutation reprocesses all keys");
    let elements = map.read_state(&state);
    assert_eq!(elements[&2].y, 100.0);
    assert_eq!(elements[&1].y, 4.0, "other keys recomputed to same value");
    assert_eq!(elements[&3].y, 16.0);
}

#[test]
fn map_entry_read_mut_only_reprocesses_that_key() {
    let (mut graph, map, counter) = build();
    let mut state = MapState::default();
    graph.tick(&mut state, None); // seed + first fan-out
    assert_eq!(counter.get(), 3);

    // Per-key mutation via `get().read_mut()`: only key 2 reprocesses.
    {
        let mut r = graph.with_state(&mut state);
        map.get(&mut r, 2).unwrap().read_mut().x = 10.0;
    }

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 4, "only the changed key reprocessed");
    let elements = map.read_state(&state);
    assert_eq!(elements[&2].y, 100.0);
    assert_eq!(elements[&1].y, 4.0, "untouched key keeps its output");
    assert_eq!(elements[&3].y, 16.0);
}

#[test]
fn map_entry_read_without_mutation_reprocesses_nothing() {
    let (mut graph, map, counter) = build();
    let mut state = MapState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 3);

    // Read-only `get()` does not mark anything dirty.
    {
        let mut r = graph.with_state(&mut state);
        let e = map.get(&mut r, 2).unwrap();
        assert_eq!(*e.read(), Element { x: 3.0, y: 9.0 });
    }

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 3, "read-only get reprocesses nothing");
}

#[test]
fn map_port_insert_only_reprocesses_new_key() {
    let (mut graph, map, counter) = build();
    let mut state = MapState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 3);

    // Insert a new key via the per-key API; only it reprocesses.
    {
        let mut r = graph.with_state(&mut state);
        map.insert(&mut r, 4, Element { x: 6.0, y: 0.0 });
    }

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 4, "only the inserted key reprocessed");
    let elements = map.read_state(&state);
    assert_eq!(elements[&4].y, 36.0);
    assert_eq!(elements.len(), 4);
}

#[test]
fn map_port_remove_only_drops_that_key() {
    let (mut graph, map, counter) = build();
    let mut state = MapState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 3);

    // Remove a key via the per-key API; no reprocessing, just removal.
    {
        let mut r = graph.with_state(&mut state);
        let removed = map.remove(&mut r, 2);
        assert_eq!(removed, Some(Element { x: 3.0, y: 9.0 }));
    }

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 3, "removal does not reprocess");
    let elements = map.read_state(&state);
    assert!(!elements.contains_key(&2));
    assert_eq!(elements.len(), 2);
}

#[test]
fn map_node_clean_tick_reprocesses_nothing() {
    let (mut graph, map, counter) = build();
    let mut state = MapState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 3);

    // No map changes; tick again.
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 3, "no reprocessing on clean tick");
    assert_eq!(map.read_state(&state).len(), 3);
}

// ── Chained map stages ────────────────────────────────────────────────────

/// A three-field element for the chained pipeline: `x` → (stage 1) → `y` →
/// (stage 2) → `z`.
#[derive(Clone, Default, PartialEq, Debug, DagStruct)]
pub struct ChainElement {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[state]
#[derive(Clone, Default, DagStruct)]
pub struct ChainState {
    pub elements: HashMap<u32, ChainElement>,
}

fn build_chain() -> (
    shame_gui::graph::Graph<ChainState>,
    shame_gui::graph::Port<HashMap<u32, ChainElement>, ChainState>,
    Rc<Cell<usize>>,
    Rc<Cell<usize>>,
) {
    let mut graph = shame_gui::graph::Graph::new();
    let stage1 = Rc::new(Cell::new(0usize));
    let stage2 = Rc::new(Cell::new(0usize));

    let p = ChainState::ports();
    let map = p.elements;

    // Seed the map.
    {
        let m = map;
        graph.add_node(
            move |gref: &mut DagStructRef<ChainState>, _gpu| {
                let mut cur = m.read(gref).clone();
                if cur.is_empty() {
                    cur.insert(
                        1,
                        ChainElement {
                            x: 2.0,
                            y: 0.0,
                            z: 0.0,
                        },
                    );
                    cur.insert(
                        2,
                        ChainElement {
                            x: 3.0,
                            y: 0.0,
                            z: 0.0,
                        },
                    );
                    cur.insert(
                        3,
                        ChainElement {
                            x: 4.0,
                            y: 0.0,
                            z: 0.0,
                        },
                    );
                    m.write(gref, cur);
                }
            },
            (),
            map,
            None,
        );
    }

    let el = ChainElement::ports();

    // Stage 1: x → y.
    {
        let c = stage1.clone();
        graph.add_map_node(
            map,
            (),
            (),
            el.x,
            el.y,
            move |_gref, _gpu, _key, e: &mut DagStructRef<ChainElement>| {
                c.set(c.get() + 1);
                let x = *el.x.read(e);
                el.y.write(e, x * x);
            },
        );
    }

    // Stage 2: y → z.
    {
        let c = stage2.clone();
        graph.add_map_node(
            map,
            (),
            (),
            el.y,
            el.z,
            move |_gref, _gpu, _key, e: &mut DagStructRef<ChainElement>| {
                c.set(c.get() + 1);
                let y = *el.y.read(e);
                el.z.write(e, y * 2.0);
            },
        );
    }

    (graph, map, stage1, stage2)
}

#[test]
fn chained_map_nodes_propagate_elem_port_dirty() {
    let (mut graph, map, stage1, stage2) = build_chain();
    let mut state = ChainState::default();

    graph.tick(&mut state, None); // seed + stage1 + stage2 over all keys
    assert_eq!(stage1.get(), 3);
    assert_eq!(stage2.get(), 3);
    assert_eq!(map.read_state(&state)[&2].z, 18.0, "z = 2 * 3^2");

    // Change key 2's `x`. Stage 1 (reads x) reprocesses key 2, writing `y`;
    // stage 2 (reads y) then reprocesses only key 2. Keys 1 and 3 untouched.
    {
        let mut r = graph.with_state(&mut state);
        map.get(&mut r, 2).unwrap().read_mut().x = 10.0;
    }

    graph.tick(&mut state, None);
    assert_eq!(stage1.get(), 4, "stage 1 reprocessed only key 2");
    assert_eq!(stage2.get(), 4, "stage 2 reprocessed only key 2");
    let elements = map.read_state(&state);
    assert_eq!(elements[&2].y, 100.0);
    assert_eq!(elements[&2].z, 200.0, "z = 2 * 10^2");
    assert_eq!(elements[&1].z, 8.0, "untouched key 1 keeps its output");
    assert_eq!(elements[&3].z, 32.0, "untouched key 3 keeps its output");
}

#[test]
fn chained_map_nodes_clean_tick_reprocesses_nothing() {
    let (mut graph, map, stage1, stage2) = build_chain();
    let mut state = ChainState::default();
    graph.tick(&mut state, None);
    assert_eq!(stage1.get(), 3);
    assert_eq!(stage2.get(), 3);

    graph.tick(&mut state, None);
    assert_eq!(stage1.get(), 3, "stage 1 clean tick");
    assert_eq!(stage2.get(), 3, "stage 2 clean tick");
    assert_eq!(map.read_state(&state).len(), 3);
}
