//! Tests for the `HashMap<String, T>` map widget: interaction, layout, and
//! the dirty boundaries around add/remove/edit.
//!
//! The widget is driven directly through `Port::on_event`/`render` (with
//! `DagStructRef::new` for isolated tracking) and through a real `Graph`
//! (via `graph.with_state`) to assert exactly which elements reprocess.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use shame_gui::DagStruct;
use shame_gui::graph::{DagStructRef, Graph, Port};
use shame_gui::gui::primitives::map::MapWidgetData;
use shame_gui::gui::{
    EventResponse, Gui, InputEvent, Key, MouseButton, RenderContext, ViewportNode, ViewportTree,
    Widget, WidgetNode,
};
use shame_gui::math::{Vec2, Vec2u};
use shame_gui::rect::Rect;
use shame_gui::shader::RectEntry;
use shame_gui::text::TextObject;

// ── Test fixtures ───────────────────────────────────────────────────────

/// A map element with one field per primitive widget kind.
#[derive(Clone, Default, PartialEq, Debug, DagStruct, Widget)]
struct Person {
    name: String,
    age: u32,
    active: bool,
}

/// Standalone state holding the map (the widget only needs `DagStruct`).
#[derive(Clone, Default, DagStruct)]
struct MapState {
    people: HashMap<String, Person>,
}

fn seeded_state() -> MapState {
    MapState {
        people: HashMap::from([
            (
                "alice".to_string(),
                Person {
                    name: "Alice".to_string(),
                    age: 30,
                    active: true,
                },
            ),
            (
                "bob".to_string(),
                Person {
                    name: "Bob".to_string(),
                    age: 40,
                    active: false,
                },
            ),
        ]),
    }
}

const W: f32 = 300.0;
const H: f32 = 200.0;

fn rect() -> Rect {
    Rect::new(Vec2::new(0.0, 0.0), Vec2::new(W, H))
}

fn click(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseDown {
        pos: Vec2::new(x, y),
        button: MouseButton::Left,
        pressure: None,
    }
}

fn render_ctx<'a>(
    fills: &'a mut Vec<RectEntry>,
    outlines: &'a mut Vec<RectEntry>,
    texts: &'a mut Vec<TextObject>,
) -> RenderContext<'a> {
    RenderContext {
        fills,
        outlines,
        texts,
        framebuffer: Vec2u::new(W as u32, H as u32),
    }
}

fn text_values(texts: &[TextObject]) -> Vec<String> {
    texts.iter().map(|t| t.text.clone()).collect()
}

// Layout constants used by the tests (rect 300×200):
//   add row:            (0, 174) 300×26; input (0,174) 272×26; "+" (276,174) 24×26
//   alice header:       (0, 0) 300×22;  expander centre (9,11); delete centre (288,11)
//   bob header:         (0, 22) 300×22; expander centre (9,33); delete centre (288,33)
//   alice section:      (8, 22) 292×100 (3-row template: 3*24 + 2*8 + 2*6)
//     name editor:      (118, 28) 176×24
//   Person template height: 3*24 + 2*8 + 2*6 = 100

// ── Rendering / interaction ─────────────────────────────────────────────

#[test]
fn render_shows_keys_and_add_row() {
    let mut state = seeded_state();
    let p = MapState::ports();
    let data = MapWidgetData::<Person>::default();
    let mut r = DagStructRef::new(&mut state);
    let mut fills = vec![];
    let mut outlines = vec![];
    let mut texts = vec![];
    let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);

    p.people.render(&mut r, &data, rect(), &mut ctx);

    assert!(!fills.is_empty() && !outlines.is_empty());
    let values = text_values(&texts);
    assert!(values.iter().any(|t| t == "alice"), "{values:?}");
    assert!(values.iter().any(|t| t == "bob"), "{values:?}");
    assert!(values.iter().any(|t| t == "+"), "{values:?}");
    assert_eq!(
        values.iter().filter(|t| *t == "x").count(),
        2,
        "one delete per key"
    );
}

#[test]
fn click_expander_expands_and_render_shows_element_fields() {
    let mut state = seeded_state();
    let p = MapState::ports();
    let mut data = MapWidgetData::<Person>::default();
    let mut r = DagStructRef::new(&mut state);

    let response = p
        .people
        .on_event(&mut r, &mut data, &click(9.0, 11.0), rect());
    assert_eq!(response, EventResponse::Consumed);
    assert!(data.expanded.contains_key("alice"));

    let mut fills = vec![];
    let mut outlines = vec![];
    let mut texts = vec![];
    let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
    p.people.render(&mut r, &data, rect(), &mut ctx);
    let values = text_values(&texts);
    for label in ["name", "age", "active"] {
        assert!(
            values.iter().any(|t| t == label),
            "{label} missing: {values:?}"
        );
    }
    assert!(
        values.iter().any(|t| t == "Alice"),
        "element values rendered: {values:?}"
    );

    // Clicking the header again collapses.
    p.people
        .on_event(&mut r, &mut data, &click(9.0, 11.0), rect());
    assert!(!data.expanded.contains_key("alice"));
    assert!(data.sub_focus.is_none());
}

#[test]
fn click_delete_removes_key() {
    let mut state = seeded_state();
    let p = MapState::ports();
    let mut data = MapWidgetData::<Person>::default();
    let mut r = DagStructRef::new(&mut state);

    let response = p
        .people
        .on_event(&mut r, &mut data, &click(288.0, 11.0), rect());
    assert_eq!(response, EventResponse::Consumed);
    assert!(!state.people.contains_key("alice"), "alice removed");
    assert!(state.people.contains_key("bob"), "other key untouched");
}

#[test]
fn add_row_typing_and_enter_inserts_default() {
    let mut state = seeded_state();
    let p = MapState::ports();
    let mut data = MapWidgetData::<Person>::default();
    let mut r = DagStructRef::new(&mut state);

    let response = p
        .people
        .on_event(&mut r, &mut data, &click(50.0, 187.0), rect());
    assert_eq!(response, EventResponse::Consumed);
    assert!(data.adding);

    for ch in ['c', 'a', 'r', 'o', 'l'] {
        p.people
            .on_event(&mut r, &mut data, &InputEvent::Char { ch }, rect());
    }
    assert_eq!(data.new_key, "carol");

    p.people.on_event(
        &mut r,
        &mut data,
        &InputEvent::KeyDown { key: Key::Enter },
        rect(),
    );
    assert!(state.people.contains_key("carol"));
    assert_eq!(state.people["carol"], Person::default());
    assert!(data.new_key.is_empty(), "buffer cleared after commit");
}

#[test]
fn add_duplicate_key_is_noop() {
    let mut state = seeded_state();
    let p = MapState::ports();
    let mut data = MapWidgetData::<Person>::default();
    let mut r = DagStructRef::new(&mut state);

    p.people
        .on_event(&mut r, &mut data, &click(50.0, 187.0), rect());
    for ch in ['a', 'l', 'i', 'c', 'e'] {
        p.people
            .on_event(&mut r, &mut data, &InputEvent::Char { ch }, rect());
    }
    p.people.on_event(
        &mut r,
        &mut data,
        &InputEvent::KeyDown { key: Key::Enter },
        rect(),
    );
    assert_eq!(
        state.people["alice"].age, 30,
        "existing element not overwritten"
    );
    assert_eq!(state.people.len(), 2);
}

#[test]
fn layout_style_tracks_keys_and_expansion() {
    let mut state = seeded_state();
    let p = MapState::ports();
    let mut data = MapWidgetData::<Person>::default();
    let mut r = DagStructRef::new(&mut state);

    let add_h = 26.0;
    let header_h = 22.0;
    let template_h = 3.0 * 24.0 + 2.0 * 8.0 + 2.0 * 6.0;

    let style = p.people.layout_style(&r, &data);
    assert_eq!(style.min_size.height.value(), add_h + 2.0 * header_h);

    p.people
        .on_event(&mut r, &mut data, &click(9.0, 11.0), rect());
    let style = p.people.layout_style(&r, &data);
    assert_eq!(
        style.min_size.height.value(),
        add_h + 2.0 * header_h + template_h
    );

    p.people
        .on_event(&mut r, &mut data, &click(288.0, 11.0), rect()); // delete alice
    let style = p.people.layout_style(&r, &data);
    assert_eq!(style.min_size.height.value(), add_h + header_h);
}

// ── Dirty boundaries (the core of the widget contract) ──────────────────

/// A graph over `people`: a per-key map node with a counter, plus a plain
/// downstream node counting map-port reads.
fn build() -> (
    Graph<MapState>,
    Port<HashMap<String, Person>, MapState>,
    Rc<Cell<usize>>,
    Rc<Cell<usize>>,
) {
    let mut graph = Graph::new();
    let counter = Rc::new(Cell::new(0usize));
    let downstream = Rc::new(Cell::new(0usize));

    let p = MapState::ports();
    let map = p.people;

    // Seed the map once on the first tick.
    graph.add_node(
        {
            let m = map;
            move |gref: &mut DagStructRef<MapState>, _gpu| {
                let mut cur = m.read(gref).clone();
                if cur.is_empty() {
                    cur.insert(
                        "alice".to_string(),
                        Person {
                            name: "Alice".to_string(),
                            age: 30,
                            active: true,
                        },
                    );
                    cur.insert(
                        "bob".to_string(),
                        Person {
                            name: "Bob".to_string(),
                            age: 40,
                            active: false,
                        },
                    );
                    m.write(gref, cur);
                }
            }
        },
        (),
        map,
        None,
    );

    // Per-key fan-out with a counter.
    {
        let c = counter.clone();
        let el = Person::ports();
        graph.add_map_node(
            map,
            (),
            (),
            el.name,
            (),
            move |_gref, _gpu, _key, e: &mut DagStructRef<Person>| {
                c.set(c.get() + 1);
                let _ = el.name.read(e);
            },
        );
    }

    // Plain downstream reader: re-runs whenever the map port is dirty.
    {
        let d = downstream.clone();
        graph.add_node(
            move |gref: &mut DagStructRef<MapState>, _gpu| {
                d.set(d.get() + 1);
                let _ = map.read(gref).len();
            },
            map,
            (),
            None,
        );
    }

    (graph, map, counter, downstream)
}

#[test]
fn render_and_layout_do_not_dirty_the_map() {
    let (mut graph, map, counter, downstream) = build();
    let mut state = MapState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 2);

    let mut data = MapWidgetData::<Person>::default();
    {
        let mut r = graph.with_state(&mut state);
        // Expand alice (data-only toggle).
        map.on_event(&mut r, &mut data, &click(9.0, 11.0), rect());
        // Render with the expanded element (read-only sub-walks).
        let mut fills = vec![];
        let mut outlines = vec![];
        let mut texts = vec![];
        let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
        map.render(&mut r, &data, rect(), &mut ctx);
        // Layout hint.
        let _ = map.layout_style(&r, &data);
    }
    graph.tick(&mut state, None);

    assert_eq!(
        counter.get(),
        2,
        "render/layout must not wake the map nodes (naive dagref marks the map port every frame)"
    );
    assert_eq!(
        downstream.get(),
        1,
        "downstream readers must not re-run either"
    );
}

#[test]
fn delete_does_not_reprocess_but_downstream_reruns() {
    let (mut graph, map, counter, downstream) = build();
    let mut state = MapState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 2);

    let mut data = MapWidgetData::<Person>::default();
    {
        let mut r = graph.with_state(&mut state);
        map.on_event(&mut r, &mut data, &click(288.0, 11.0), rect()); // delete alice
    }
    graph.tick(&mut state, None);

    assert!(!state.people.contains_key("alice"));
    assert_eq!(counter.get(), 2, "the removed element is never reprocessed");
    assert_eq!(downstream.get(), 2, "non-map readers re-run on remove");
}

#[test]
fn add_reprocesses_only_the_new_key() {
    let (mut graph, map, counter, downstream) = build();
    let mut state = MapState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 2);

    let mut data = MapWidgetData::<Person>::default();
    {
        let mut r = graph.with_state(&mut state);
        map.on_event(&mut r, &mut data, &click(50.0, 187.0), rect());
        for ch in ['c', 'a', 'r', 'o', 'l'] {
            map.on_event(&mut r, &mut data, &InputEvent::Char { ch }, rect());
        }
        map.on_event(
            &mut r,
            &mut data,
            &InputEvent::KeyDown { key: Key::Enter },
            rect(),
        );
    }
    graph.tick(&mut state, None);

    assert!(state.people.contains_key("carol"));
    assert_eq!(state.people["carol"], Person::default());
    assert_eq!(counter.get(), 3, "only the new key reprocessed");
    assert_eq!(downstream.get(), 2);
}

#[test]
fn child_edit_reprocesses_only_that_element() {
    let (mut graph, map, counter, _downstream) = build();
    let mut state = MapState::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 2);

    let mut data = MapWidgetData::<Person>::default();
    {
        let mut r = graph.with_state(&mut state);
        map.on_event(&mut r, &mut data, &click(9.0, 11.0), rect()); // expand alice
        map.on_event(&mut r, &mut data, &click(200.0, 35.0), rect()); // alice's name field
        map.on_event(&mut r, &mut data, &InputEvent::Char { ch: 'X' }, rect());
        map.on_event(
            &mut r,
            &mut data,
            &InputEvent::KeyDown { key: Key::Enter },
            rect(),
        );
    }
    graph.tick(&mut state, None);

    assert_eq!(state.people["alice"].name, "AliceX");
    assert_eq!(state.people["bob"].name, "Bob", "sibling element untouched");
    assert_eq!(counter.get(), 3, "only alice reprocessed");
}

#[test]
fn expanded_elements_have_isolated_widget_data() {
    let mut state = seeded_state();
    let p = MapState::ports();
    let mut data = MapWidgetData::<Person>::default();
    let mut r = DagStructRef::new(&mut state);

    p.people
        .on_event(&mut r, &mut data, &click(9.0, 33.0), rect()); // expand bob first
    p.people
        .on_event(&mut r, &mut data, &click(9.0, 11.0), rect()); // then alice
    p.people
        .on_event(&mut r, &mut data, &click(200.0, 35.0), rect()); // alice's name field
    p.people
        .on_event(&mut r, &mut data, &InputEvent::Char { ch: 'X' }, rect());

    let mut fills = vec![];
    let mut outlines = vec![];
    let mut texts = vec![];
    let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
    p.people.render(&mut r, &data, rect(), &mut ctx);
    let values = text_values(&texts);
    assert!(
        values.iter().any(|t| t == "AliceX|"),
        "alice's field shows its draft: {values:?}"
    );
    assert!(
        values.iter().any(|t| t == "Bob"),
        "bob's field shows its own state value: {values:?}"
    );
    assert!(
        !values.iter().any(|t| t == "BobX|"),
        "no cross-talk into bob's buffer"
    );
}

// ── GUI integration ─────────────────────────────────────────────────────

#[test]
fn gui_routes_keyboard_to_expanded_child() {
    let mut state = seeded_state();
    let p = MapState::ports();
    let node = WidgetNode::new_default(p.people);
    let map_node_id = node.id();
    let mut gui = Gui::new(ViewportTree::new(ViewportNode::widget(node)));
    let fb = Vec2u::new(W as u32, H as u32);
    let mut r = DagStructRef::new(&mut state);

    gui.on_event(&click(9.0, 11.0), fb, &mut r); // expand alice
    gui.on_event(&click(200.0, 35.0), fb, &mut r); // alice's name field
    assert_eq!(gui.focus, Some(map_node_id), "the map widget claims focus");
    gui.on_event(&InputEvent::Char { ch: 'X' }, fb, &mut r);
    gui.on_event(&InputEvent::KeyDown { key: Key::Enter }, fb, &mut r);

    assert_eq!(state.people["alice"].name, "AliceX");
    assert_eq!(state.people["bob"].name, "Bob");
}

// ── Nested maps inside elements ─────────────────────────────────────────

/// An element with a nested map — the map widget recurses, and add/remove
/// at the nested level must dirty exactly that element's subtree.
#[derive(Clone, Default, PartialEq, Debug, DagStruct, Widget)]
struct Item {
    v: f32,
}

#[derive(Clone, Default, PartialEq, Debug, DagStruct, Widget)]
struct Layer {
    items: HashMap<String, Item>,
}

#[derive(Clone, Default, DagStruct)]
struct LayerState {
    layers: HashMap<String, Layer>,
}

#[test]
fn nested_map_add_and_remove_reprocess_only_that_layer() {
    let mut graph = Graph::new();
    let counter = Rc::new(Cell::new(0usize));

    let p = LayerState::ports();
    let map = p.layers;
    let l = Layer::ports();
    graph.add_map_node(map, (), (), l.items, (), {
        let c = counter.clone();
        move |_gref, _gpu, _key, e: &mut DagStructRef<Layer>| {
            c.set(c.get() + 1);
            let _ = l.items.read(e).len();
        }
    });

    let mut state = LayerState {
        layers: HashMap::from([
            (
                "bg".to_string(),
                Layer {
                    items: HashMap::new(),
                },
            ),
            (
                "fg".to_string(),
                Layer {
                    items: HashMap::new(),
                },
            ),
        ]),
    };
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 2);

    let mut data = MapWidgetData::<Layer>::default();
    {
        let mut r = graph.with_state(&mut state);
        // Expand "bg" (sorted first), render once so section heights are
        // measured, then click the nested map's add-row input.
        map.on_event(&mut r, &mut data, &click(9.0, 11.0), rect());
        let mut fills = vec![];
        let mut outlines = vec![];
        let mut texts = vec![];
        let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
        map.render(&mut r, &data, rect(), &mut ctx);
        map.on_event(&mut r, &mut data, &click(150.0, 41.0), rect());
        for ch in ['k', '1'] {
            map.on_event(&mut r, &mut data, &InputEvent::Char { ch }, rect());
        }
        map.on_event(
            &mut r,
            &mut data,
            &InputEvent::KeyDown { key: Key::Enter },
            rect(),
        );
    }
    graph.tick(&mut state, None);

    assert!(state.layers["bg"].items.contains_key("k1"));
    assert!(state.layers["fg"].items.is_empty());
    assert_eq!(counter.get(), 3, "only the bg layer reprocessed");

    // Remove the item through the nested delete button (refresh heights
    // first: the nested widget now has one header row).
    {
        let mut r = graph.with_state(&mut state);
        let mut fills = vec![];
        let mut outlines = vec![];
        let mut texts = vec![];
        let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
        map.render(&mut r, &data, rect(), &mut ctx);
        map.on_event(&mut r, &mut data, &click(282.0, 39.0), rect());
    }
    graph.tick(&mut state, None);

    assert!(state.layers["bg"].items.is_empty());
    assert!(state.layers["fg"].items.is_empty());
    assert_eq!(counter.get(), 4, "remove re-derives the bg layer only");
}
