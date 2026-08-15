//! Tests for the `MapSelector` key-selector widget: interaction, layout,
//! and the dirty boundaries around the map constraint.
//!
//! The widget is driven directly through `MapSelector`'s `Widget` methods
//! (with `DagStructRef::new` for isolated tracking) and through a real
//! `Graph` (via `graph.with_state`) to assert exactly which ports wake.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use shame_gui::DagStruct;
use shame_gui::graph::{DagStructRef, Graph, Port};
use shame_gui::gui::primitives::map::MapWidgetData;
use shame_gui::gui::primitives::selector::{MapSelector, SelectorData};
use shame_gui::gui::{
    EventResponse, Gui, InputEvent, MouseButton, RenderContext, ViewportNode, ViewportTree, Widget,
    WidgetNode,
};
use shame_gui::math::{Vec2, Vec2u};
use shame_gui::rect::Rect;
use shame_gui::shader::RectEntry;
use shame_gui::text::TextObject;

// ── Test fixtures ───────────────────────────────────────────────────────

/// A map value — the selector only needs its keys, so the element type is
/// just `Clone` for the manual tests; the derive tests need the full
/// `Widget` derive for the map row of the generated tree.
#[derive(Clone, Default, Debug, DagStruct, Widget)]
struct Team {
    desc: String,
}

/// State holding the constraint map and the selected key.
#[derive(Clone, Default, DagStruct)]
struct SelState {
    teams: HashMap<String, Team>,
    selected: String,
}

fn seeded_state() -> SelState {
    SelState {
        teams: HashMap::from([
            (
                "core".to_string(),
                Team {
                    desc: "engine".to_string(),
                },
            ),
            (
                "ui".to_string(),
                Team {
                    desc: "gui".to_string(),
                },
            ),
        ]),
        selected: "core".to_string(),
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

fn mouse_move(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseMove {
        pos: Vec2::new(x, y),
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
//   header:      (0, 0) 300×22   (the click-to-open bar)
//   option rows: "core" (0, 22) 300×24; "ui" (0, 46) 300×24
//   open height: 22 + 2*24 = 70

// ── Rendering / interaction ─────────────────────────────────────────────

#[test]
fn render_shows_value_and_placeholder() {
    let mut state = seeded_state();
    let p = SelState::ports();
    let data = SelectorData::default();
    let mut r = DagStructRef::new(&mut state);
    let mut fills = vec![];
    let mut outlines = vec![];
    let mut texts = vec![];
    let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);

    MapSelector::new(p.teams, p.selected).render(&mut r, &data, rect(), &mut ctx);

    assert!(!fills.is_empty() && !outlines.is_empty());
    let values = text_values(&texts);
    assert!(values.iter().any(|t| t == "core"), "{values:?}");
    assert!(!values.iter().any(|t| t == "(none)"), "{values:?}");
    drop(r);

    // Empty value renders the placeholder.
    state.selected = String::new();
    let mut r = DagStructRef::new(&mut state);
    let mut texts = vec![];
    let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
    MapSelector::new(p.teams, p.selected).render(&mut r, &data, rect(), &mut ctx);
    assert!(text_values(&texts).iter().any(|t| t == "(none)"));
}

#[test]
fn click_header_opens_and_lists_sorted_keys() {
    let mut state = seeded_state();
    let p = SelState::ports();
    let mut data = SelectorData::default();
    let mut r = DagStructRef::new(&mut state);

    let response = MapSelector::new(p.teams, p.selected).on_event(
        &mut r,
        &mut data,
        &click(10.0, 10.0),
        rect(),
    );
    assert_eq!(response, EventResponse::Consumed);
    assert!(data.open);

    let mut fills = vec![];
    let mut outlines = vec![];
    let mut texts = vec![];
    let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
    MapSelector::new(p.teams, p.selected).render(&mut r, &data, rect(), &mut ctx);
    let values = text_values(&texts);
    let core = values.iter().position(|t| t == "core").unwrap();
    let ui = values.iter().position(|t| t == "ui").unwrap();
    assert!(core < ui, "keys listed in sorted order: {values:?}");

    // Clicking the header again closes.
    MapSelector::new(p.teams, p.selected).on_event(&mut r, &mut data, &click(10.0, 10.0), rect());
    assert!(!data.open);
}

#[test]
fn click_row_selects_and_writes_port() {
    let mut state = seeded_state();
    let p = SelState::ports();
    let mut data = SelectorData::default();
    let mut r = DagStructRef::new(&mut state);

    let sel = MapSelector::new(p.teams, p.selected);
    sel.on_event(&mut r, &mut data, &click(10.0, 10.0), rect()); // open
    let response = sel.on_event(&mut r, &mut data, &click(10.0, 58.0), rect()); // "ui" row
    assert_eq!(response, EventResponse::Consumed);
    assert_eq!(state.selected, "ui", "the picked key is written");
    assert!(!data.open, "list closes after picking");
}

#[test]
fn click_header_again_closes_without_selecting() {
    let mut state = seeded_state();
    let p = SelState::ports();
    let mut data = SelectorData::default();
    let mut r = DagStructRef::new(&mut state);

    let sel = MapSelector::new(p.teams, p.selected);
    sel.on_event(&mut r, &mut data, &click(10.0, 10.0), rect()); // open
    sel.on_event(&mut r, &mut data, &click(10.0, 10.0), rect()); // header again
    assert!(!data.open);
    assert_eq!(state.selected, "core", "closing never writes");
}

#[test]
fn click_below_the_list_closes() {
    let mut state = seeded_state();
    let p = SelState::ports();
    let mut data = SelectorData::default();
    let mut r = DagStructRef::new(&mut state);

    let sel = MapSelector::new(p.teams, p.selected);
    sel.on_event(&mut r, &mut data, &click(10.0, 10.0), rect()); // open
    // The widget is driven with a taller-than-layout rect here (300×200):
    // a click on the empty space below the rows closes without selecting.
    let response = sel.on_event(&mut r, &mut data, &click(10.0, 150.0), rect());
    assert_eq!(response, EventResponse::Consumed);
    assert!(!data.open);
    assert_eq!(state.selected, "core");
}

#[test]
fn layout_style_tracks_open_state() {
    let mut state = seeded_state();
    let p = SelState::ports();
    let data = SelectorData::default();
    let mut r = DagStructRef::new(&mut state);

    let style = MapSelector::new(p.teams, p.selected).layout_style(&r, &data);
    assert_eq!(style.min_size.height.value(), 22.0);

    let mut data = SelectorData::default();
    MapSelector::new(p.teams, p.selected).on_event(&mut r, &mut data, &click(10.0, 10.0), rect());
    let style = MapSelector::new(p.teams, p.selected).layout_style(&r, &data);
    assert_eq!(
        style.min_size.height.value(),
        22.0 + 2.0 * 24.0,
        "one option row per key when open"
    );
}

#[test]
fn hover_tracks_the_row_under_the_mouse() {
    let mut state = seeded_state();
    let p = SelState::ports();
    let mut data = SelectorData::default();
    let mut r = DagStructRef::new(&mut state);

    let sel = MapSelector::new(p.teams, p.selected);
    sel.on_event(&mut r, &mut data, &click(10.0, 10.0), rect()); // open

    sel.on_event(&mut r, &mut data, &mouse_move(10.0, 34.0), rect());
    assert_eq!(data.hover, Some(0));
    sel.on_event(&mut r, &mut data, &mouse_move(10.0, 58.0), rect());
    assert_eq!(data.hover, Some(1));
    sel.on_event(&mut r, &mut data, &mouse_move(10.0, 150.0), rect());
    assert_eq!(data.hover, None, "below the list is not a row");
    sel.on_event(&mut r, &mut data, &mouse_move(500.0, 500.0), rect());
    assert_eq!(data.hover, None, "outside the widget is not a row");

    sel.on_event(&mut r, &mut data, &click(10.0, 10.0), rect()); // close
    sel.on_event(&mut r, &mut data, &mouse_move(10.0, 34.0), rect());
    assert_eq!(data.hover, None, "no hover when closed");
}

#[test]
fn empty_map_shows_no_options_and_resets_stale_value() {
    let mut state = SelState {
        teams: HashMap::new(),
        selected: "ghost".to_string(),
    };
    let p = SelState::ports();
    let mut data = SelectorData::default();
    let mut r = DagStructRef::new(&mut state);

    let sel = MapSelector::new(p.teams, p.selected);
    let mut fills = vec![];
    let mut outlines = vec![];
    let mut texts = vec![];
    let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
    sel.render(&mut r, &data, rect(), &mut ctx);
    assert!(
        p.selected.read(&r).is_empty(),
        "stale value reset to the unset state"
    );
    assert!(text_values(&texts).iter().any(|t| t == "(none)"));

    sel.on_event(&mut r, &mut data, &click(10.0, 10.0), rect()); // open
    let mut texts = vec![];
    let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
    sel.render(&mut r, &data, rect(), &mut ctx);
    assert!(
        text_values(&texts).iter().any(|t| t == "(no options)"),
        "an open empty list says so"
    );

    // A click below the header (no rows to pick) just closes.
    sel.on_event(&mut r, &mut data, &click(10.0, 50.0), rect());
    assert!(!data.open);
}

// ── Dirty boundaries (the core of the widget contract) ──────────────────

/// A graph over both ports: a counter on the selected key and a counter on
/// the map.
fn build() -> (
    Graph<SelState>,
    Port<HashMap<String, Team>, SelState>,
    Port<String, SelState>,
    Rc<Cell<usize>>,
    Rc<Cell<usize>>,
) {
    let mut graph = Graph::new();
    let key_counter = Rc::new(Cell::new(0usize));
    let map_counter = Rc::new(Cell::new(0usize));
    let p = SelState::ports();
    let map = p.teams;
    let key = p.selected;

    graph.add_node(
        {
            let c = key_counter.clone();
            let k = key;
            move |gref: &mut DagStructRef<SelState>, _gpu| {
                c.set(c.get() + 1);
                let _ = k.read(gref);
            }
        },
        key,
        (),
        None,
    );
    graph.add_node(
        {
            let c = map_counter.clone();
            let m = map;
            move |gref: &mut DagStructRef<SelState>, _gpu| {
                c.set(c.get() + 1);
                let _ = m.read(gref).len();
            }
        },
        map,
        (),
        None,
    );

    (graph, map, key, key_counter, map_counter)
}

#[test]
fn render_and_layout_do_not_dirty() {
    let (mut graph, map, key, key_counter, map_counter) = build();
    let mut state = seeded_state();
    graph.tick(&mut state, None);
    assert_eq!((key_counter.get(), map_counter.get()), (1, 1));

    let data = SelectorData::default();
    {
        let mut r = graph.with_state(&mut state);
        let mut fills = vec![];
        let mut outlines = vec![];
        let mut texts = vec![];
        let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
        MapSelector::new(map, key).render(&mut r, &data, rect(), &mut ctx);
        let _ = MapSelector::new(map, key).layout_style(&r, &data);
    }
    graph.tick(&mut state, None);

    assert_eq!(
        (key_counter.get(), map_counter.get()),
        (1, 1),
        "a valid render/layout pass must not dirty either port"
    );
}

#[test]
fn clicking_the_selected_row_does_not_dirty() {
    let (mut graph, map, key, key_counter, _map_counter) = build();
    let mut state = seeded_state();
    graph.tick(&mut state, None);
    assert_eq!(key_counter.get(), 1);

    let mut data = SelectorData::default();
    {
        let mut r = graph.with_state(&mut state);
        let sel = MapSelector::new(map, key);
        sel.on_event(&mut r, &mut data, &click(10.0, 10.0), rect()); // open
        sel.on_event(&mut r, &mut data, &click(10.0, 34.0), rect()); // "core" again
    }
    graph.tick(&mut state, None);

    assert_eq!(state.selected, "core");
    assert_eq!(
        key_counter.get(),
        1,
        "re-picking the current key writes nothing"
    );
}

#[test]
fn selection_write_dirties_only_the_key_port() {
    let (mut graph, map, key, key_counter, map_counter) = build();
    let mut state = seeded_state();
    graph.tick(&mut state, None);
    assert_eq!((key_counter.get(), map_counter.get()), (1, 1));

    let mut data = SelectorData::default();
    {
        let mut r = graph.with_state(&mut state);
        let sel = MapSelector::new(map, key);
        sel.on_event(&mut r, &mut data, &click(10.0, 10.0), rect()); // open
        sel.on_event(&mut r, &mut data, &click(10.0, 58.0), rect()); // pick "ui"
    }
    graph.tick(&mut state, None);

    assert_eq!(state.selected, "ui");
    assert_eq!(key_counter.get(), 2, "the key port re-ran");
    assert_eq!(map_counter.get(), 1, "the map port never re-ran");
}

#[test]
fn stale_selection_resets_on_render_and_converges() {
    let (mut graph, map, key, key_counter, _map_counter) = build();
    let mut state = seeded_state();
    state.selected = "ghost".to_string(); // not a key — stale
    graph.tick(&mut state, None);
    assert_eq!(key_counter.get(), 1);

    let data = SelectorData::default();
    {
        let mut r = graph.with_state(&mut state);
        let mut fills = vec![];
        let mut outlines = vec![];
        let mut texts = vec![];
        let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
        MapSelector::new(map, key).render(&mut r, &data, rect(), &mut ctx);
        assert!(key.read(&r).is_empty(), "render resets the stale value");
    }
    graph.tick(&mut state, None);
    assert_eq!(key_counter.get(), 2, "the reset woke the key readers once");

    // A second clean pass (now valid) must not dirty anything.
    {
        let mut r = graph.with_state(&mut state);
        let mut fills = vec![];
        let mut outlines = vec![];
        let mut texts = vec![];
        let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
        MapSelector::new(map, key).render(&mut r, &data, rect(), &mut ctx);
    }
    graph.tick(&mut state, None);
    assert_eq!(key_counter.get(), 2, "converged: valid frames stay clean");
}

// ── GUI integration ─────────────────────────────────────────────────────

#[test]
fn gui_routes_clicks_to_the_selector() {
    let mut state = seeded_state();
    let p = SelState::ports();
    let node = WidgetNode::new(
        MapSelector::new(p.teams, p.selected),
        SelectorData::default(),
    );
    let mut gui = Gui::new(ViewportTree::new(ViewportNode::widget(node)));
    let fb = Vec2u::new(W as u32, H as u32);
    let mut r = DagStructRef::new(&mut state);

    gui.on_event(&click(10.0, 10.0), fb, &mut r); // open
    gui.on_event(&click(10.0, 58.0), fb, &mut r); // pick "ui"

    assert_eq!(state.selected, "ui");
}

// ── Derive attribute ────────────────────────────────────────────────────

/// A state that derives its widget tree: the `#[widget(selector = "teams")]`
/// attribute turns the `team` field into a `MapSelector` over `teams`.
#[derive(Clone, Default, DagStruct, Widget)]
struct DerivedState {
    teams: HashMap<String, Team>,
    #[widget(selector = "teams")]
    team: String,
}

#[test]
fn derive_emits_a_selector_for_the_annotated_field() {
    let nodes = DerivedState::into_viewport_nodes();
    assert_eq!(nodes.len(), 2, "one row per rendered field");
    assert_eq!(nodes[0].0, "teams");
    assert_eq!(nodes[1].0, "team");

    // The generated "team" row must be a working selector: header click
    // opens the list, row click writes the port.
    let mut state = DerivedState {
        teams: HashMap::from([
            (
                "core".to_string(),
                Team {
                    desc: "engine".to_string(),
                },
            ),
            (
                "ui".to_string(),
                Team {
                    desc: "gui".to_string(),
                },
            ),
        ]),
        team: "core".to_string(),
    };
    let mut gui = Gui::new(ViewportTree::new(ViewportNode::container(nodes)));
    let fb = Vec2u::new(W as u32, H as u32);
    let mut r = DagStructRef::new(&mut state);

    // Row 2 editor ("team"): padding 6 + label 96 + gap 8 →
    // y = 6 (padding) + 70 (teams row) + 8 (gap) = 84; header centre (150, 95).
    gui.on_event(&click(150.0, 95.0), fb, &mut r); // open the list
    gui.on_event(&click(150.0, 140.0), fb, &mut r); // pick "ui" (2nd list row)

    assert_eq!(state.team, "ui");
}

// ── Selector inside a map element (nested state) ────────────────────────

/// A map element whose template contains a selector: the `pick` field is
/// constrained to the element's own `options` map (same struct — the derive
/// attribute case, rendered through the map widget's `WidgetElement`
/// template).
#[derive(Clone, Default, DagStruct, Widget)]
struct ElementWithSelector {
    options: HashMap<String, Team>,
    #[widget(selector = "options")]
    pick: String,
}

#[derive(Clone, Default, DagStruct)]
struct OuterMapState {
    items: HashMap<String, ElementWithSelector>,
}

#[test]
fn selector_inside_a_map_element_edits_only_that_element() {
    let mut graph = Graph::new();
    let counter = Rc::new(Cell::new(0usize));

    let p = OuterMapState::ports();
    let map = p.items;
    let el = ElementWithSelector::ports();
    graph.add_map_node(map, (), (), el.pick, (), {
        let c = counter.clone();
        move |_gref, _gpu, _key, e: &mut DagStructRef<ElementWithSelector>| {
            c.set(c.get() + 1);
            let _ = el.pick.read(e);
        }
    });

    let mut state = OuterMapState {
        items: HashMap::from([
            (
                "e1".to_string(),
                ElementWithSelector {
                    options: HashMap::from([
                        (
                            "core".to_string(),
                            Team {
                                desc: "engine".to_string(),
                            },
                        ),
                        (
                            "ui".to_string(),
                            Team {
                                desc: "gui".to_string(),
                            },
                        ),
                    ]),
                    pick: "core".to_string(),
                },
            ),
            (
                "e2".to_string(),
                ElementWithSelector {
                    options: HashMap::from([(
                        "red".to_string(),
                        Team {
                            desc: "r".to_string(),
                        },
                    )]),
                    pick: "red".to_string(),
                },
            ),
        ]),
    };
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 2);

    let mut data = MapWidgetData::<ElementWithSelector>::default();
    {
        let mut r = graph.with_state(&mut state);
        // Expand "e1" (sorted first), render to measure the section, open
        // the selector, render again (its height grows), then pick "ui".
        map.on_event(&mut r, &mut data, &click(9.0, 11.0), rect());
        let mut fills = vec![];
        let mut outlines = vec![];
        let mut texts = vec![];
        let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
        map.render(&mut r, &data, rect(), &mut ctx);
        map.on_event(&mut r, &mut data, &click(200.0, 117.0), rect()); // open selector
        let mut fills = vec![];
        let mut outlines = vec![];
        let mut texts = vec![];
        let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
        map.render(&mut r, &data, rect(), &mut ctx);
        map.on_event(&mut r, &mut data, &click(200.0, 160.0), rect()); // pick "ui"
    }
    graph.tick(&mut state, None);

    assert_eq!(state.items["e1"].pick, "ui");
    assert_eq!(state.items["e2"].pick, "red", "sibling element untouched");
    assert_eq!(counter.get(), 3, "only the edited element reprocessed");
}
