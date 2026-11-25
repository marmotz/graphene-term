//! Interface drawn around and between the panes, laid out without GPU access.

pub mod badge;
pub mod command_palette;
#[allow(dead_code)] // Runtime integration is tracked by issue #103.
pub mod context_menu;
pub mod dialog;
pub mod pane_bar;
pub mod rename_tab;
pub mod scene;
pub mod tab_bar;
pub mod text_field;
pub mod tooltip;

use crate::layout::Rect;
use crate::terminal::PaneId;
use tab_bar::{TabBarHit, TabBarLayout, UiMetrics};

/// A button of the chrome drawn hovered under the mouse: the settings tab, the close
/// button of a tab, by index, or of a pane bar, or the `+` button of the tab bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeButton {
    Settings,
    TabClose(usize),
    PaneClose(PaneId),
    NewTab,
}

/// The button under the pixel `(x, y)`: in the tab bar `tab_bar`, then in the bars of
/// the panes `panes` (pane and rectangle, as a split layout gives them).
pub fn button_at(
    tab_bar: Option<&TabBarLayout>,
    panes: &[(PaneId, Rect)],
    x: f32,
    y: f32,
    metrics: UiMetrics,
) -> Option<ChromeButton> {
    match tab_bar.and_then(|bar| bar.hit(x, y)) {
        Some(TabBarHit::Close(index)) => return Some(ChromeButton::TabClose(index)),
        Some(TabBarHit::NewTab) => return Some(ChromeButton::NewTab),
        Some(TabBarHit::Settings) => return Some(ChromeButton::Settings),
        _ => {}
    }
    match pane_bar::hit_at(panes, x, y, metrics) {
        Some((pane, pane_bar::PaneBarHit::Close)) => Some(ChromeButton::PaneClose(pane)),
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
            button_at(
                Some(&bar),
                &panes(&bar),
                close.x + 1.0,
                close.y + 1.0,
                METRICS
            ),
            Some(ChromeButton::TabClose(1))
        );
    }

    #[test]
    fn finds_the_close_button_of_a_pane_bar() {
        let bar = tab_bar();
        let panes = panes(&bar);
        let close = PaneBarLayout::new(PaneId(2), panes[1].1, String::new(), false, METRICS).close;
        let (x, y) = (close.x + 1.0, close.y + 1.0);
        assert_eq!(
            button_at(Some(&bar), &panes, x, y, METRICS),
            Some(ChromeButton::PaneClose(PaneId(2)))
        );
        assert_eq!(
            button_at(None, &panes, x, y, METRICS),
            Some(ChromeButton::PaneClose(PaneId(2)))
        );
    }

    #[test]
    fn finds_the_new_tab_button() {
        let bar = tab_bar();
        let plus = bar.new_tab;
        assert_eq!(
            button_at(
                Some(&bar),
                &panes(&bar),
                plus.x + 1.0,
                plus.y + 1.0,
                METRICS
            ),
            Some(ChromeButton::NewTab)
        );
    }

    #[test]
    fn nothing_off_the_buttons() {
        let bar = tab_bar();
        let panes = panes(&bar);
        let tab = bar.tabs[0].rect;
        let title = PaneBarLayout::new(PaneId(1), panes[0].1, String::new(), false, METRICS);
        for (x, y) in [
            (tab.x + 1.0, tab.y + 1.0),
            (title.title_x, title.rect.y + 1.0),
            (250.0, 400.0),
        ] {
            assert_eq!(button_at(Some(&bar), &panes, x, y, METRICS), None);
        }
    }
}
