//! Per-frame rect computation for the viewport tree: table (container),
//! split, divider, and tab rect helpers. Pure arithmetic — no state.

use crate::math::Vec2;
use crate::rect::Rect;

/// Table layout: one `[label | editor]` row per child. Row height follows
/// the editor's own style; the label column is `label_width` wide with a
/// `row_gap` between label and editor. Returns `(label_rect, editor_rect)`
/// pairs in pixel space, in the same order as `child_styles`.
pub fn table_rects(
    container_style: &taffy::Style,
    child_styles: &[taffy::Style],
    label_width: f32,
    row_gap: f32,
    container: Rect,
) -> Vec<(Rect, Rect)> {
    let mut tree: taffy::TaffyTree = taffy::TaffyTree::new();
    // The root fills the assigned container rect: width is always
    // definite (flex_grow editors need it), and height matches the
    // parent-assigned space so auto-height children can stretch to fill.
    let mut root_style = container_style.clone();
    root_style.size = taffy::Size {
        width: taffy::Dimension::length(container.size.x),
        height: taffy::Dimension::length(container.size.y),
    };
    let root = tree.new_leaf(root_style).unwrap();
    let mut rows = Vec::with_capacity(child_styles.len());
    for style in child_styles {
        // Auto-height editors (no explicit size or min_size height)
        // get flex_grow on the row so it stretches to fill the container.
        let editor_has_height = !style.size.height.is_auto() || !style.min_size.height.is_auto();
        let mut row_style = taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Row,
            gap: taffy::Size::length(row_gap),
            ..Default::default()
        };
        if !editor_has_height {
            row_style.flex_grow = 1.0;
        }
        let row = tree.new_leaf(row_style).unwrap();
        let label = tree
            .new_leaf(taffy::Style {
                min_size: taffy::Size {
                    width: taffy::Dimension::length(label_width),
                    height: taffy::Dimension::auto(),
                },
                ..Default::default()
            })
            .unwrap();
        let mut editor_style = style.clone();
        editor_style.flex_grow = 1.0;
        let editor = tree.new_leaf(editor_style).unwrap();
        tree.add_child(row, label).unwrap();
        tree.add_child(row, editor).unwrap();
        tree.add_child(root, row).unwrap();
        rows.push((label, editor));
    }
    tree.compute_layout(
        root,
        taffy::Size {
            width: taffy::AvailableSpace::Definite(container.size.x),
            height: taffy::AvailableSpace::Definite(container.size.y),
        },
    )
    .unwrap();
    rows.into_iter()
        .map(|(label, editor)| {
            let row = tree.parent(label).unwrap();
            let row_layout = tree.layout(row).unwrap();
            let label_layout = tree.layout(label).unwrap();
            let editor_layout = tree.layout(editor).unwrap();
            let ox = container.pos.x + row_layout.location.x;
            let oy = container.pos.y + row_layout.location.y;
            (
                Rect::new(
                    Vec2::new(ox + label_layout.location.x, oy + label_layout.location.y),
                    Vec2::new(label_layout.size.width, label_layout.size.height),
                ),
                Rect::new(
                    Vec2::new(ox + editor_layout.location.x, oy + editor_layout.location.y),
                    Vec2::new(editor_layout.size.width, editor_layout.size.height),
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_rows_stack() {
        // capture nothing; just run the computation to assert sanity
        let container_style = taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Column,
            gap: taffy::Size::length(8.0),
            padding: taffy::Rect::length(6.0),
            ..Default::default()
        };
        let child = taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Row,
            min_size: taffy::Size {
                width: taffy::Dimension::auto(),
                height: taffy::Dimension::length(24.0),
            },
            padding: taffy::Rect::length(4.0),
            ..Default::default()
        };
        let container = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(300.0, 200.0));
        let rows = table_rects(
            &container_style,
            &[child.clone(), child],
            96.0,
            8.0,
            container,
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].0.pos,
            Vec2::new(6.0, 6.0),
            "first label starts after root padding"
        );
        assert_eq!(
            rows[1].0.pos.y,
            rows[0].0.pos.y + rows[0].0.size.y + 8.0,
            "rows stack with gap"
        );
    }
}
