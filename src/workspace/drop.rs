//! Where a pane dragged by its title bar would land: the drop target under the
//! pointer and the rectangle highlighting it.
//!
//! Pure geometry over the pane layout of the shown tab and the tab bar, so it is
//! tested without a window.

use crate::chrome::tab_bar::{TabBarHit, TabBarLayout};
use crate::i18n::Direction;
use crate::layout::Rect;
use crate::terminal::PaneId;

use super::tree::Layout;

/// Fraction of the width (or height) of a pane between each of its edges and its
/// center zone.
pub const CENTER_MARGIN: f32 = 0.25;

/// Part of a pane the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// Near an edge, outside the center.
    Edge(Direction),
    /// The inner rectangle at [`CENTER_MARGIN`] from each edge.
    Center,
}

/// What a dragged pane is dropped onto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // The pane drag and drop is tracked by issue #137.
pub enum DropTarget {
    /// Split `pane` on `side`, the moved pane on that side.
    Edge { pane: PaneId, side: Direction },
    /// Swap with `pane`.
    Center { pane: PaneId },
    /// Into tab `index`, splitting its active pane.
    Tab(usize),
    /// Into a new tab, last.
    NewTab,
}

/// Zone of `rect` the pixel `(x, y)` falls in: the center, else the nearest edge, the
/// distances to the edges normalised by the width or the height.
pub fn zone(rect: Rect, x: f32, y: f32) -> Zone {
    let fx = if rect.width > 0.0 {
        (x - rect.x) / rect.width
    } else {
        0.5
    };
    let fy = if rect.height > 0.0 {
        (y - rect.y) / rect.height
    } else {
        0.5
    };
    let inner = CENTER_MARGIN..1.0 - CENTER_MARGIN;
    if inner.contains(&fx) && inner.contains(&fy) {
        return Zone::Center;
    }
    [
        (fx, Direction::Left),
        (1.0 - fx, Direction::Right),
        (fy, Direction::Up),
        (1.0 - fy, Direction::Down),
    ]
    .into_iter()
    .fold(
        None::<(f32, Direction)>,
        |best, (distance, side)| match best {
            Some((nearest, _)) if nearest <= distance => best,
            _ => Some((distance, side)),
        },
    )
    .map_or(Zone::Center, |(_, side)| Zone::Edge(side))
}

/// Target under the pixel `(x, y)` for the pane `dragged`.
///
/// Over the tab bar: `Tab(i)` on a tab or its close button, `NewTab` on the `+`
/// button or the empty part of the bar. Over a pane of the shown tab (`panes`): `Edge`
/// or `Center` by [`zone`], none over `dragged` itself, and none for an `Edge`
/// `can_insert(pane, side)` refuses (the move would not fit).
#[allow(dead_code)] // The pane drag and drop is tracked by issue #137.
pub fn target(
    panes: &Layout,
    tab_bar: Option<&TabBarLayout>,
    dragged: PaneId,
    x: f32,
    y: f32,
    can_insert: impl Fn(PaneId, Direction) -> bool,
) -> Option<DropTarget> {
    if let Some(bar) = tab_bar.filter(|bar| bar.rect.contains(x, y)) {
        return Some(match bar.hit(x, y) {
            Some(TabBarHit::Tab(index) | TabBarHit::Close(index)) => DropTarget::Tab(index),
            Some(TabBarHit::NewTab) | None => DropTarget::NewTab,
        });
    }
    let (pane, rect) = panes
        .panes
        .iter()
        .copied()
        .find(|(_, rect)| rect.contains(x, y))?;
    if pane == dragged {
        return None;
    }
    match zone(rect, x, y) {
        Zone::Center => Some(DropTarget::Center { pane }),
        Zone::Edge(side) => can_insert(pane, side).then_some(DropTarget::Edge { pane, side }),
    }
}

/// Rectangle highlighting `target`: the half of the pane on the side of an `Edge`,
/// the whole pane for `Center`, the slot of the tab for `Tab`, the `+` button for
/// `NewTab`. `None` when the pane or tab is not laid out.
#[allow(dead_code)] // The pane drag and drop is tracked by issue #137.
pub fn highlight(
    target: DropTarget,
    panes: &Layout,
    tab_bar: Option<&TabBarLayout>,
) -> Option<Rect> {
    match target {
        DropTarget::Edge { pane, side } => panes.rect_of(pane).map(|rect| half(rect, side)),
        DropTarget::Center { pane } => panes.rect_of(pane),
        DropTarget::Tab(index) => tab_bar?.tabs.get(index).map(|tab| tab.rect),
        DropTarget::NewTab => tab_bar.map(|bar| bar.new_tab),
    }
}

/// Half of `rect` on `side`.
fn half(rect: Rect, side: Direction) -> Rect {
    let (width, height) = (rect.width / 2.0, rect.height / 2.0);
    match side {
        Direction::Left => Rect { width, ..rect },
        Direction::Right => Rect {
            x: rect.x + rect.width - width,
            width,
            ..rect
        },
        Direction::Up => Rect { height, ..rect },
        Direction::Down => Rect {
            y: rect.y + rect.height - height,
            height,
            ..rect
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::tab_bar::UiMetrics;
    use crate::workspace::TabBarPosition;

    const METRICS: UiMetrics = UiMetrics {
        line_height: 18.0,
        scale: 1.0,
    };

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Panes 0 and 1 side by side under a 40 px tab bar, each 400 x 360.
    fn panes() -> Layout {
        Layout {
            panes: vec![
                (PaneId(0), rect(0.0, 40.0, 400.0, 360.0)),
                (PaneId(1), rect(401.0, 40.0, 400.0, 360.0)),
            ],
            ..Layout::default()
        }
    }

    fn bar(count: usize) -> TabBarLayout {
        let titles: Vec<String> = (0..count).map(|i| format!("tab {i}")).collect();
        TabBarLayout::new(
            Rect::from_size(801, 400),
            TabBarPosition::Top,
            &titles,
            0,
            METRICS,
        )
    }

    fn center(rect: Rect) -> (f32, f32) {
        (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    fn any(_: PaneId, _: Direction) -> bool {
        true
    }

    #[test]
    fn zone_center_is_the_inner_quarter_margins() {
        let r = rect(100.0, 100.0, 200.0, 100.0);
        assert_eq!(zone(r, 200.0, 150.0), Zone::Center);
        assert_eq!(zone(r, 150.0, 125.0), Zone::Center);
        assert_eq!(zone(r, 249.0, 174.0), Zone::Center);
        assert_ne!(zone(r, 149.0, 150.0), Zone::Center);
        assert_ne!(zone(r, 200.0, 124.0), Zone::Center);
    }

    #[test]
    fn zone_outside_the_center_is_the_nearest_edge() {
        let r = rect(0.0, 0.0, 200.0, 100.0);
        assert_eq!(zone(r, 10.0, 50.0), Zone::Edge(Direction::Left));
        assert_eq!(zone(r, 190.0, 50.0), Zone::Edge(Direction::Right));
        assert_eq!(zone(r, 100.0, 5.0), Zone::Edge(Direction::Up));
        assert_eq!(zone(r, 100.0, 95.0), Zone::Edge(Direction::Down));
    }

    #[test]
    fn zone_distances_are_normalised_by_the_pane_size() {
        // 20 px from the left (10 % of the width) beats 15 px from the top (15 % of
        // the height): in pixels the top is nearer.
        let r = rect(0.0, 0.0, 200.0, 100.0);
        assert_eq!(zone(r, 20.0, 15.0), Zone::Edge(Direction::Left));
        assert_eq!(zone(r, 40.0, 5.0), Zone::Edge(Direction::Up));
    }

    #[test]
    fn target_over_another_pane_is_its_edge_or_center() {
        let layout = panes();
        assert_eq!(
            target(&layout, None, PaneId(0), 600.0, 220.0, any),
            Some(DropTarget::Center { pane: PaneId(1) })
        );
        assert_eq!(
            target(&layout, None, PaneId(0), 790.0, 220.0, any),
            Some(DropTarget::Edge {
                pane: PaneId(1),
                side: Direction::Right
            })
        );
        assert_eq!(
            target(&layout, None, PaneId(0), 600.0, 45.0, any),
            Some(DropTarget::Edge {
                pane: PaneId(1),
                side: Direction::Up
            })
        );
    }

    #[test]
    fn no_target_over_the_dragged_pane_nor_outside_the_panes() {
        let layout = panes();
        assert_eq!(target(&layout, None, PaneId(0), 200.0, 220.0, any), None);
        assert_eq!(target(&layout, None, PaneId(0), 900.0, 220.0, any), None);
        // The divider between the panes.
        assert_eq!(target(&layout, None, PaneId(0), 400.5, 220.0, any), None);
    }

    #[test]
    fn a_refused_edge_is_no_target_but_the_center_stays() {
        let layout = panes();
        let refuse =
            |pane: PaneId, side: Direction| !(pane == PaneId(1) && side == Direction::Right);
        assert_eq!(target(&layout, None, PaneId(0), 790.0, 220.0, refuse), None);
        assert_eq!(
            target(&layout, None, PaneId(0), 410.0, 220.0, refuse),
            Some(DropTarget::Edge {
                pane: PaneId(1),
                side: Direction::Left
            })
        );
        assert_eq!(
            target(&layout, None, PaneId(0), 600.0, 220.0, |_, _| false),
            Some(DropTarget::Center { pane: PaneId(1) })
        );
    }

    #[test]
    fn target_over_the_tab_bar_is_a_tab_or_a_new_tab() {
        let layout = panes();
        let bar = bar(3);
        let (x, y) = center(bar.tabs[1].rect);
        assert_eq!(
            target(&layout, Some(&bar), PaneId(0), x, y, any),
            Some(DropTarget::Tab(1))
        );
        let (x, y) = center(bar.tabs[2].close);
        assert_eq!(
            target(&layout, Some(&bar), PaneId(0), x, y, any),
            Some(DropTarget::Tab(2))
        );
        let (x, y) = center(bar.new_tab);
        assert_eq!(
            target(&layout, Some(&bar), PaneId(0), x, y, any),
            Some(DropTarget::NewTab)
        );
        let empty = bar.new_tab.x + bar.new_tab.width + 10.0;
        assert!(bar.rect.contains(empty, y));
        assert_eq!(
            target(&layout, Some(&bar), PaneId(0), empty, y, any),
            Some(DropTarget::NewTab)
        );
    }

    #[test]
    fn highlight_covers_the_side_half_the_pane_the_slot_or_the_button() {
        let layout = panes();
        let bar = bar(2);
        let pane = rect(401.0, 40.0, 400.0, 360.0);
        let edge = |side| {
            highlight(
                DropTarget::Edge {
                    pane: PaneId(1),
                    side,
                },
                &layout,
                None,
            )
        };
        assert_eq!(edge(Direction::Left), Some(rect(401.0, 40.0, 200.0, 360.0)));
        assert_eq!(
            edge(Direction::Right),
            Some(rect(601.0, 40.0, 200.0, 360.0))
        );
        assert_eq!(edge(Direction::Up), Some(rect(401.0, 40.0, 400.0, 180.0)));
        assert_eq!(
            edge(Direction::Down),
            Some(rect(401.0, 220.0, 400.0, 180.0))
        );
        assert_eq!(
            highlight(DropTarget::Center { pane: PaneId(1) }, &layout, None),
            Some(pane)
        );
        assert_eq!(
            highlight(DropTarget::Tab(1), &layout, Some(&bar)),
            Some(bar.tabs[1].rect)
        );
        assert_eq!(
            highlight(DropTarget::NewTab, &layout, Some(&bar)),
            Some(bar.new_tab)
        );
    }

    #[test]
    fn highlight_of_an_unknown_pane_or_tab_is_none() {
        let layout = panes();
        assert_eq!(
            highlight(DropTarget::Center { pane: PaneId(9) }, &layout, None),
            None
        );
        assert_eq!(highlight(DropTarget::Tab(5), &layout, Some(&bar(2))), None);
        assert_eq!(highlight(DropTarget::NewTab, &layout, None), None);
    }
}
