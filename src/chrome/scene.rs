//! Interface of the window (pure, no GPU access): the tab bar, the dividers between the
//! panes of the shown tab and the border of its active pane, and the confirmation
//! dialog or the command palette over everything. Dividers and border are only drawn while the tab shows
//! several panes, the bar while there are several tabs.

use super::command_palette::PaletteLayout;
use super::dialog::DialogLayout;
use super::tab_bar::TabBarLayout;
use super::tooltip::TooltipLayout;
use crate::layout::Rect;
use crate::palette::UiColors;
use crate::renderer::scene::gpu_color;
use crate::renderer::{RectInstance, UiFont, UiLayer, UiTextItem};
use crate::terminal::PaneId;
use crate::workspace::tree::Layout;

/// Thickness of the active pane border, in physical pixels.
pub const ACTIVE_BORDER: f32 = 2.0;

/// Character of the tab close buttons.
const CLOSE: &str = "×";

/// Thickness of the strokes of the `+` button, relative to the bar height.
const PLUS_STROKE: f32 = 1.0 / 15.0;

/// Thickness of the dialog border, in physical pixels.
const DIALOG_BORDER: f32 = 1.0;

/// The box drawn over everything, laid out.
#[derive(Debug, Clone, Copy)]
pub enum Overlay<'a> {
    Dialog(&'a DialogLayout),
    Palette(&'a PaletteLayout),
}

/// The interface over the panes of `layout`: the pane rectangles (see [`panes`]), then
/// the tab bar when `tab_bar` is given, and the `overlay`.
pub fn build(
    layout: &Layout,
    active: Option<PaneId>,
    tab_bar: Option<&TabBarLayout>,
    overlay: Option<Overlay<'_>>,
    colors: &UiColors,
    srgb_target: bool,
) -> UiLayer {
    let mut ui = UiLayer {
        rects: panes(layout, active, colors, srgb_target),
        ..UiLayer::default()
    };
    if let Some(bar) = tab_bar {
        tabs(bar, colors, srgb_target, &mut ui);
    }
    match overlay {
        Some(Overlay::Dialog(layout)) => dialog(layout, colors, srgb_target, &mut ui),
        Some(Overlay::Palette(layout)) => palette(layout, colors, srgb_target, &mut ui),
        None => {}
    }
    ui
}

/// The command palette, in the overlay: its box and border, the search field and its
/// text cursor, the selected row, then the texts: the query or its hint, and each row's
/// label (matching characters in the accent color) and shortcut.
fn palette(palette: &PaletteLayout, colors: &UiColors, srgb_target: bool, ui: &mut UiLayer) {
    let color = |c| gpu_color(c, srgb_target);
    let text = color(colors.palette_text);
    let dim = color(colors.tab_inactive_text);
    ui.overlay_rects
        .push(rect(palette.rect, color(colors.palette_background)));
    ui.overlay_rects.extend(border(
        palette.rect,
        DIALOG_BORDER,
        color(colors.pane_border),
    ));
    ui.overlay_rects
        .push(rect(palette.input, color(colors.tab_bar_background)));
    ui.overlay_rects.push(rect(palette.caret, text));
    if let Some(row) = palette.rows.iter().find(|row| row.selected) {
        ui.overlay_rects
            .push(rect(row.rect, color(colors.tab_active_background)));
    }
    ui.overlay_texts.push(UiTextItem {
        text: palette.input_text.clone(),
        x: palette.text_x,
        y: palette.input_text_y,
        color: if palette.placeholder { dim } else { text },
        max_width: palette.text_width,
        font: UiFont::Palette,
    });
    let matching = color(colors.palette_match);
    for row in &palette.rows {
        for run in &row.runs {
            ui.overlay_texts.push(UiTextItem {
                text: run.text.clone(),
                x: run.x,
                y: row.text_y,
                color: if run.highlighted { matching } else { text },
                max_width: run.max_width,
                font: UiFont::Palette,
            });
        }
        if let Some((shortcut, x)) = &row.shortcut {
            ui.overlay_texts.push(UiTextItem {
                text: shortcut.clone(),
                x: *x,
                y: row.text_y,
                color: dim,
                max_width: row.rect.x + row.rect.width - x,
                font: UiFont::Palette,
            });
        }
    }
}

/// The dialog, in the overlay: its box and border, its title and message, then its
/// buttons, the default one in the accent color.
fn dialog(dialog: &DialogLayout, colors: &UiColors, srgb_target: bool, ui: &mut UiLayer) {
    let color = |c| gpu_color(c, srgb_target);
    let (background, text) = (color(colors.palette_background), color(colors.palette_text));
    ui.overlay_rects.push(rect(dialog.rect, background));
    ui.overlay_rects.extend(border(
        dialog.rect,
        DIALOG_BORDER,
        color(colors.pane_border),
    ));
    for (line, y) in [
        (&dialog.title, dialog.title_y),
        (&dialog.message, dialog.message_y),
    ] {
        ui.overlay_texts.push(UiTextItem {
            text: line.clone(),
            x: dialog.text_x,
            y,
            color: text,
            max_width: dialog.text_width,
            font: UiFont::Chrome,
        });
    }
    for button in &dialog.buttons {
        let (fill, label) = if button.default {
            (color(colors.palette_match), background)
        } else {
            (color(colors.tab_bar_background), text)
        };
        ui.overlay_rects.push(rect(button.rect, fill));
        ui.overlay_texts.push(UiTextItem {
            text: button.label.clone(),
            x: button.label_x,
            y: dialog.label_y,
            color: label,
            max_width: button.rect.width,
            font: UiFont::Chrome,
        });
    }
}

/// The link tooltip, last in the overlay so it shows over the dialog or palette: its
/// box and border, then the URI.
pub fn tooltip(tooltip: &TooltipLayout, colors: &UiColors, srgb_target: bool, ui: &mut UiLayer) {
    let color = |c| gpu_color(c, srgb_target);
    ui.overlay_rects
        .push(rect(tooltip.rect, color(colors.palette_background)));
    ui.overlay_rects.extend(border(
        tooltip.rect,
        DIALOG_BORDER,
        color(colors.pane_border),
    ));
    ui.overlay_texts.push(UiTextItem {
        text: tooltip.text.clone(),
        x: tooltip.text_x,
        y: tooltip.text_y,
        color: color(colors.palette_text),
        max_width: tooltip.text_width,
        font: UiFont::Chrome,
    });
}

/// The tab bar: its background, the shown tab's background, each title and close
/// button, then the `+` button.
fn tabs(bar: &TabBarLayout, colors: &UiColors, srgb_target: bool, ui: &mut UiLayer) {
    let color = |c| gpu_color(c, srgb_target);
    ui.rects
        .push(rect(bar.rect, color(colors.tab_bar_background)));
    for tab in &bar.tabs {
        let text = if tab.active {
            ui.rects
                .push(rect(tab.rect, color(colors.tab_active_background)));
            color(colors.tab_active_text)
        } else {
            color(colors.tab_inactive_text)
        };
        ui.texts.push(UiTextItem {
            text: tab.title.clone(),
            x: tab.title_x,
            y: bar.text_y,
            color: text,
            max_width: tab.title_width,
            font: UiFont::Chrome,
        });
        // The glyph is about half the line height wide: centered in its square.
        ui.texts.push(UiTextItem {
            text: CLOSE.to_owned(),
            x: (tab.close.x + tab.close.width / 4.0).round(),
            y: bar.text_y,
            color: text,
            max_width: tab.close.width,
            font: UiFont::Chrome,
        });
    }
    let plus = bar.new_tab;
    let stroke = (plus.height * PLUS_STROKE).round().max(1.0);
    let arm = (plus.height / 3.0).round();
    let (cx, cy) = (
        (plus.x + plus.width / 2.0).round(),
        (plus.y + plus.height / 2.0).round(),
    );
    let text = color(colors.tab_inactive_text);
    ui.rects.push(RectInstance::new(
        [cx - arm / 2.0, cy - stroke / 2.0],
        [arm, stroke],
        text,
    ));
    ui.rects.push(RectInstance::new(
        [cx - stroke / 2.0, cy - arm / 2.0],
        [stroke, arm],
        text,
    ));
}

/// Rectangles drawn over the panes of `layout`: its dividers, in the pane border color,
/// then the border of the `active` pane, inside its rectangle, in the accent color
/// (the one highlighting command palette matches).
pub fn panes(
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
    use crate::chrome::tab_bar::UiMetrics;
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
        assert!(panes(&layout, Some(PaneId(1)), &colors(), false).is_empty());
        assert_eq!(
            build(&layout, Some(PaneId(1)), None, None, &colors(), false),
            UiLayer::default()
        );
    }

    #[test]
    fn several_panes_draw_the_dividers_then_the_active_border() {
        let rects = panes(&two_columns(), Some(PaneId(2)), &colors(), false);
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
        let rects = panes(&two_columns(), Some(PaneId(9)), &colors(), false);
        assert_eq!(rects.len(), 1);
        assert!(panes(&two_columns(), None, &colors(), false).len() == 1);
    }

    #[test]
    fn colors_follow_the_target_encoding() {
        let rects = panes(&two_columns(), Some(PaneId(1)), &colors(), true);
        assert_eq!(rects[0].color, gpu_color(BORDER, true));
        assert_eq!(rects[1].color, gpu_color(ACCENT, true));
    }

    const BAR: Rgb = Rgb { r: 10, g: 0, b: 0 };
    const ACTIVE_TAB: Rgb = Rgb { r: 20, g: 0, b: 0 };
    const ACTIVE_TEXT: Rgb = Rgb { r: 30, g: 0, b: 0 };
    const INACTIVE_TEXT: Rgb = Rgb { r: 40, g: 0, b: 0 };

    fn tab_colors() -> UiColors {
        UiColors {
            tab_bar_background: BAR,
            tab_active_background: ACTIVE_TAB,
            tab_active_text: ACTIVE_TEXT,
            tab_inactive_text: INACTIVE_TEXT,
            ..colors()
        }
    }

    fn bar() -> TabBarLayout {
        let metrics = UiMetrics {
            line_height: 18.0,
            scale: 1.0,
        };
        let titles = ["one".to_owned(), "two".to_owned()];
        TabBarLayout::new(1000.0, &titles, 1, metrics)
    }

    #[test]
    fn tab_bar_draws_its_background_the_shown_tab_and_the_titles() {
        let single = Layout {
            panes: vec![(PaneId(1), area(0.0, 30.0, 1000.0, 100.0))],
            dividers: Vec::new(),
            divider: 1.0,
        };
        let bar = bar();
        let ui = build(
            &single,
            Some(PaneId(1)),
            Some(&bar),
            None,
            &tab_colors(),
            false,
        );
        let color = |c| gpu_color(c, false);
        assert_eq!(ui.rects[0].position, [0.0, 0.0]);
        assert_eq!(ui.rects[0].size, [1000.0, bar.rect.height]);
        assert_eq!(ui.rects[0].color, color(BAR));
        // Only the shown tab (the second) gets a background.
        assert_eq!(ui.rects[1].position, [bar.tabs[1].rect.x, 0.0]);
        assert_eq!(ui.rects[1].color, color(ACTIVE_TAB));
        // Then the two strokes of the `+` button, inside it.
        assert_eq!(ui.rects.len(), 4);
        assert!(
            ui.rects[2..]
                .iter()
                .all(|r| bar.new_tab.contains(r.position[0], r.position[1]))
        );

        let titles: Vec<(&str, [f32; 4])> = ui
            .texts
            .iter()
            .map(|t| (t.text.as_str(), t.color))
            .collect();
        assert_eq!(
            titles,
            [
                ("one", color(INACTIVE_TEXT)),
                (CLOSE, color(INACTIVE_TEXT)),
                ("two", color(ACTIVE_TEXT)),
                (CLOSE, color(ACTIVE_TEXT)),
            ]
        );
        assert_eq!(ui.texts[2].x, bar.tabs[1].title_x);
        assert_eq!(ui.texts[2].max_width, bar.tabs[1].title_width);
        assert!(ui.texts.iter().all(|t| t.y == bar.text_y));
    }

    #[test]
    fn tab_bar_comes_after_the_pane_rects() {
        let bar = bar();
        let ui = build(
            &two_columns(),
            Some(PaneId(1)),
            Some(&bar),
            None,
            &tab_colors(),
            false,
        );
        assert_eq!(ui.rects[0].color, gpu_color(BORDER, false));
        assert_eq!(ui.rects[5].color, gpu_color(BAR, false));
        assert!(ui.overlay_rects.is_empty() && ui.overlay_texts.is_empty());
    }

    const DIALOG_BACKGROUND: Rgb = Rgb { r: 50, g: 0, b: 0 };
    const DIALOG_TEXT: Rgb = Rgb { r: 60, g: 0, b: 0 };

    #[test]
    fn dialog_is_drawn_in_the_overlay() {
        use crate::chrome::dialog::{ConfirmClose, Target};
        use crate::i18n::Lang;
        let colors = UiColors {
            palette_background: DIALOG_BACKGROUND,
            palette_text: DIALOG_TEXT,
            ..tab_colors()
        };
        let confirm = ConfirmClose {
            target: Target::Window,
            programs: vec!["vim".to_owned()],
        };
        let metrics = UiMetrics {
            line_height: 18.0,
            scale: 1.0,
        };
        let dialog = DialogLayout::new(
            &confirm,
            Lang::En,
            area(0.0, 0.0, 1000.0, 600.0),
            metrics,
            |t| t.chars().count() as f32 * 8.0,
        );
        let bar = bar();
        let ui = build(
            &two_columns(),
            Some(PaneId(1)),
            Some(&bar),
            Some(Overlay::Dialog(&dialog)),
            &colors,
            false,
        );
        let color = |c| gpu_color(c, false);
        // The interface is unchanged: the dialog only adds to the overlay.
        let without = build(
            &two_columns(),
            Some(PaneId(1)),
            Some(&bar),
            None,
            &colors,
            false,
        );
        assert_eq!((&ui.rects, &ui.texts), (&without.rects, &without.texts));
        // Box, its 4 border sides, then the two buttons.
        assert_eq!(ui.overlay_rects.len(), 7);
        assert_eq!(ui.overlay_rects[0].position, [dialog.rect.x, dialog.rect.y]);
        assert_eq!(ui.overlay_rects[0].color, color(DIALOG_BACKGROUND));
        assert!(
            ui.overlay_rects[1..5]
                .iter()
                .all(|r| r.color == color(BORDER))
        );
        assert_eq!(ui.overlay_rects[5].color, color(BAR));
        assert_eq!(ui.overlay_rects[6].color, color(ACCENT));
        let texts: Vec<(&str, [f32; 4])> = ui
            .overlay_texts
            .iter()
            .map(|t| (t.text.as_str(), t.color))
            .collect();
        assert_eq!(
            texts,
            [
                ("Close the window?", color(DIALOG_TEXT)),
                ("vim is still running", color(DIALOG_TEXT)),
                ("Cancel", color(DIALOG_TEXT)),
                ("Close", color(DIALOG_BACKGROUND)),
            ]
        );
    }

    #[test]
    fn palette_is_drawn_in_the_overlay() {
        use crate::chrome::command_palette::{Command, CommandPalette, Entry, PaletteLayout};
        use crate::i18n::Lang;
        use crate::input::Action;
        use winit::keyboard::Key;
        let colors = UiColors {
            palette_background: DIALOG_BACKGROUND,
            palette_text: DIALOG_TEXT,
            ..tab_colors()
        };
        let entry = |label: &str, action, shortcut| Entry {
            command: Command::Action(action),
            label: label.to_owned(),
            shortcut,
        };
        let mut palette = CommandPalette::new(
            vec![
                entry("Split right", Action::SplitRight, Some("Ctrl+Shift+E")),
                entry("Split down", Action::SplitDown, None),
            ],
            Vec::new(),
        );
        palette.key(&Key::Character("s".into()), Some("s"));
        let metrics = UiMetrics {
            line_height: 18.0,
            scale: 1.0,
        };
        let layout = PaletteLayout::new(
            &palette,
            Lang::En,
            area(0.0, 0.0, 1000.0, 600.0),
            metrics,
            |t| t.chars().count() as f32 * 8.0,
        );
        let ui = build(
            &two_columns(),
            Some(PaneId(1)),
            None,
            Some(Overlay::Palette(&layout)),
            &colors,
            false,
        );
        let color = |c| gpu_color(c, false);
        // Box, its 4 border sides, the search field, the text cursor, the selected row.
        assert_eq!(ui.overlay_rects.len(), 8);
        assert_eq!(ui.overlay_rects[0].color, color(DIALOG_BACKGROUND));
        assert_eq!(ui.overlay_rects[5].color, color(BAR));
        assert_eq!(ui.overlay_rects[6].color, color(DIALOG_TEXT));
        assert_eq!(ui.overlay_rects[7].color, color(ACTIVE_TAB));
        assert_eq!(
            ui.overlay_rects[7].position,
            [layout.rows[0].rect.x, layout.rows[0].rect.y]
        );
        let texts: Vec<(&str, [f32; 4])> = ui
            .overlay_texts
            .iter()
            .map(|t| (t.text.as_str(), t.color))
            .collect();
        assert_eq!(
            texts,
            [
                ("s", color(DIALOG_TEXT)),
                ("S", color(ACCENT)),
                ("plit right", color(DIALOG_TEXT)),
                ("Ctrl+Shift+E", color(INACTIVE_TEXT)),
                ("S", color(ACCENT)),
                ("plit down", color(DIALOG_TEXT)),
            ]
        );
    }

    #[test]
    fn empty_palette_query_shows_its_hint_dimmed() {
        use crate::chrome::command_palette::{CommandPalette, PaletteLayout};
        use crate::i18n::Lang;
        let palette = CommandPalette::new(Vec::new(), Vec::new());
        let layout = PaletteLayout::new(
            &palette,
            Lang::En,
            area(0.0, 0.0, 1000.0, 600.0),
            UiMetrics {
                line_height: 18.0,
                scale: 1.0,
            },
            |t| t.chars().count() as f32 * 8.0,
        );
        let ui = build(
            &two_columns(),
            Some(PaneId(1)),
            None,
            Some(Overlay::Palette(&layout)),
            &tab_colors(),
            false,
        );
        assert_eq!(ui.overlay_texts.len(), 1);
        assert_eq!(ui.overlay_texts[0].text, "Type a command, a tab or a pane");
        assert_eq!(ui.overlay_texts[0].color, gpu_color(INACTIVE_TEXT, false));
        // No row, so no selected row: box, border, field and cursor.
        assert_eq!(ui.overlay_rects.len(), 7);
    }

    #[test]
    fn tooltip_is_drawn_last_in_the_overlay() {
        let colors = UiColors {
            palette_background: DIALOG_BACKGROUND,
            palette_text: DIALOG_TEXT,
            ..colors()
        };
        let metrics = UiMetrics {
            line_height: 18.0,
            scale: 1.0,
        };
        let layout = TooltipLayout::new(
            "https://example.com",
            (10.0, 10.0),
            area(0.0, 0.0, 1000.0, 600.0),
            metrics,
            |t| t.chars().count() as f32 * 8.0,
        )
        .unwrap();
        let mut ui = build(&two_columns(), Some(PaneId(1)), None, None, &colors, false);
        let before = ui.clone();
        tooltip(&layout, &colors, false, &mut ui);
        assert_eq!((&ui.rects, &ui.texts), (&before.rects, &before.texts));
        // Box then its border, then the URI.
        assert_eq!(ui.overlay_rects.len(), 5);
        assert_eq!(ui.overlay_rects[0].position, [layout.rect.x, layout.rect.y]);
        assert_eq!(
            ui.overlay_rects[0].color,
            gpu_color(DIALOG_BACKGROUND, false)
        );
        assert!(
            ui.overlay_rects[1..]
                .iter()
                .all(|r| r.color == gpu_color(BORDER, false))
        );
        assert_eq!(
            ui.overlay_texts,
            vec![UiTextItem {
                text: "https://example.com".to_owned(),
                x: layout.text_x,
                y: layout.text_y,
                color: gpu_color(DIALOG_TEXT, false),
                max_width: layout.text_width,
                font: UiFont::Chrome,
            }]
        );
    }
}
