//! Form scene: interactive widgets — checkbox, button, tabs, split divider.

#![allow(dead_code)]

use shame_gui::DagStruct;
use shame_gui::Widget;
use shame_gui::app::App;
use shame_gui::gui::{Gui, SplitDir, ViewportNode, ViewportTree};

#[derive(Clone, Default, DagStruct, Widget)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
    pub remember: bool,
}

#[derive(Clone, Default, DagStruct, Widget)]
#[widget(tab)]
pub struct SettingsForm {
    pub volume: f32,
    pub sensitivity: u32,
    pub muted: bool,
    pub label: String,
}

#[derive(Clone, Default, DagStruct, Widget)]
pub struct AboutForm {
    pub version: String,
    pub credits: String,
}

pub fn form_scene() -> App {
    let mut app = App::new("shame-gui form");
    let login = LoginForm {
        username: "alice".into(),
        password: String::new(),
        remember: true,
    };
    let settings = SettingsForm {
        volume: 0.6,
        sensitivity: 3,
        muted: false,
        label: "default".into(),
    };
    let about = AboutForm {
        version: "0.1.0".into(),
        credits: "shame-gui".into(),
    };
    let arena = app.arena_mut();
    let settings_tabs: Vec<(String, ViewportNode)> = settings
        .into_tab_nodes(arena, None)
        .into_iter()
        .map(|(name, children)| (name, ViewportNode::container(children)))
        .collect();
    let tree = ViewportTree::new(ViewportNode::split(
        SplitDir::Horizontal,
        0.5,
        ViewportNode::container(login.into_viewport_nodes(arena, None)),
        ViewportNode::tab(vec![
            ("Settings".into(), ViewportNode::tab(settings_tabs)),
            (
                "About".into(),
                ViewportNode::container(about.into_viewport_nodes(arena, None)),
            ),
        ]),
    ));
    app.add_gui(Gui::new(tree));
    app
}
