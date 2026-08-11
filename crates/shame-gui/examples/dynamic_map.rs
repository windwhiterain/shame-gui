//! Dynamic HashMap fan-out: `Graph::add_map_node` processes a
//! `HashMap<K, E>` element-by-element.
//!
//! The state holds a `HashMap<u32, Element>`. An upstream node seeds the map
//! and (on a timer) mutates entries; the map node squares each element's `x`
//! into `y`, reprocessing only the keys that changed. The result is drawn as
//! filled rects in the built-in `fills` render port.
//!
//! Run with: cargo run -p shame-gui --example dynamic_map

use std::collections::HashMap;

use shame_gui::DagStruct;
use shame_gui::Vec2;
use shame_gui::app::App;
use shame_gui::color::Color;
use shame_gui::graph::DagStructRef;
use shame_gui::rect::Rect;
use shame_gui::shader::RectEntry;
use shame_gui::sm;
use shame_gui::state;
use shame_gui::text::TextSystem;

/// Per-element state: `x` is the input, `y` the computed output.
#[derive(Clone, Default, PartialEq, DagStruct)]
struct Element {
    x: f32,
    y: f32,
}

/// App state: built-in fields (injected by `#[state]`) + the map being fanned out.
#[state]
#[derive(Clone, Default, DagStruct)]
struct MapState {
    elements: HashMap<u32, Element>,
}

fn main() {
    let mut app = App::<MapState>::new("Dynamic HashMap fan-out");

    let p = MapState::ports();
    let render = <MapState as shame_gui::graph::AppState>::render_ports();
    let map = p.elements;
    let fills = render.fills;

    {
        let g = app.graph_mut();

        // Upstream: seed the map once, then keep it stable.
        g.add_node(
            {
                let m = map;
                move |gref: &mut DagStructRef<MapState>, _gpu: Option<&sm::Gpu>| {
                    let mut cur = m.read(gref).clone();
                    if cur.is_empty() {
                        for (i, x) in [2.0f32, 3.0, 4.0, 5.0].iter().enumerate() {
                            cur.insert(i as u32, Element { x: *x, y: 0.0 });
                        }
                        m.write(gref, cur);
                    }
                }
            },
            (),
            map,
            None,
        );

        // Fan-out: square x into y, per element.
        let el = Element::ports();
        g.add_map_node(
            map,
            (),
            (),
            el.x,
            el.y,
            move |_gref, _gpu, _key, e: &mut DagStructRef<Element>| {
                let x = *el.x.read(e);
                el.y.write(e, x * x);
            },
        );

        // Draw each element as a rect.
        g.add_node(
            {
                let m = map;
                move |gref: &mut DagStructRef<MapState>, _gpu: Option<&sm::Gpu>| {
                    let elements = m.read(gref).clone();
                    let mut rects = Vec::with_capacity(elements.len());
                    for (i, e) in elements.iter() {
                        rects.push(RectEntry {
                            rect: Rect::new(
                                Vec2::new(40.0 + *i as f32 * 160.0, 60.0),
                                Vec2::new(e.y * 8.0, e.y * 8.0),
                            ),
                            color: Color::rgb(0.3, 0.6, 0.9).to_linear(),
                            z: 0.1,
                        });
                    }
                    fills.write(gref, rects);
                }
            },
            map,
            fills,
            None,
        );
    }

    app.run(TextSystem::new());
}
