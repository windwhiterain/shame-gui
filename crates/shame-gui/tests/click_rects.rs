//! Click-to-place-rects test: drives the scene with an `AppRunner` that
//! simulates a sequence of clicks and verifies a rect is placed at each
//! clicked position.

#[path = "../examples/click_rects.rs"]
mod scene;

use shame_gui::Vec2;
use shame_gui::Vec2u;
use shame_gui::capture::{AppContext, AppRunner};
use shame_gui::graph::AppState;
use shame_gui::gui::{InputEvent, MouseButton};

const RECT_SIZE: f32 = 40.0;

/// Runner that both simulates clicks (returning `MouseDown`/`MouseUp`
/// events from `after_tick`) and verifies the resulting rects.
struct ClickRunner {
    clicks: Vec<(u32, Vec2)>,
    tick: u32,
}

impl<S: AppState + 'static> AppRunner<S> for ClickRunner {
    fn after_tick(&mut self, ctx: &AppContext<'_, S>) -> Vec<InputEvent> {
        self.tick += 1;
        let current = self.tick;

        let expected: Vec<Vec2> = self
            .clicks
            .iter()
            .filter(|(f, _)| *f < current)
            .map(|(_, p)| *p)
            .collect();

        // Read the built-in fills via the render ports.
        let fills = <S as shame_gui::graph::AppState>::builtins(ctx.state)
            .fills
            .clone();
        assert_eq!(
            fills.len(),
            expected.len(),
            "rect count after tick {current}"
        );
        for (entry, pos) in fills.iter().zip(expected.iter()) {
            let top_left = Vec2::new(pos.x - RECT_SIZE * 0.5, pos.y - RECT_SIZE * 0.5);
            assert_eq!(
                entry.rect.pos, top_left,
                "rect position after tick {current}"
            );
        }

        let mut events = Vec::new();
        for (f, pos) in &self.clicks {
            if *f == current {
                events.push(InputEvent::MouseDown {
                    pos: *pos,
                    button: MouseButton::Left,
                    pressure: None,
                });
            }
            if *f + 1 == current {
                events.push(InputEvent::MouseUp {
                    pos: *pos,
                    button: MouseButton::Left,
                    pressure: None,
                });
            }
        }
        events
    }
}

#[test]
fn click_places_rects() {
    let clicks = vec![
        (1, Vec2::new(100.0, 80.0)),
        (3, Vec2::new(300.0, 200.0)),
        (5, Vec2::new(500.0, 400.0)),
    ];

    let app = scene::click_rects_scene();
    let runner = ClickRunner {
        clicks: clicks.clone(),
        tick: 0,
    };
    let mut app = app.with_runner(runner);

    let fb_size = Vec2u::new(800, 600);
    for i in 0..7 {
        app.step(
            &[],
            1.0 / 60.0,
            i as f32 / 60.0,
            Vec2::new(0.0, 0.0),
            fb_size,
        );
    }

    let fills = <scene::ClickState as shame_gui::graph::AppState>::builtins(app.state())
        .fills
        .clone();
    assert_eq!(fills.len(), clicks.len());
    for (entry, (_, pos)) in fills.iter().zip(clicks.iter()) {
        let top_left = Vec2::new(pos.x - RECT_SIZE * 0.5, pos.y - RECT_SIZE * 0.5);
        assert_eq!(entry.rect.pos, top_left);
    }
}
