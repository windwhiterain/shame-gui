use shame_gui::graph::DagStructRef;
use shame_gui::graph::port::Port;
use shame_gui::gui::{EventResponse, InputEvent, Key, MouseButton, Widget, WidgetNode};
use shame_gui::math::Vec2;
use shame_gui::rect::Rect;

// ── Number widget tests ────────────────────────────────────────────────────

use shame_gui::gui::primitives::number::NumberWidgetData;

fn field_rect() -> Rect {
    Rect::new(Vec2::new(100.0, 50.0), Vec2::new(80.0, 24.0))
}

fn click_inside() -> InputEvent {
    let r = field_rect();
    InputEvent::MouseDown {
        pos: r.pos + Vec2::new(5.0, 5.0),
        button: MouseButton::Left,
        pressure: None,
    }
}

// A small standalone element state for widget unit tests (no graph needed).
#[derive(Clone, Default, shame_gui::DagStruct)]
struct W {
    n: u32,
}

fn make_port(w: &mut W, value: u32) -> Port<u32, W> {
    w.n = value;
    W::ports().n
}

#[test]
fn number_click_starts_editing() {
    let mut w = W::default();
    let port = make_port(&mut w, 0);
    let mut nd = NumberWidgetData::default();
    let rect = field_rect();
    let mut r = DagStructRef::new(&mut w);
    let response = port.on_event(&mut r, &mut nd, &click_inside(), rect);
    assert_eq!(response, EventResponse::Consumed);
    assert!(nd.editing);
    assert_eq!(nd.buffer, "0");
}

#[test]
fn number_enter_commits_parsed_value() {
    let mut w = W::default();
    let port = make_port(&mut w, 0);
    let mut nd = NumberWidgetData::default();
    let rect = field_rect();
    let mut r = DagStructRef::new(&mut w);
    port.on_event(&mut r, &mut nd, &click_inside(), rect);
    nd.buffer.clear();
    nd.cursor = 0;
    port.on_event(&mut r, &mut nd, &InputEvent::Char { ch: '4' }, rect);
    port.on_event(&mut r, &mut nd, &InputEvent::Char { ch: '2' }, rect);
    port.on_event(
        &mut r,
        &mut nd,
        &InputEvent::KeyDown { key: Key::Enter },
        rect,
    );
    assert_eq!(*port.read(&r), 42);
    assert!(!nd.editing);
}

#[test]
fn number_invalid_input_shows_error() {
    let mut w = W::default();
    let port = make_port(&mut w, 0);
    let mut nd = NumberWidgetData::default();
    let rect = field_rect();
    let mut r = DagStructRef::new(&mut w);
    port.on_event(&mut r, &mut nd, &click_inside(), rect);
    nd.buffer.clear();
    nd.cursor = 0;
    port.on_event(&mut r, &mut nd, &InputEvent::Char { ch: 'a' }, rect);
    port.on_event(&mut r, &mut nd, &InputEvent::Char { ch: 'b' }, rect);
    port.on_event(
        &mut r,
        &mut nd,
        &InputEvent::KeyDown { key: Key::Enter },
        rect,
    );
    assert!(nd.error.is_some());
    assert!(nd.editing);
    assert_eq!(*port.read(&r), 0);
}

#[test]
fn number_escape_cancels_editing() {
    let mut w = W::default();
    let port = make_port(&mut w, 77);
    let mut nd = NumberWidgetData::default();
    let rect = field_rect();
    let mut r = DagStructRef::new(&mut w);
    port.on_event(&mut r, &mut nd, &click_inside(), rect);
    nd.buffer.clear();
    nd.cursor = 0;
    port.on_event(&mut r, &mut nd, &InputEvent::Char { ch: '9' }, rect);
    port.on_event(&mut r, &mut nd, &InputEvent::Char { ch: '9' }, rect);
    port.on_event(
        &mut r,
        &mut nd,
        &InputEvent::KeyDown { key: Key::Escape },
        rect,
    );
    assert_eq!(*port.read(&r), 77);
    assert!(!nd.editing);
}

#[test]
fn number_selectable_is_true() {
    let mut w = W::default();
    let port = make_port(&mut w, 0);
    assert!(port.selectable());
}

// ── String widget ──────────────────────────────────────────────────────────

use shame_gui::gui::primitives::string::StringWidgetData;

#[derive(Clone, Default, shame_gui::DagStruct)]
struct Ws {
    s: String,
}

#[test]
fn string_enter_commits_value() {
    let mut w = Ws { s: "old".into() };
    let port = Ws::ports().s;
    let mut sd = StringWidgetData::default();
    let rect = field_rect();
    let mut r = DagStructRef::new(&mut w);
    port.on_event(&mut r, &mut sd, &click_inside(), rect);
    sd.buffer.clear();
    sd.cursor = 0;
    port.on_event(&mut r, &mut sd, &InputEvent::Char { ch: 'n' }, rect);
    port.on_event(&mut r, &mut sd, &InputEvent::Char { ch: 'e' }, rect);
    port.on_event(&mut r, &mut sd, &InputEvent::Char { ch: 'w' }, rect);
    port.on_event(
        &mut r,
        &mut sd,
        &InputEvent::KeyDown { key: Key::Enter },
        rect,
    );
    assert_eq!(port.read(&r).as_str(), "new");
    assert!(!sd.editing);
}

// ── Container node: renders children with labels ──────────────────────────

use shame_gui::gui::ViewportNode;

fn make_test_widget(_w: &mut W) -> WidgetNode<W> {
    let port = W::ports().n;
    WidgetNode::new(port, NumberWidgetData::default())
}

#[test]
fn container_renders_labels_and_editors() {
    let mut w = W::default();
    let nodes: Vec<(String, ViewportNode<W>)> = vec![
        (
            "count".into(),
            ViewportNode::Widget(make_test_widget(&mut w)),
        ),
        (
            "name".into(),
            ViewportNode::Widget(make_test_widget(&mut w)),
        ),
    ];
    let node = ViewportNode::container(nodes);
    match node {
        ViewportNode::Container(children) => {
            assert_eq!(children.len(), 2);
            assert_eq!(children[0].0, "count");
            assert_eq!(children[1].0, "name");
        }
        _ => panic!("expected Container"),
    }
}

// ── Container focus routing ───────────────────────────────────────────────

use shame_gui::gui::{Gui, ViewportTree};

#[test]
fn container_focus_routes_enter_to_child() {
    let mut w = W::default();
    let port = W::ports().n;
    let widget = WidgetNode::new(port, NumberWidgetData::default());
    let child_id = widget.id();

    let children: Vec<(String, ViewportNode<W>)> = vec![("n".into(), ViewportNode::Widget(widget))];
    let tree = ViewportTree::new(ViewportNode::container(children));
    let mut gui = Gui::new(tree);
    let fb = shame_gui::math::Vec2u::new(800, 600);

    let mut r = DagStructRef::new(&mut w);
    gui.on_event(
        &InputEvent::MouseDown {
            pos: Vec2::new(110.0, 12.0),
            button: MouseButton::Left,
            pressure: None,
        },
        fb,
        &mut r,
    );
    gui.on_event(&InputEvent::Char { ch: '4' }, fb, &mut r);
    gui.on_event(&InputEvent::Char { ch: '2' }, fb, &mut r);
    gui.on_event(&InputEvent::KeyDown { key: Key::Enter }, fb, &mut r);

    assert_eq!(gui.focus, Some(child_id));
    assert_eq!(w.n, 42);
}

// ── Editing survives across frames ────────────────────────────────────────

#[test]
fn editing_survives_subsequent_events() {
    let mut w = W::default();
    let port = W::ports().n;
    let widget = WidgetNode::new(port, NumberWidgetData::default());

    let children: Vec<(String, ViewportNode<W>)> = vec![("v".into(), ViewportNode::Widget(widget))];
    let tree = ViewportTree::new(ViewportNode::container(children));
    let mut gui = Gui::new(tree);
    let fb = shame_gui::math::Vec2u::new(800, 600);

    let mut r = DagStructRef::new(&mut w);
    gui.on_event(
        &InputEvent::MouseDown {
            pos: Vec2::new(110.0, 12.0),
            button: MouseButton::Left,
            pressure: None,
        },
        fb,
        &mut r,
    );
    gui.on_event(
        &InputEvent::KeyDown {
            key: Key::Backspace,
        },
        fb,
        &mut r,
    );
    gui.on_event(
        &InputEvent::KeyDown {
            key: Key::Backspace,
        },
        fb,
        &mut r,
    );
    gui.on_event(&InputEvent::Char { ch: '4' }, fb, &mut r);
    gui.on_event(&InputEvent::Char { ch: '7' }, fb, &mut r);
    gui.on_event(&InputEvent::KeyDown { key: Key::Enter }, fb, &mut r);
    gui.on_event(
        &InputEvent::MouseMove {
            pos: Vec2::new(200.0, 100.0),
            pressure: None,
        },
        fb,
        &mut r,
    );

    assert_eq!(w.n, 47);
}
