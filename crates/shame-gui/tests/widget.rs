use shame_gui::graph::StateArena;
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

fn make_number_port(arena: &mut StateArena, value: u32) -> Port<u32> {
    let port = Port::new(arena.alloc_with(value));
    port
}

#[test]
fn number_click_starts_editing() {
    let mut arena = StateArena::new();
    let port = make_number_port(&mut arena, 0);
    let mut nd = NumberWidgetData::default();
    let rect = field_rect();
    let response = port.on_event(&mut arena, &mut nd, &click_inside(), rect);
    assert_eq!(response, EventResponse::Consumed);
    assert!(nd.editing);
    assert_eq!(nd.buffer, "0");
}

#[test]
fn number_enter_commits_parsed_value() {
    let mut arena = StateArena::new();
    let port = make_number_port(&mut arena, 0);
    let mut nd = NumberWidgetData::default();
    let rect = field_rect();
    port.on_event(&mut arena, &mut nd, &click_inside(), rect);
    nd.buffer.clear();
    nd.cursor = 0;
    port.on_event(&mut arena, &mut nd, &InputEvent::Char { ch: '4' }, rect);
    port.on_event(&mut arena, &mut nd, &InputEvent::Char { ch: '2' }, rect);
    port.on_event(
        &mut arena,
        &mut nd,
        &InputEvent::KeyDown { key: Key::Enter },
        rect,
    );
    assert_eq!(*port.read(&arena), 42);
    assert!(!nd.editing);
}

#[test]
fn number_invalid_input_shows_error() {
    let mut arena = StateArena::new();
    let port = make_number_port(&mut arena, 0);
    let mut nd = NumberWidgetData::default();
    let rect = field_rect();
    port.on_event(&mut arena, &mut nd, &click_inside(), rect);
    nd.buffer.clear();
    nd.cursor = 0;
    port.on_event(&mut arena, &mut nd, &InputEvent::Char { ch: 'a' }, rect);
    port.on_event(&mut arena, &mut nd, &InputEvent::Char { ch: 'b' }, rect);
    port.on_event(
        &mut arena,
        &mut nd,
        &InputEvent::KeyDown { key: Key::Enter },
        rect,
    );
    assert!(nd.error.is_some());
    assert!(nd.editing);
    assert_eq!(*port.read(&arena), 0);
}

#[test]
fn number_escape_cancels_editing() {
    let mut arena = StateArena::new();
    let port = make_number_port(&mut arena, 77);
    let mut nd = NumberWidgetData::default();
    let rect = field_rect();
    port.on_event(&mut arena, &mut nd, &click_inside(), rect);
    nd.buffer.clear();
    nd.cursor = 0;
    port.on_event(&mut arena, &mut nd, &InputEvent::Char { ch: '9' }, rect);
    port.on_event(&mut arena, &mut nd, &InputEvent::Char { ch: '9' }, rect);
    port.on_event(
        &mut arena,
        &mut nd,
        &InputEvent::KeyDown { key: Key::Escape },
        rect,
    );
    assert_eq!(*port.read(&arena), 77);
    assert!(!nd.editing);
}

#[test]
fn number_selectable_is_true() {
    let mut arena = StateArena::new();
    let port = make_number_port(&mut arena, 0);
    assert!(port.selectable());
}

// ── String widget ──────────────────────────────────────────────────────────

use shame_gui::gui::primitives::string::StringWidgetData;

#[test]
fn string_enter_commits_value() {
    let mut arena = StateArena::new();
    let port = Port::<String>::new(arena.alloc_with(String::from("old")));
    let mut sd = StringWidgetData::default();
    let rect = field_rect();
    port.on_event(&mut arena, &mut sd, &click_inside(), rect);
    sd.buffer.clear();
    sd.cursor = 0;
    port.on_event(&mut arena, &mut sd, &InputEvent::Char { ch: 'n' }, rect);
    port.on_event(&mut arena, &mut sd, &InputEvent::Char { ch: 'e' }, rect);
    port.on_event(&mut arena, &mut sd, &InputEvent::Char { ch: 'w' }, rect);
    port.on_event(
        &mut arena,
        &mut sd,
        &InputEvent::KeyDown { key: Key::Enter },
        rect,
    );
    assert_eq!(port.read(&arena).as_str(), "new");
    assert!(!sd.editing);
}

// ── Container node: renders children with labels ──────────────────────────

use shame_gui::gui::ViewportNode;

fn make_test_widget(arena: &mut StateArena) -> WidgetNode {
    let port = Port::<u32>::new(arena.alloc::<u32>());
    WidgetNode::new(port, NumberWidgetData::default())
}

#[test]
fn container_renders_labels_and_editors() {
    let mut arena = StateArena::new();
    let nodes: Vec<(String, ViewportNode)> = vec![
        (
            "count".into(),
            ViewportNode::Widget(make_test_widget(&mut arena)),
        ),
        ("name".into(), {
            let port = Port::<String>::new(arena.alloc::<String>());
            ViewportNode::Widget(WidgetNode::new(port, StringWidgetData::default()))
        }),
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
    let mut arena = StateArena::new();
    let port = Port::<u32>::new(arena.alloc::<u32>());
    let widget = WidgetNode::new(port, NumberWidgetData::default());
    let child_id = widget.id();
    let port_id = port.id();

    let children: Vec<(String, ViewportNode)> = vec![("n".into(), ViewportNode::Widget(widget))];
    let tree = ViewportTree::new(ViewportNode::container(children));
    let mut gui = Gui::new(tree);
    let fb = shame_gui::math::Vec2u::new(800, 600);

    gui.on_event(
        &InputEvent::MouseDown {
            pos: Vec2::new(110.0, 12.0),
            button: MouseButton::Left,
            pressure: None,
        },
        fb,
        &mut arena,
    );
    gui.on_event(&InputEvent::Char { ch: '4' }, fb, &mut arena);
    gui.on_event(&InputEvent::Char { ch: '2' }, fb, &mut arena);
    gui.on_event(&InputEvent::KeyDown { key: Key::Enter }, fb, &mut arena);

    assert_eq!(gui.focus, Some(child_id));
    assert_eq!(*arena.read::<u32>(port_id), 42);
}

// ── Editing survives across frames ────────────────────────────────────────

#[test]
fn editing_survives_subsequent_events() {
    let mut arena = StateArena::new();
    let port = Port::<u32>::new(arena.alloc::<u32>());
    let widget = WidgetNode::new(port, NumberWidgetData::default());
    let port_id = port.id();

    let children: Vec<(String, ViewportNode)> = vec![("v".into(), ViewportNode::Widget(widget))];
    let tree = ViewportTree::new(ViewportNode::container(children));
    let mut gui = Gui::new(tree);
    let fb = shame_gui::math::Vec2u::new(800, 600);

    gui.on_event(
        &InputEvent::MouseDown {
            pos: Vec2::new(110.0, 12.0),
            button: MouseButton::Left,
            pressure: None,
        },
        fb,
        &mut arena,
    );
    gui.on_event(
        &InputEvent::KeyDown {
            key: Key::Backspace,
        },
        fb,
        &mut arena,
    );
    gui.on_event(
        &InputEvent::KeyDown {
            key: Key::Backspace,
        },
        fb,
        &mut arena,
    );
    gui.on_event(&InputEvent::Char { ch: '4' }, fb, &mut arena);
    gui.on_event(&InputEvent::Char { ch: '7' }, fb, &mut arena);
    gui.on_event(&InputEvent::KeyDown { key: Key::Enter }, fb, &mut arena);

    gui.on_event(
        &InputEvent::MouseMove {
            pos: Vec2::new(200.0, 100.0),
            pressure: None,
        },
        fb,
        &mut arena,
    );

    assert_eq!(*arena.read::<u32>(port_id), 47);
}
