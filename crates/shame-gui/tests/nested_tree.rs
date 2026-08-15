//! Arbitrary-depth render-tree fan-out: `Graph::add_map_tree_node` descends a
//! `HashMap` hierarchy of any depth from one flat node, reprocessing only the
//! subtrees the nested dirty records name. Mirrors `nested_map.rs`, but the
//! levels are driven by a single node over a recursive [`MapPath`].

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use shame_gui::DagStruct;
use shame_gui::graph::DagStructRef;
use shame_gui::graph::Graph;
use shame_gui::graph::LeafMarker;
use shame_gui::graph::MapPath;
use shame_gui::graph::Port;
use shame_gui::state;

/// The bottom element: `x` is the input, `y` the computed output.
#[derive(Clone, Default, PartialEq, Debug, DagStruct)]
pub struct Leaf {
    pub x: f32,
    pub y: f32,
}

/// Middle level: each element owns its own leaf map.
#[derive(Clone, Default, PartialEq, Debug, DagStruct)]
pub struct Mid {
    pub leaves: HashMap<u32, Leaf>,
    pub sum: f32,
}

/// Top level: each element owns a map of mids.
#[derive(Clone, Default, PartialEq, Debug, DagStruct)]
pub struct Group {
    pub children: HashMap<u32, Mid>,
    pub total: f32,
}

/// App state: the top map.
#[state]
#[derive(Clone, Default, DagStruct)]
pub struct TreeState {
    pub groups: HashMap<u32, Group>,
}

/// Seeds two groups on the first tick (group 1: 3+2 leaves across two mids,
/// group 2: 2 leaves in one mid — 7 leaves total).
fn seed(g: &mut Graph<TreeState>, map: Port<HashMap<u32, Group>, TreeState>) {
    let m = map;
    g.add_node(
        move |gref: &mut DagStructRef<TreeState>, _gpu| {
            let mut cur = m.read(gref).clone();
            if cur.is_empty() {
                let mut g1 = Group::default();
                let mut mid1 = Mid::default();
                for (i, x) in [2.0f32, 3.0, 4.0].iter().enumerate() {
                    mid1.leaves.insert(i as u32, Leaf { x: *x, y: 0.0 });
                }
                let mut mid2 = Mid::default();
                for (i, x) in [5.0f32, 6.0].iter().enumerate() {
                    mid2.leaves.insert(i as u32, Leaf { x: *x, y: 0.0 });
                }
                g1.children.insert(1, mid1);
                g1.children.insert(2, mid2);
                let mut g2 = Group::default();
                let mut mid3 = Mid::default();
                for (i, x) in [7.0f32, 8.0].iter().enumerate() {
                    mid3.leaves.insert(i as u32, Leaf { x: *x, y: 0.0 });
                }
                g2.children.insert(3, mid3);
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

/// Builds the graph: seed node + a 3-level render tree
/// (`groups → children → leaves`) that squares `x` into `y` per leaf.
/// Returns the graph, the top map port, and the leaf-eval counter.
pub fn build() -> (
    Graph<TreeState>,
    Port<HashMap<u32, Group>, TreeState>,
    Rc<Cell<usize>>,
) {
    let mut graph = Graph::new();
    let counter = Rc::new(Cell::new(0usize));
    let p = TreeState::ports();
    let map = p.groups;

    seed(&mut graph, map);

    {
        let c = counter.clone();
        let g = Group::ports();
        let m = Mid::ports();
        let l = Leaf::ports();
        graph.add_map_tree_node(
            map,
            MapPath {
                map: g.children,
                next: MapPath {
                    map: m.leaves,
                    next: LeafMarker::new(),
                },
            },
            g.total.id(),
            move |eref: &mut DagStructRef<Leaf>, _gpu| {
                c.set(c.get() + 1);
                let x = *l.x.read(eref);
                l.y.write(eref, x * x);
            },
        );
    }

    (graph, map, counter)
}

#[test]
fn first_tick_processes_all_leaves() {
    let (mut graph, map, counter) = build();
    let mut state = TreeState::default();

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 7, "all leaves processed once (3 + 2 + 2)");
    let groups = map.read_state(&state);
    assert_eq!(groups[&1].children[&1].leaves[&1].y, 9.0);
    assert_eq!(groups[&1].children[&2].leaves[&1].y, 36.0);
    assert_eq!(groups[&2].children[&3].leaves[&0].y, 49.0);
}

#[test]
fn whole_element_mutation_reprocesses_only_that_subtree() {
    let (mut graph, map, counter) = build();
    let mut state = TreeState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 7);

    // Mutate one leaf deep inside group 1 through the top map (whole-element).
    {
        let mut r = graph.with_state(&mut state);
        map.get(&mut r, 1)
            .unwrap()
            .read_mut()
            .children
            .get_mut(&1)
            .unwrap()
            .leaves
            .get_mut(&2)
            .unwrap()
            .x = 10.0;
    }

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 12, "group 1's five leaves reprocessed");
    let groups = map.read_state(&state);
    assert_eq!(groups[&1].children[&1].leaves[&2].y, 100.0);
    assert_eq!(
        groups[&2].children[&3].leaves[&0].y, 49.0,
        "untouched group"
    );
}

#[test]
fn whole_map_write_reprocesses_all_groups() {
    let (mut graph, map, counter) = build();
    let mut state = TreeState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 7);

    {
        let mut r = graph.with_state(&mut state);
        map.read_mut(&mut r)
            .get_mut(&2)
            .unwrap()
            .children
            .get_mut(&3)
            .unwrap()
            .leaves
            .get_mut(&0)
            .unwrap()
            .x = 12.0;
    }

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 14, "all seven leaves reprocessed");
    let groups = map.read_state(&state);
    assert_eq!(groups[&2].children[&3].leaves[&0].y, 144.0);
    assert_eq!(groups[&1].children[&1].leaves[&0].y, 4.0);
}

#[test]
fn outer_insert_only_processes_new_subtree() {
    let (mut graph, map, counter) = build();
    let mut state = TreeState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 7);

    {
        let mut r = graph.with_state(&mut state);
        let mut g3 = Group::default();
        let mut mid4 = Mid::default();
        for (i, x) in [9.0f32, 10.0].iter().enumerate() {
            mid4.leaves.insert(i as u32, Leaf { x: *x, y: 0.0 });
        }
        g3.children.insert(4, mid4);
        map.insert(&mut r, 3, g3);
    }

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 9, "only the new group's two leaves");
    let groups = map.read_state(&state);
    assert_eq!(groups[&3].children[&4].leaves[&1].y, 100.0);
    assert_eq!(groups.len(), 3);
    assert_eq!(
        groups[&1].children[&1].leaves[&0].y, 4.0,
        "untouched groups"
    );
}

#[test]
fn outer_remove_drops_group_without_reprocessing() {
    let (mut graph, map, counter) = build();
    let mut state = TreeState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 7);

    {
        let mut r = graph.with_state(&mut state);
        map.remove(&mut r, 2);
    }

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 7, "removal does not reprocess");
    let groups = map.read_state(&state);
    assert!(!groups.contains_key(&2));
    assert_eq!(groups.len(), 1);
    assert!(groups.contains_key(&1), "untouched group remains");
}

#[test]
fn clean_tick_reprocesses_nothing() {
    let (mut graph, map, counter) = build();
    let mut state = TreeState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 7);

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 7, "no reprocessing on clean tick");
    assert_eq!(map.read_state(&state).len(), 2);
}

#[test]
fn chained_stage_writes_propagate_to_the_tree() {
    // A chained map node (registered before the tree) bumps each group's
    // `total`; the tree's trigger port is `total`, so a group the chained
    // node reprocessed is reprocessed by the tree in the same tick.
    let mut graph = Graph::new();
    let counter = Rc::new(Cell::new(0usize));
    let p = TreeState::ports();
    let map = p.groups;

    seed(&mut graph, map);

    {
        let g = Group::ports();
        graph.add_map_node(map, (), (), (), g.total, move |_gref, _gpu, _k, eref| {
            let t = *g.total.read(eref) + 1.0;
            g.total.write(eref, t);
        });
    }

    {
        let c = counter.clone();
        let g = Group::ports();
        let m = Mid::ports();
        let l = Leaf::ports();
        graph.add_map_tree_node(
            map,
            MapPath {
                map: g.children,
                next: MapPath {
                    map: m.leaves,
                    next: LeafMarker::new(),
                },
            },
            g.total.id(),
            move |eref: &mut DagStructRef<Leaf>, _gpu| {
                c.set(c.get() + 1);
                let x = *l.x.read(eref);
                l.y.write(eref, x * x);
            },
        );
    }

    let mut state = TreeState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 7, "first tick: full pass");
    let groups = map.read_state(&state);
    assert_eq!(groups[&1].total, 1.0, "chained stage bumped the totals");

    // Mutate one group; the chained stage re-runs it, writes `total`, and the
    // tree reprocesses the group's subtree in the same tick.
    {
        let mut r = graph.with_state(&mut state);
        map.get(&mut r, 1)
            .unwrap()
            .read_mut()
            .children
            .get_mut(&1)
            .unwrap()
            .leaves
            .get_mut(&2)
            .unwrap()
            .x = 10.0;
    }
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 12, "group 1's five leaves reprocessed");
    let groups = map.read_state(&state);
    assert_eq!(groups[&1].total, 2.0, "chained stage bumped it again");
    assert_eq!(groups[&1].children[&1].leaves[&2].y, 100.0);
    assert_eq!(groups[&2].children[&3].leaves[&0].y, 49.0);
}

#[test]
fn widget_side_per_cell_write_reprocesses_only_that_leaf() {
    // Widget-side access (no map-node eval): each `MapEntry::dagref` shares
    // the element's tracking with the graph, so the innermost `read_mut`
    // records only that leaf — the tree reprocesses a single cell, not the
    // whole layer subtree.
    let (mut graph, map, counter) = build();
    let mut state = TreeState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 7);

    {
        let mut r = graph.with_state(&mut state);
        let g = Group::ports();
        let m = Mid::ports();
        let mut g1 = map.get(&mut r, 1).unwrap();
        let mut eref = g1.dagref();
        let mut m1 = g.children.get(&mut eref, 1).unwrap();
        let mut eref2 = m1.dagref();
        m.leaves.get(&mut eref2, 2).unwrap().read_mut().x = 10.0;
    }

    graph.tick(&mut state, None);
    assert_eq!(
        counter.get(),
        8,
        "only leaf 2 of mid 1 of group 1 reprocessed"
    );
    let groups = map.read_state(&state);
    assert_eq!(groups[&1].children[&1].leaves[&2].y, 100.0);
    assert_eq!(
        groups[&1].children[&1].leaves[&0].y, 4.0,
        "sibling leaf in the same mid untouched"
    );
    assert_eq!(
        groups[&2].children[&3].leaves[&0].y, 49.0,
        "other group untouched"
    );
}

#[test]
fn widget_side_leaf_port_write_is_per_cell() {
    // The tree's trigger is the leaf read port (x): a widget-side write of
    // that port through a tracked element ref reprocesses only that leaf.
    let mut graph = Graph::new();
    let counter = Rc::new(Cell::new(0usize));
    let p = TreeState::ports();
    let map = p.groups;
    seed(&mut graph, map);
    {
        let c = counter.clone();
        let g = Group::ports();
        let m = Mid::ports();
        let l = Leaf::ports();
        graph.add_map_tree_node(
            map,
            MapPath {
                map: g.children,
                next: MapPath {
                    map: m.leaves,
                    next: LeafMarker::new(),
                },
            },
            l.x.id(),
            move |eref: &mut DagStructRef<Leaf>, _gpu| {
                c.set(c.get() + 1);
                let x = *l.x.read(eref);
                l.y.write(eref, x * x);
            },
        );
    }

    let mut state = TreeState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 7);

    {
        let mut r = graph.with_state(&mut state);
        let g = Group::ports();
        let m = Mid::ports();
        let l = Leaf::ports();
        let mut g1 = map.get(&mut r, 1).unwrap();
        let mut eref = g1.dagref();
        let mut m1 = g.children.get(&mut eref, 1).unwrap();
        let mut eref2 = m1.dagref();
        let mut l1 = m.leaves.get(&mut eref2, 1).unwrap();
        let mut eref3 = l1.dagref();
        l.x.write(&mut eref3, 10.0);
    }

    graph.tick(&mut state, None);
    assert_eq!(
        counter.get(),
        8,
        "only leaf 1 of mid 1 of group 1 reprocessed"
    );
    let groups = map.read_state(&state);
    assert_eq!(groups[&1].children[&1].leaves[&1].y, 100.0);
    assert_eq!(
        groups[&1].children[&1].leaves[&2].y, 16.0,
        "sibling leaf in the same mid untouched"
    );
    assert_eq!(groups[&2].children[&3].leaves[&0].y, 49.0);
}

#[test]
fn widget_side_mid_write_reprocesses_that_mid_only() {
    // Writing a mid element (not a leaf) through the tracked refs marks the
    // mid `full` — its subtree reprocesses, but no other mid and no other
    // group.
    let (mut graph, map, counter) = build();
    let mut state = TreeState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 7);

    {
        let mut r = graph.with_state(&mut state);
        let g = Group::ports();
        let mut g1 = map.get(&mut r, 1).unwrap();
        let mut eref = g1.dagref();
        g.children
            .get(&mut eref, 1)
            .unwrap()
            .read_mut()
            .leaves
            .get_mut(&0)
            .unwrap()
            .x = 5.0;
    }

    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 10, "mid 1's three leaves reprocessed");
    let groups = map.read_state(&state);
    assert_eq!(groups[&1].children[&1].leaves[&0].y, 25.0);
    assert_eq!(
        groups[&1].children[&2].leaves[&1].y, 36.0,
        "sibling mid untouched"
    );
    assert_eq!(
        groups[&2].children[&3].leaves[&0].y, 49.0,
        "other group untouched"
    );
}

/// A 2-level render tree with the shape of the batched registration:
/// top elements carry a constant, the map below holds the leaves.
#[derive(Clone, Default, PartialEq, Debug, DagStruct)]
pub struct Pair {
    pub constant: f32,
    pub leaves: HashMap<u32, Leaf>,
}

#[state]
#[derive(Clone, Default, DagStruct)]
pub struct PairState {
    pub pairs: HashMap<u32, Pair>,
}

#[test]
fn depth_two_path_squares_immediately() {
    let mut graph = Graph::new();
    let counter = Rc::new(Cell::new(0usize));
    let p = PairState::ports();
    let map = p.pairs;

    {
        let m = map;
        graph.add_node(
            move |gref: &mut DagStructRef<PairState>, _gpu| {
                let mut cur = m.read(gref).clone();
                if cur.is_empty() {
                    let mut p1 = Pair::default();
                    p1.constant = 5.0;
                    p1.leaves.insert(0, Leaf { x: 2.0, y: 0.0 });
                    p1.leaves.insert(1, Leaf { x: 3.0, y: 0.0 });
                    let mut p2 = Pair::default();
                    p2.constant = 7.0;
                    p2.leaves.insert(0, Leaf { x: 4.0, y: 0.0 });
                    cur.insert(1, p1);
                    cur.insert(2, p2);
                    m.write(gref, cur);
                }
            },
            (),
            map,
            None,
        );
    }

    {
        let c = counter.clone();
        let pair = Pair::ports();
        let l = Leaf::ports();
        graph.add_map_tree_node(
            map,
            MapPath {
                map: pair.leaves,
                next: LeafMarker::new(),
            },
            pair.constant.id(),
            move |eref: &mut DagStructRef<Leaf>, _gpu| {
                c.set(c.get() + 1);
                let x = *l.x.read(eref);
                l.y.write(eref, x * x);
            },
        );
    }

    let mut state = PairState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 3, "three leaves across two pairs");
    let pairs = map.read_state(&state);
    assert_eq!(pairs[&1].constant, 5.0, "constant untouched");
    assert_eq!(pairs[&1].leaves[&1].y, 9.0);
    assert_eq!(pairs[&2].leaves[&0].y, 16.0);
}

#[test]
fn depth_one_leaf_at_top_path() {
    // A leaf-at-top path: the top map's elements are the leaves, so the tree
    // node behaves like the flat map registration (the trigger port is the
    // leaf read port).
    let mut graph = Graph::new();
    let counter = Rc::new(Cell::new(0usize));
    let p = PairState::ports();
    let map = p.pairs;

    {
        let m = map;
        graph.add_node(
            move |gref: &mut DagStructRef<PairState>, _gpu| {
                let mut cur = m.read(gref).clone();
                if cur.is_empty() {
                    cur.insert(
                        1,
                        Pair {
                            constant: 1.0,
                            leaves: HashMap::new(),
                        },
                    );
                    cur.insert(
                        2,
                        Pair {
                            constant: 2.0,
                            leaves: HashMap::new(),
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

    // A chained stage (before the tree) writes `constant` per element; the
    // tree's trigger is the constant port, so the tree reprocesses it.
    {
        let pair = Pair::ports();
        graph.add_map_node(
            map,
            (),
            (),
            (),
            pair.constant,
            move |_gref, _gpu, _k, eref| {
                let c = *pair.constant.read(eref) + 1.0;
                pair.constant.write(eref, c);
            },
        );
    }

    {
        let c = counter.clone();
        let pair = Pair::ports();
        graph.add_map_tree_node(
            map,
            LeafMarker::new(),
            pair.constant.id(),
            move |eref: &mut DagStructRef<Pair>, _gpu| {
                c.set(c.get() + 1);
                let v = *pair.constant.read(eref);
                pair.constant.write(eref, v * 2.0);
            },
        );
    }

    let mut state = PairState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 2, "both elements processed once");
    let pairs = map.read_state(&state);
    assert_eq!(pairs[&1].constant, 4.0, "chained bump then doubled");
    assert_eq!(pairs[&2].constant, 6.0);
}
