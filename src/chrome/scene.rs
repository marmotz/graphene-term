//! Interface rectangles of the shown tab (pure, no GPU access): the dividers between
//! its panes and the border of the active pane. Nothing is drawn while the tab shows a
//! single pane.

use crate::layout::Rect;
use crate::palette::UiColors;
use crate::renderer::RectInstance;
use crate::renderer::scene::gpu_color;
use crate::terminal::PaneId;
use crate::workspace::tree::Layout;

/// Thickness of the active pane border, in physical pixels.
pub const ACTIVE_BORDER: f32 = 2.0;

/// Rectangles drawn over the panes of `layout`: its dividers, in the pane border color,
/// then the border of the `active` pane, inside its rectangle, in the accent color
/// (the one highlighting command palette matches).
pub fn build(
    layout: &Layout,
    active: Option<PaneId>,
    colors: &UiColors,
    srgb_target: bool,
) -> Vec<RectInstance> {
    if layout.panes.len() < 2 {
        return Vec::new();
    }
    let divider = gpu_color(colors.pane_border, srgb_target);
    let mut rects: Vec<_> = layout
        .dividers
        .iter()
        .map(|d| rect(d.rect, divider))
        .collect();
    if let Some(pane) = active.and_then(|id| layout.rect_of(id)) {
        let accent = gpu_color(colors.palette_match, srgb_target);
        rects.extend(border(pane, ACTIVE_BORDER, accent));
    }
    rects
}

fn rect(r: Rect, color: [f32; 4]) -> RectInstance {
    RectInstance::new([r.x, r.y], [r.width, r.height], color)
}

/// The four sides of a `thickness` pixels border inside `r`: top, bottom, left, right.
fn border(r: Rect, thickness: f32, color: [f32; 4]) -> [RectInstance; 4] {
    let side = |x: f32, y: f32, width: f32, height: f32| {
        rect(
            Rect {
                x,
                y,
                width,
                height,
            },
            color,
        )
    };
    let inner = (r.height - 2.0 * thickness).max(0.0);
    [
        side(r.x, r.y, r.width, thickness),
        side(r.x, r.y + r.height - thickness, r.width, thickness),
        side(r.x, r.y + thickness, thickness, inner),
        side(r.x + r.width - thickness, r.y + thickness, thickness, inner),
    ]
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::vte::ansi::Rgb;

    use super::*;
    use crate::workspace::tree::{Axis, Branch, Divider};

    const BORDER: Rgb = Rgb { r: 1, g: 2, b: 3 };
    const ACCENT: Rgb = Rgb {
        r: 200,
        g: 100,
        b: 50,
    };

    fn colors() -> UiColors {
        UiColors {
            pane_border: BORDER,
            palette_match: ACCENT,
            ..UiColors::uniform(Rgb { r: 0, g: 0, b: 0 })
        }
    }

    fn area(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Two panes side by side in a 201 x 100 area, split by a 1 px divider.
    fn two_columns() -> Layout {
        Layout {
            panes: vec![
                (PaneId(1), area(0.0, 0.0, 100.0, 100.0)),
                (PaneId(2), area(101.0, 0.0, 100.0, 100.0)),
            ],
            dividers: vec![Divider {
                node_path: Vec::<Branch>::new(),
                axis: Axis::Horizontal,
                rect: area(100.0, 0.0, 1.0, 100.0),
            }],
            divider: 1.0,
        }
    }

    /// (position, size, color) of each rect.
    fn drawn(rects: &[RectInstance]) -> Vec<([f32; 2], [f32; 2], [f32; 4])> {
        rects
            .iter()
            .map(|r| (r.position, r.size, r.color))
            .collect()
    }

    #[test]
    fn a_single_pane_draws_no_border() {
        let layout = Layout {
            panes: vec![(PaneId(1), area(0.0, 0.0, 200.0, 100.0))],
            dividers: Vec::new(),
            divider: 1.0,
        };
        assert!(build(&layout, Some(PaneId(1)), &colors(), false).is_empty());
    }

    #[test]
    fn several_panes_draw_the_dividers_then_the_active_border() {
        let rects = build(&two_columns(), Some(PaneId(2)), &colors(), false);
        let border = gpu_color(BORDER, false);
        let accent = gpu_color(ACCENT, false);
        assert_eq!(
            drawn(&rects),
            vec![
                ([100.0, 0.0], [1.0, 100.0], border),
                // Inside the active pane, 2 px thick: top, bottom, left, right.
                ([101.0, 0.0], [100.0, 2.0], accent),
                ([101.0, 98.0], [100.0, 2.0], accent),
                ([101.0, 2.0], [2.0, 96.0], accent),
                ([199.0, 2.0], [2.0, 96.0], accent),
            ]
        );
    }

    #[test]
    fn unknown_active_pane_draws_only_the_dividers() {
        let rects = build(&two_columns(), Some(PaneId(9)), &colors(), false);
        assert_eq!(rects.len(), 1);
        assert!(build(&two_columns(), None, &colors(), false).len() == 1);
    }

    #[test]
    fn colors_follow_the_target_encoding() {
        let rects = build(&two_columns(), Some(PaneId(1)), &colors(), true);
        assert_eq!(rects[0].color, gpu_color(BORDER, true));
        assert_eq!(rects[1].color, gpu_color(ACCENT, true));
    }
}
