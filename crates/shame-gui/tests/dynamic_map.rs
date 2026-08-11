//! Dynamic HashMap fan-out: `Graph::add_map_node` processes a
//! `HashMap<K, E>` element-by-element, reprocessing only changed keys.
//!
//! This exercises the core feature without a winit loop: a state struct
//! holding a `HashMap<u32, Element>`, plus an upstream node that mutates the
//! map, and a map node that squares each element's input into its output.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use shame_gui::DagStruct;
use shame_gui::graph::DagStructRef;
use shame_gui::state;

/// The per-element state: `x` is the input, `y` the computed output.
#[derive(Clone, Default, PartialEq, DagStruct)]
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
fn map_node_only_reprocesses_changed_keys() {
    let (mut graph, map, counter) = build();
    let mut state = MapState::default();
    graph.tick(&mut state, None); // seed + first fan-out
    assert_eq!(counter.get(), 3);

    // Mutate a single element's input; only that key should reprocess.
    {
        let mut r = graph.with_state(&mut state);
        map.read_mut(&mut r).get_mut(&2).unwrap().x = 10.0;
    }

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 4, "only the changed key reprocessed");
    let elements = map.read_state(&state);
    assert_eq!(elements[&2].y, 100.0);
    assert_eq!(elements[&1].y, 4.0, "untouched key keeps its output");
    assert_eq!(elements[&3].y, 16.0);
}

#[test]
fn map_node_removes_output_for_removed_key() {
    let (mut graph, map, counter) = build();
    let mut state = MapState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 3);

    {
        let mut r = graph.with_state(&mut state);
        map.read_mut(&mut r).remove(&2);
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
