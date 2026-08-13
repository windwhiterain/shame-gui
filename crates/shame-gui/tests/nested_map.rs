//! Nested HashMap fan-out: a `HashMap<u32, Group>` whose elements each hold
//! their own nested `HashMap<u32, Leaf>`, fanned out by two flat nodes — the
//! nested node (`Graph::add_map_node_nested`) squares each leaf's `x` into
//! `y`, and the outer map node sums a group's leaves into `total`.
//!
//! Mirrors `dynamic_map.rs`: dirty is write-based and record-driven, so the
//! nested level reprocesses only the elements the dirty records name.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use shame_gui::DagStruct;
use shame_gui::graph::DagStructRef;
use shame_gui::state;

/// The innermost element: `x` is the input, `y` the computed output.
#[derive(Clone, Default, PartialEq, Debug, DagStruct)]
pub struct Leaf {
    pub x: f32,
    pub y: f32,
}

/// The outer element: a nested map of leaves + a computed total.
#[derive(Clone, Default, PartialEq, Debug, DagStruct)]
pub struct Group {
    pub children: HashMap<u32, Leaf>,
    pub total: f32,
}

/// App state: the outer map.
#[state]
#[derive(Clone, Default, DagStruct)]
pub struct NestedState {
    pub groups: HashMap<u32, Group>,
}

pub fn build() -> (
    shame_gui::graph::Graph<NestedState>,
    shame_gui::graph::Port<HashMap<u32, Group>, NestedState>,
    Rc<Cell<usize>>,
    Rc<Cell<usize>>,
) {
    let mut graph = shame_gui::graph::Graph::new();
    let nested_counter = Rc::new(Cell::new(0usize));
    let outer_counter = Rc::new(Cell::new(0usize));

    let p = NestedState::ports();
    let map = p.groups;
    let g = Group::ports();
    let l = Leaf::ports();

    // Upstream: seed two groups (1: three leaves, 2: two leaves) on first tick.
    {
        let m = map;
        graph.add_node(
            move |gref: &mut DagStructRef<NestedState>, _gpu| {
                let mut cur = m.read(gref).clone();
                if cur.is_empty() {
                    let mut g1 = Group::default();
                    for (i, x) in [2.0f32, 3.0, 4.0].iter().enumerate() {
                        g1.children.insert(i as u32, Leaf { x: *x, y: 0.0 });
                    }
                    let mut g2 = Group::default();
                    for (i, x) in [5.0f32, 6.0].iter().enumerate() {
                        g2.children.insert(i as u32, Leaf { x: *x, y: 0.0 });
                    }
                    cur.insert(1, g1);
                    cur.insert(2, g2);
                    m.write(gref, cur);
                }
            },
            (),
            map,
            None,
        );
    }

    // Nested fan-out: square x into y, per leaf. Registered BEFORE the outer
    // map node so its per-element marks reach the outer node's `elem_in` in
    // the same tick.
    {
        let c = nested_counter.clone();
        let m = map;
        graph.add_map_node_nested(
            m,
            g.children,
            l.x,
            l.y,
            move |_gref, _gpu, _leaf_key, leaf: &mut DagStructRef<Leaf>| {
                c.set(c.get() + 1);
                let x = *l.x.read(leaf);
                l.y.write(leaf, x * x);
            },
        );
    }

    // Outer fan-out: sum the leaves' y into each group's total.
    {
        let c = outer_counter.clone();
        let m = map;
        graph.add_map_node(
            m,
            (),
            (),
            g.children,
            g.total,
            move |_gref, _gpu, _group_key, group: &mut DagStructRef<Group>| {
                c.set(c.get() + 1);
                let sum: f32 = g.children.read(group).values().map(|c| c.y).sum();
                g.total.write(group, sum);
            },
        );
    }

    (graph, map, nested_counter, outer_counter)
}

#[test]
fn nested_first_tick_processes_all_leaves_and_totals() {
    let (mut graph, map, nested, outer) = build();
    let mut state = NestedState::default();

    graph.tick(&mut state, None);
    assert_eq!(nested.get(), 5, "all leaves processed once (3 + 2)");
    assert_eq!(outer.get(), 2, "both groups processed once");
    let groups = map.read_state(&state);
    assert_eq!(groups[&1].total, 29.0, "4 + 9 + 16");
    assert_eq!(groups[&2].total, 61.0, "25 + 36");
}

#[test]
fn whole_element_mutation_reprocesses_only_that_group() {
    let (mut graph, map, nested, outer) = build();
    let mut state = NestedState::default();
    graph.tick(&mut state, None); // seed + fan-outs
    assert_eq!(nested.get(), 5);
    assert_eq!(outer.get(), 2);

    // Mutate one leaf of group 1 through the outer key (whole-element).
    {
        let mut r = graph.with_state(&mut state);
        map.get(&mut r, 1)
            .unwrap()
            .read_mut()
            .children
            .get_mut(&2)
            .unwrap()
            .x = 10.0;
    }

    graph.tick(&mut state, None);
    assert_eq!(nested.get(), 8, "group 1's three leaves reprocessed");
    assert_eq!(outer.get(), 3, "only group 1's total recomputed");
    let groups = map.read_state(&state);
    assert_eq!(groups[&1].children[&2].y, 100.0);
    assert_eq!(groups[&1].total, 113.0, "4 + 9 + 100");
    assert_eq!(groups[&2].total, 61.0, "untouched group keeps its output");
    assert_eq!(
        groups[&2].children[&1].y, 36.0,
        "untouched leaves keep theirs"
    );
}

#[test]
fn whole_map_write_reprocesses_all_groups() {
    let (mut graph, map, nested, outer) = build();
    let mut state = NestedState::default();
    graph.tick(&mut state, None);
    assert_eq!(nested.get(), 5);
    assert_eq!(outer.get(), 2);

    {
        let mut r = graph.with_state(&mut state);
        map.read_mut(&mut r)
            .get_mut(&2)
            .unwrap()
            .children
            .get_mut(&0)
            .unwrap()
            .x = 9.0;
    }

    graph.tick(&mut state, None);
    assert_eq!(nested.get(), 10, "all five leaves reprocessed");
    assert_eq!(outer.get(), 4, "both groups recomputed");
    let groups = map.read_state(&state);
    assert_eq!(groups[&2].total, 117.0, "81 + 36");
    assert_eq!(groups[&1].total, 29.0);
}

#[test]
fn outer_insert_only_processes_new_group() {
    let (mut graph, map, nested, outer) = build();
    let mut state = NestedState::default();
    graph.tick(&mut state, None);
    assert_eq!(nested.get(), 5);
    assert_eq!(outer.get(), 2);

    {
        let mut r = graph.with_state(&mut state);
        let mut g3 = Group::default();
        for (i, x) in [7.0f32, 8.0].iter().enumerate() {
            g3.children.insert(i as u32, Leaf { x: *x, y: 0.0 });
        }
        map.insert(&mut r, 3, g3);
    }

    graph.tick(&mut state, None);
    assert_eq!(nested.get(), 7, "only the new group's two leaves");
    assert_eq!(outer.get(), 3, "only the new group");
    let groups = map.read_state(&state);
    assert_eq!(groups[&3].total, 113.0, "49 + 64");
    assert_eq!(groups.len(), 3);
}

#[test]
fn outer_remove_drops_group_without_reprocessing() {
    let (mut graph, map, nested, outer) = build();
    let mut state = NestedState::default();
    graph.tick(&mut state, None);
    assert_eq!(nested.get(), 5);
    assert_eq!(outer.get(), 2);

    {
        let mut r = graph.with_state(&mut state);
        map.remove(&mut r, 2);
    }

    graph.tick(&mut state, None);
    assert_eq!(nested.get(), 5, "removal does not reprocess");
    assert_eq!(outer.get(), 2);
    let groups = map.read_state(&state);
    assert!(!groups.contains_key(&2));
    assert_eq!(groups.len(), 1);
    assert!(groups.contains_key(&1), "untouched group remains");
}

#[test]
fn nested_clean_tick_reprocesses_nothing() {
    let (mut graph, map, nested, outer) = build();
    let mut state = NestedState::default();
    graph.tick(&mut state, None);
    assert_eq!(nested.get(), 5);
    assert_eq!(outer.get(), 2);

    graph.tick(&mut state, None);
    assert_eq!(nested.get(), 5, "no reprocessing on clean tick");
    assert_eq!(outer.get(), 2);
    assert_eq!(map.read_state(&state).len(), 2);
}
