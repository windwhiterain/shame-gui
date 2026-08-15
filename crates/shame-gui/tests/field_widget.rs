//! Tests for the nested-struct field widget (`Port<T, S>` where `T` derives
//! `Widget`): interaction, layout, dirty boundaries (the field port wakes
//! only on real sub-writes, at any nesting depth), and GUI keyboard routing.
//!
//! The widget is driven directly through `Port::on_event`/`render` (with
//! `DagStructRef::new` for isolated tracking) and through a real `Graph`
//! (via `graph.with_state`) to assert exactly which ports wake.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use shame_gui::DagStruct;
use shame_gui::graph::{DagStructRef, Graph, Port};
use shame_gui::gui::primitives::field::FieldWidgetData;
use shame_gui::gui::{
    EventResponse, Gui, InputEvent, Key, MouseButton, RenderContext, ViewportNode, ViewportTree,
    Widget, WidgetNode,
};
use shame_gui::math::{Vec2, Vec2u};
use shame_gui::rect::Rect;
use shame_gui::shader::RectEntry;
use shame_gui::text::TextObject;

// ── Test fixtures ───────────────────────────────────────────────────────

#[derive(Clone, Default, PartialEq, Debug, DagStruct, Widget)]
struct Audio {
    device: String,
    volume: f32,
    mute: bool,
}

#[derive(Clone, Default, PartialEq, Debug, DagStruct, Widget)]
struct Video {
    brightness: f32,
    hdr: bool,
}

/// A state holding nested widget structs as plain fields.
#[derive(Clone, Default, PartialEq, Debug, DagStruct, Widget)]
struct Root {
    audio: Audio,
    video: Video,
    master: f32,
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
        framebuffer: Vec2u::new(300, 200),
    }
}

fn text_values(texts: &[TextObject]) -> Vec<String> {
    texts.iter().map(|t| t.text.clone()).collect()
}

// Layout constants used by the tests (widget rect 300×100 = the Audio
// template height 3*24 + 2*8 + 2*6):
//   device editor: (110, 6) 184×24   volume editor: (110, 38) 184×24
//   mute editor:   (110, 70) 184×24  (padding 6, label 96, row gap 8)

fn audio_rect() -> Rect {
    Rect::new(Vec2::new(0.0, 0.0), Vec2::new(300.0, 100.0))
}

// ── Derive / rendering ──────────────────────────────────────────────────

#[test]
fn derive_emits_nested_rows() {
    let nodes = Root::into_viewport_nodes();
    let labels: Vec<&str> = nodes.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(labels, ["audio", "video", "master"]);

    // Each nested struct field renders its own rows via the field widget.
    let data = FieldWidgetData::<Audio>::default();
    let child_labels: Vec<&str> = data.template.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(child_labels, ["device", "volume", "mute"]);
}

#[test]
fn render_draws_nested_labels_and_values() {
    let mut state = Root {
        audio: Audio {
            device: "default".to_string(),
            volume: 7.0,
            mute: true,
        },
        ..Default::default()
    };
    let p = Root::ports();
    let data = FieldWidgetData::<Audio>::default();
    let mut r = DagStructRef::new(&mut state);
    let mut fills = vec![];
    let mut outlines = vec![];
    let mut texts = vec![];
    let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);

    p.audio.render(&mut r, &data, audio_rect(), &mut ctx);

    assert!(!fills.is_empty(), "editors draw field backgrounds");
    let values = text_values(&texts);
    for label in ["device", "volume", "mute"] {
        assert!(
            values.iter().any(|t| t == label),
            "{label} missing: {values:?}"
        );
    }
    assert!(
        values.iter().any(|t| t == "default"),
        "nested value rendered: {values:?}"
    );
}

// ── Interaction ─────────────────────────────────────────────────────────

#[test]
fn click_edits_nested_field() {
    let mut state = Root::default();
    let p = Root::ports();
    let mut data = FieldWidgetData::<Audio>::default();
    let mut r = DagStructRef::new(&mut state);

    // device row: label (6, 6) 96×24; editor (110, 6) 184×24.
    let response = p
        .audio
        .on_event(&mut r, &mut data, &click(202.0, 18.0), audio_rect());
    assert_eq!(response, EventResponse::Consumed);
    p.audio.on_event(
        &mut r,
        &mut data,
        &InputEvent::Char { ch: 'X' },
        audio_rect(),
    );
    p.audio.on_event(
        &mut r,
        &mut data,
        &InputEvent::KeyDown { key: Key::Enter },
        audio_rect(),
    );

    assert_eq!(state.audio.device, "X");
    assert_eq!(state.master, 0.0, "sibling fields untouched");
}

#[test]
fn click_outside_rect_is_ignored() {
    let mut state = Root::default();
    let p = Root::ports();
    let mut data = FieldWidgetData::<Audio>::default();
    let mut r = DagStructRef::new(&mut state);

    let response = p.audio.on_event(
        &mut r,
        &mut data,
        &click(290.0, 95.0), // inside the widget rect, but no row is there
        audio_rect(),
    );
    assert_eq!(response, EventResponse::Ignored);
    assert!(!state.audio.device.starts_with('X'));
}

#[test]
fn layout_style_uses_measured_height() {
    let mut state = Root::default();
    let p = Root::ports();
    let data = FieldWidgetData::<Audio>::default();
    let mut r = DagStructRef::new(&mut state);

    // Before the first render: the template estimate (3 rows).
    let style = p.audio.layout_style(&r, &data);
    assert_eq!(style.min_size.height.value(), 100.0);

    // After a render, the measured height matches the content.
    let mut fills = vec![];
    let mut outlines = vec![];
    let mut texts = vec![];
    let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
    p.audio.render(&mut r, &data, audio_rect(), &mut ctx);
    let style = p.audio.layout_style(&r, &data);
    assert_eq!(style.min_size.height.value(), 100.0);
}

// ── Dirty boundaries (the core of the widget contract) ──────────────────

/// A graph over `Root`: a node counting reads of the whole `audio` field,
/// and a node counting reads of `master` (a sibling field).
fn build() -> (
    Graph<Root>,
    Port<Audio, Root>,
    Rc<Cell<usize>>,
    Rc<Cell<usize>>,
) {
    let mut graph = Graph::new();
    let audio_counter = Rc::new(Cell::new(0usize));
    let master_counter = Rc::new(Cell::new(0usize));
    let p = Root::ports();

    graph.add_node(
        {
            let c = audio_counter.clone();
            move |gref: &mut DagStructRef<Root>, _gpu| {
                c.set(c.get() + 1);
                let _ = p.audio.read(gref).volume;
            }
        },
        p.audio,
        (),
        None,
    );
    graph.add_node(
        {
            let c = master_counter.clone();
            move |gref: &mut DagStructRef<Root>, _gpu| {
                c.set(c.get() + 1);
                let _ = p.master.read(gref);
            }
        },
        p.master,
        (),
        None,
    );
    (graph, p.audio, audio_counter, master_counter)
}

#[test]
fn render_and_layout_do_not_dirty_the_field() {
    let (mut graph, audio, audio_counter, master_counter) = build();
    let mut state = Root::default();
    graph.tick(&mut state, None);
    assert_eq!(audio_counter.get(), 1);
    assert_eq!(master_counter.get(), 1);

    let data = FieldWidgetData::<Audio>::default();
    {
        let mut r = graph.with_state(&mut state);
        let mut fills = vec![];
        let mut outlines = vec![];
        let mut texts = vec![];
        let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
        audio.render(&mut r, &data, audio_rect(), &mut ctx);
        let _ = audio.layout_style(&r, &data);
    }
    graph.tick(&mut state, None);

    assert_eq!(
        audio_counter.get(),
        1,
        "render/layout must not wake the field reader"
    );
    assert_eq!(master_counter.get(), 1, "sibling readers must not re-run");
}

#[test]
fn child_edit_wakes_only_the_field_reader() {
    let (mut graph, audio, audio_counter, master_counter) = build();
    let mut state = Root::default();
    graph.tick(&mut state, None);
    assert_eq!(audio_counter.get(), 1);

    let mut data = FieldWidgetData::<Audio>::default();
    {
        let mut r = graph.with_state(&mut state);
        audio.on_event(&mut r, &mut data, &click(202.0, 18.0), audio_rect());
        audio.on_event(
            &mut r,
            &mut data,
            &InputEvent::Char { ch: 'X' },
            audio_rect(),
        );
        audio.on_event(
            &mut r,
            &mut data,
            &InputEvent::KeyDown { key: Key::Enter },
            audio_rect(),
        );
    }
    graph.tick(&mut state, None);

    assert_eq!(state.audio.device, "X");
    assert_eq!(
        audio_counter.get(),
        2,
        "the audio field reader re-runs on a sub-write"
    );
    assert_eq!(master_counter.get(), 1, "sibling readers must not re-run");
}

// ── Deep nesting ────────────────────────────────────────────────────────

/// Three levels: `Deep { bus: Bus { fx: Fx { drive } } }`.
#[derive(Clone, Default, PartialEq, Debug, DagStruct, Widget)]
struct Fx {
    drive: f32,
}

#[derive(Clone, Default, PartialEq, Debug, DagStruct, Widget)]
struct Bus {
    fx: Fx,
    name: String,
}

#[derive(Clone, Default, PartialEq, Debug, DagStruct, Widget)]
struct Deep {
    bus: Bus,
    other: f32,
}

#[test]
fn deep_nesting_wakes_the_outer_field() {
    let mut graph = Graph::new();
    let counter = Rc::new(Cell::new(0usize));
    let p = Deep::ports();
    graph.add_node(
        {
            let c = counter.clone();
            move |gref: &mut DagStructRef<Deep>, _gpu| {
                c.set(c.get() + 1);
                let _ = p.bus.read(gref).name.len();
            }
        },
        p.bus,
        (),
        None,
    );

    let mut state = Deep::default();
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 1);

    // Bus template: [fx (1-row Fx, 36 tall) | name]; the fx editor sits at
    // (110, 6) 184×36, and inside it the drive editor at (220, 12) 68×24.
    let mut data = FieldWidgetData::<Bus>::default();
    let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(300.0, 80.0));
    {
        let mut r = graph.with_state(&mut state);
        let response = p.bus.on_event(&mut r, &mut data, &click(254.0, 24.0), rect);
        assert_eq!(response, EventResponse::Consumed);
        p.bus
            .on_event(&mut r, &mut data, &InputEvent::Char { ch: '5' }, rect);
        p.bus.on_event(
            &mut r,
            &mut data,
            &InputEvent::KeyDown { key: Key::Enter },
            rect,
        );
    }
    graph.tick(&mut state, None);

    assert_eq!(state.bus.fx.drive, 5.0);
    assert_eq!(
        counter.get(),
        2,
        "a 3-level edit wakes the outer field reader"
    );
    assert_eq!(state.other, 0.0);
}

// ── Nested map inside a nested struct ───────────────────────────────────

#[derive(Clone, Default, PartialEq, Debug, DagStruct, Widget)]
struct Item {
    v: f32,
}

#[derive(Clone, Default, PartialEq, Debug, DagStruct, Widget)]
struct Mixer {
    tracks: HashMap<String, Item>,
}

#[derive(Clone, Default, PartialEq, Debug, DagStruct, Widget)]
struct Studio {
    mixer: Mixer,
    other: f32,
}

#[test]
fn nested_map_edit_wakes_the_field_reader() {
    let mut graph = Graph::new();
    let counter = Rc::new(Cell::new(0usize));
    let p = Studio::ports();
    graph.add_node(
        {
            let c = counter.clone();
            move |gref: &mut DagStructRef<Studio>, _gpu| {
                c.set(c.get() + 1);
                let _ = p.mixer.read(gref).tracks.len();
            }
        },
        p.mixer,
        (),
        None,
    );

    let mut state = Studio {
        mixer: Mixer {
            // Number editors prefill their buffer with the current value,
            // so typing '2' yields "02" → 2.0 (leading zeros parse fine).
            tracks: HashMap::from([("drums".to_string(), Item { v: 0.0 })]),
        },
        ..Default::default()
    };
    graph.tick(&mut state, None);
    assert_eq!(counter.get(), 1);

    // Mixer template: one row holding the tracks map widget. Collapsed, the
    // map is 48 tall (add row 26 + one header 22); after expanding "drums"
    // it is 84 tall (48 + one 36-tall section). Render once after expanding
    // so the field widget's measured height matches, then click the item's
    // `v` editor: section (118, 28) 176×36, editor (228, 34) 60×24.
    let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(300.0, 120.0));
    let mut data = FieldWidgetData::<Mixer>::default();
    {
        let mut r = graph.with_state(&mut state);
        p.mixer
            .on_event(&mut r, &mut data, &click(119.0, 17.0), rect); // expand "drums"
        let mut fills = vec![];
        let mut outlines = vec![];
        let mut texts = vec![];
        let mut ctx = render_ctx(&mut fills, &mut outlines, &mut texts);
        p.mixer.render(&mut r, &data, rect, &mut ctx);
        let response = p
            .mixer
            .on_event(&mut r, &mut data, &click(258.0, 46.0), rect);
        assert_eq!(response, EventResponse::Consumed);
        p.mixer
            .on_event(&mut r, &mut data, &InputEvent::Char { ch: '2' }, rect);
        p.mixer.on_event(
            &mut r,
            &mut data,
            &InputEvent::KeyDown { key: Key::Enter },
            rect,
        );
    }
    graph.tick(&mut state, None);

    assert_eq!(state.mixer.tracks["drums"].v, 2.0);
    assert_eq!(
        counter.get(),
        2,
        "a map edit inside a struct wakes the field reader"
    );
}

// ── GUI integration ─────────────────────────────────────────────────────

#[test]
fn gui_routes_keyboard_to_nested_child() {
    let mut state = Root::default();
    let node = WidgetNode::new_default(Root::ports().audio);
    let audio_node_id = node.id();
    let mut gui = Gui::new(ViewportTree::new(ViewportNode::widget(node)));
    let fb = Vec2u::new(300, 200);
    let mut r = DagStructRef::new(&mut state);

    gui.on_event(&click(202.0, 18.0), fb, &mut r); // audio's device editor
    assert_eq!(
        gui.focus,
        Some(audio_node_id),
        "the field widget claims focus"
    );
    gui.on_event(&InputEvent::Char { ch: 'X' }, fb, &mut r);
    gui.on_event(&InputEvent::KeyDown { key: Key::Enter }, fb, &mut r);

    assert_eq!(state.audio.device, "X");
    assert_eq!(state.video.brightness, 0.0, "sibling struct untouched");
}
