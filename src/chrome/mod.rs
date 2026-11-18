//! Interface drawn around and between the panes, laid out without GPU access.

pub mod command_palette;
#[allow(dead_code)] // Runtime integration is tracked by issue #103.
pub mod context_menu;
pub mod dialog;
pub mod pane_bar;
pub mod scene;
pub mod tab_bar;
pub mod tooltip;

use crate::layout::Rect;
use crate::terminal::PaneId;
use tab_bar::{TabBarHit, TabBarLayout, UiMetrics};

/// A close button of the chrome: the one of a tab, by index, or of a pane bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseButton {
    Tab(usize),
    Pane(PaneId),
}

/// The close button under the pixel `(x, y)`: in the tab bar `tab_bar`, then in the
/// bars of the panes `panes` (pane and rectangle, as a split layout gives them).
pub fn close_button_at(
    tab_bar: Option<&TabBarLayout>,
    panes: &[(PaneId, Rect)],
    x: f32,
    y: f32,
    metrics: UiMetrics,
) -> Option<CloseButton> {
    if let Some(TabBarHit::Close(index)) = tab_bar.and_then(|bar| bar.hit(x, y)) {
        return Some(CloseButton::Tab(index));
    }
    match pane_bar::hit_at(panes, x, y, metrics) {
        Some((pane, pane_bar::PaneBarHit::Close)) => Some(CloseButton::Pane(pane)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::TabBarPosition;
    use pane_bar::PaneBarLayout;

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

    fn tab_bar() -> TabBarLayout {
        let titles = ["one".to_owned(), "two".to_owned()];
        let window = rect(0.0, 0.0, 1000.0, 600.0);
        TabBarLayout::new(window, TabBarPosition::Top, &titles, 0, METRICS)
    }

    fn panes(bar: &TabBarLayout) -> Vec<(PaneId, Rect)> {
        let top = bar.rect.height;
        vec![
            (PaneId(1), rect(0.0, top, 500.0, 300.0)),
            (PaneId(2), rect(501.0, top, 499.0, 300.0)),
        ]
    }

    #[test]
    fn finds_the_close_button_of_a_tab() {
        let bar = tab_bar();
        let close = bar.tabs[1].close;
        assert_eq!(
            close_button_at(
                Some(&bar),
                &panes(&bar),
                close.x + 1.0,
                close.y + 1.0,
                METRICS
            ),
            Some(CloseButton::Tab(1))
        );
    }

    #[test]
    fn finds_the_close_button_of_a_pane_bar() {
        let bar = tab_bar();
        let panes = panes(&bar);
        let close = PaneBarLayout::new(PaneId(2), panes[1].1, String::new(), false, METRICS).close;
        let (x, y) = (close.x + 1.0, close.y + 1.0);
        assert_eq!(
            close_button_at(Some(&bar), &panes, x, y, METRICS),
            Some(CloseButton::Pane(PaneId(2)))
        );
        assert_eq!(
            close_button_at(None, &panes, x, y, METRICS),
            Some(CloseButton::Pane(PaneId(2)))
        );
    }

    #[test]
    fn nothing_off_the_close_buttons() {
        let bar = tab_bar();
        let panes = panes(&bar);
        let tab = bar.tabs[0].rect;
        let title = PaneBarLayout::new(PaneId(1), panes[0].1, String::new(), false, METRICS);
        for (x, y) in [
            (tab.x + 1.0, tab.y + 1.0),
            (title.title_x, title.rect.y + 1.0),
            (250.0, 400.0),
        ] {
            assert_eq!(close_button_at(Some(&bar), &panes, x, y, METRICS), None);
        }
    }
}
