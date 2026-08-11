use shame_gui::Vec2;
use shame_gui::Vec2u;
use shame_gui::rect::Rect;

#[test]
fn full_screen_rect_maps_to_full_ndc() {
    let r = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(800.0, 600.0));
    // pos.y is the bottom edge in NDC; size extends upward (−1 + 2 = +1 top edge)
    assert_eq!(
        r.to_ndc(Vec2u::new(800, 600)),
        Rect::new(Vec2::new(-1.0, -1.0), Vec2::new(2.0, 2.0))
    );
}

#[test]
fn y_axis_flips() {
    // top half of the screen in y-down pixels → upper half in y-up NDC
    let r = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(800.0, 300.0));
    assert_eq!(
        r.to_ndc(Vec2u::new(800, 600)),
        Rect::new(Vec2::new(-1.0, 0.0), Vec2::new(2.0, 1.0))
    );
}

#[test]
fn quarter_rect() {
    let r = Rect::new(Vec2::new(200.0, 150.0), Vec2::new(400.0, 300.0));
    assert_eq!(
        r.to_ndc(Vec2u::new(800, 600)),
        Rect::new(Vec2::new(-0.5, -0.5), Vec2::new(1.0, 1.0))
    );
}

#[test]
fn zero_size_rect() {
    let r = Rect::new(Vec2::new(400.0, 300.0), Vec2::new(0.0, 0.0));
    assert_eq!(
        r.to_ndc(Vec2u::new(800, 600)),
        Rect::new(Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.0))
    );
}

#[test]
fn rect_contains() {
    let rect = Rect::new(Vec2::new(10.0, 20.0), Vec2::new(100.0, 50.0));
    assert!(rect.contains(Vec2::new(10.0, 20.0)));
    assert!(rect.contains(Vec2::new(109.0, 69.0)));
    assert!(!rect.contains(Vec2::new(110.0, 20.0)));
    assert!(!rect.contains(Vec2::new(10.0, 70.0)));
    assert!(!rect.contains(Vec2::new(5.0, 20.0)));
}
