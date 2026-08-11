//! Form scene: interactive widgets — checkbox, text inputs, tabs, split divider.

use shame_gui::app::App;
use shame_gui::graph::Port;
use shame_gui::gui::primitives::number::NumberWidgetData;
use shame_gui::gui::primitives::string::StringWidgetData;
use shame_gui::gui::{Gui, SplitDir, ViewportNode, ViewportTree, WidgetNode};
use shame_gui::state;
use shame_gui::text::TextSystem;
use shame_gui::{DagStruct, Widget};

/// One combined form state: built-in fields (injected by `#[state]`) + login + settings + about.
#[state]
#[derive(Clone, Default, DagStruct, Widget)]
pub struct FormState {
    // login
    username: String,
    password: String,
    remember: bool,
    // settings
    volume: f32,
    sensitivity: u32,
    muted: bool,
    label: String,
    // about
    version: String,
    credits: String,
}

fn text<S: 'static>(p: Port<String, S>) -> WidgetNode<S> {
    WidgetNode::new(p, StringWidgetData::default())
}

fn num<S: 'static>(p: Port<f32, S>) -> WidgetNode<S> {
    WidgetNode::new(p, NumberWidgetData::default())
}

fn unum<S: 'static>(p: Port<u32, S>) -> WidgetNode<S> {
    WidgetNode::new(p, NumberWidgetData::default())
}

pub fn form_scene() -> App<FormState> {
    let mut app = App::<FormState>::new("shame-gui form");
    {
        let s = app.state_mut();
        s.username = "alice".into();
        s.password = String::new();
        s.remember = true;
        s.volume = 0.6;
        s.sensitivity = 3;
        s.muted = false;
        s.label = "default".into();
        s.version = "0.1.0".into();
        s.credits = "shame-gui".into();
    }

    let p = FormState::ports();

    // ── Login: table of username / password / remember ──────────────────
    let login = ViewportNode::container(vec![
        ("username".into(), ViewportNode::Widget(text(p.username))),
        ("password".into(), ViewportNode::Widget(text(p.password))),
        (
            "remember".into(),
            ViewportNode::Widget(WidgetNode::new_default(p.remember)),
        ),
    ]);

    // ── Settings: one tab per field (tab of labelled container rows) ─────
    let settings = ViewportNode::tab(vec![
        (
            "volume".into(),
            ViewportNode::container(vec![("volume".into(), ViewportNode::Widget(num(p.volume)))]),
        ),
        (
            "sensitivity".into(),
            ViewportNode::container(vec![(
                "sensitivity".into(),
                ViewportNode::Widget(unum(p.sensitivity)),
            )]),
        ),
        (
            "muted".into(),
            ViewportNode::container(vec![(
                "muted".into(),
                ViewportNode::Widget(WidgetNode::new_default(p.muted)),
            )]),
        ),
        (
            "label".into(),
            ViewportNode::container(vec![("label".into(), ViewportNode::Widget(text(p.label)))]),
        ),
    ]);

    // ── About: table of version / credits ────────────────────────────────
    let about = ViewportNode::container(vec![
        ("version".into(), ViewportNode::Widget(text(p.version))),
        ("credits".into(), ViewportNode::Widget(text(p.credits))),
    ]);

    let tree = ViewportTree::new(ViewportNode::split(
        SplitDir::Horizontal,
        0.5,
        login,
        ViewportNode::tab(vec![("Settings".into(), settings), ("About".into(), about)]),
    ));
    app.add_gui(Gui::new(tree));
    app
}

fn main() {
    form_scene().run(TextSystem::new());
}
