//! Click-to-place-rects: demonstrates the condition/event DAG feature.
//!
//! Two DAG nodes work together:
//!
//! 1. A **fire node** (ordinary dirty-driven node) reads the `mouse_down`
//!    source port, detects the rising edge of a click, and *fires* a `bool`
//!    condition via [`Port::fire`].
//! 2. An **add-rect node** is registered with `condition: Some(click_cond)`
//!    — it ignores the dirty mechanism entirely and runs only when that
//!    condition is fired. Each run appends a fixed-size rect at the current
//!    `mouse_pos` (the click position) into the built-in `fills`.
//!
//! Run with: cargo run -p shame-gui --example click_rects

use shame_gui::DagStruct;
use shame_gui::Vec2;
use shame_gui::app::App;
use shame_gui::color::Color;
use shame_gui::rect::Rect;
use shame_gui::shader::RectEntry;
use shame_gui::sm;
use shame_gui::state;
use shame_gui::text::TextSystem;

/// Side length of each placed rect, in physical pixels.
const RECT_SIZE: f32 = 40.0;

/// App state: built-in fields (injected by `#[state]`) + a one-shot click condition.
#[state]
#[derive(Clone, Default, DagStruct)]
pub struct ClickState {
    click_cond: bool,
}

pub fn click_rects_scene() -> App<ClickState> {
    let mut app = App::<ClickState>::new("shame-gui click rects");

    let p = ClickState::ports();
    let src = <ClickState as shame_gui::graph::AppState>::source_ports();
    let render = <ClickState as shame_gui::graph::AppState>::render_ports();

    let mouse_down = src.mouse_down;
    let mouse_pos = src.mouse_pos;
    let click_cond = p.click_cond;
    let fills_port = render.fills;

    {
        let g = app.graph_mut();

        // ── Fire node: detect the click's rising edge and fire the event ──
        let md = mouse_down;
        let cc = click_cond;
        let mut prev_down = false;
        g.add_node(
            move |gref: &mut shame_gui::graph::DagStructRef<ClickState>, _gpu: Option<&sm::Gpu>| {
                let down = *md.read(gref);
                if down && !prev_down {
                    cc.fire(gref);
                }
                prev_down = down;
            },
            mouse_down,
            click_cond,
            None,
        );

        // ── Add-rect node: runs only when the click condition fires ──────
        let mp = mouse_pos;
        let fp = fills_port;
        g.add_node(
            move |gref: &mut shame_gui::graph::DagStructRef<ClickState>, _gpu: Option<&sm::Gpu>| {
                let pos = *mp.read(gref);
                let mut fills = fp.read(gref).clone();
                fills.push(RectEntry {
                    rect: Rect::new(
                        Vec2::new(pos.x - RECT_SIZE * 0.5, pos.y - RECT_SIZE * 0.5),
                        Vec2::new(RECT_SIZE, RECT_SIZE),
                    ),
                    color: Color::rgb(0.9, 0.5, 0.3).to_linear(),
                    z: 0.1,
                });
                fp.write(gref, fills);
            },
            (),
            fills_port,
            Some(click_cond),
        );
    }

    app
}

fn main() {
    click_rects_scene().run(TextSystem::new());
}
