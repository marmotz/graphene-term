//! Interface of the window (pure, no GPU access): the title bar of every pane, the tab
//! bar, the dividers between the panes of the shown tab and the border of its active
//! pane, and the confirmation dialog or the command palette over everything. Dividers
//! and border are only drawn while the tab shows several panes, the tab bar while there
//! are several tabs.

use super::badge::Pill;
use super::command_palette::PaletteLayout;
use super::context_menu::{MenuLayout, MenuRow};
use super::dialog::DialogLayout;
use super::pane_bar::PaneBarLayout;
use super::rename_tab::RenameTabLayout;
use super::tab_bar::TabBarLayout;
use super::tooltip::TooltipLayout;
use crate::easter_eggs::art::ArtLayout;
use crate::layout::Rect;
use crate::palette::UiColors;
use crate::progress::{ProgressBar, ProgressColors};
use crate::renderer::progress_bar::progress_rects;
use crate::renderer::scene::gpu_color;
use crate::renderer::{RectInstance, UiFont, UiLayer, UiTextItem};
use crate::terminal::PaneId;
use crate::workspace::tree::Layout;
use alacritty_terminal::vte::ansi::Rgb;

/// Thickness of the active pane border, in physical pixels.
pub const ACTIVE_BORDER: f32 = 2.0;

/// Opacity of the inactive tab text color in the dividers, to keep them discreet yet
/// easy to find with the mouse. That color stands out from the background even through
/// a translucent window, where the pane border color blends into it.
pub const DIVIDER_OPACITY: f32 = 0.35;

/// Share of the accent color in the background of an inactive pane bar, the rest being
/// the tab bar background: a muted version of the active bar's hue.
pub const INACTIVE_PANE_BAR: f32 = 0.15;

/// Same share for an inactive pane bar in the broadcast set: stronger, so that the
/// panes receiving the input stand out at a glance.
pub const INACTIVE_BROADCAST_BAR: f32 = 0.3;

/// Opacity of the accent rectangle showing where a dragged pane would land.
pub const DROP_HIGHLIGHT: f32 = 0.3;

/// Character of the tab close buttons.
const CLOSE: &str = "×";

/// Thickness of the strokes of the `+` button, relative to the bar height.
const PLUS_STROKE: f32 = 1.0 / 15.0;

/// Thickness of the dialog border, in physical pixels.
const DIALOG_BORDER: f32 = 1.0;

/// Opacity of the text color in the background of a hovered button.
pub const BUTTON_HOVER: f32 = 0.2;

/// Opacity of the hovered context-menu row, matching the shortcuts table in settings.
const MENU_HOVER_HIGHLIGHT: f32 = 0.45;

/// The box drawn over everything, laid out.
#[derive(Debug, Clone, Copy)]
pub enum Overlay<'a> {
    Dialog(&'a DialogLayout),
    Palette(&'a PaletteLayout),
    RenameTab(&'a RenameTabLayout),
    #[allow(dead_code)] // Instantiated when menu opening is wired in issue #103.
    Menu(&'a MenuLayout),
    Art(&'a ArtLayout),
}

/// The interface over the panes of `layout`: the pane chrome (see [`panes`]), then the
/// tab bar when `tab_bar` is given, and the `overlay`.
pub fn build(
    layout: &Layout,
    active: Option<PaneId>,
    pane_bars: &[PaneBarLayout],
    tab_bar: Option<&TabBarLayout>,
    overlay: Option<Overlay<'_>>,
    colors: &UiColors,
    srgb_target: bool,
) -> UiLayer {
    let mut ui = UiLayer::default();
    panes(layout, active, pane_bars, colors, srgb_target, &mut ui);
    if let Some(bar) = tab_bar {
        tabs(bar, colors, srgb_target, &mut ui);
    }
    match overlay {
        Some(Overlay::Dialog(layout)) => dialog(layout, colors, srgb_target, &mut ui),
        Some(Overlay::Palette(layout)) => palette(layout, colors, srgb_target, &mut ui),
        Some(Overlay::RenameTab(layout)) => rename_tab(layout, colors, srgb_target, &mut ui),
        Some(Overlay::Menu(layout)) => menu(layout, colors, srgb_target, &mut ui),
        Some(Overlay::Art(layout)) => art(layout, colors, srgb_target, &mut ui),
        None => {}
    }
    ui
}

/// A context menu: its box and border, hovered rows, separators, labels and shortcuts.
fn menu(layout: &MenuLayout, colors: &UiColors, srgb_target: bool, ui: &mut UiLayer) {
    let color = |c| gpu_color(c, srgb_target);
    let background = color(colors.palette_background);
    let border_color = color(colors.pane_border);
    ui.overlay_rects.push(rect(layout.rect, background));
    ui.overlay_rects
        .extend(border(layout.rect, DIALOG_BORDER, border_color));
    draw_menu_rows(&layout.rows, colors, srgb_target, ui);
    for submenu in &layout.submenus {
        ui.overlay_rects.push(rect(submenu.rect, background));
        ui.overlay_rects
            .extend(border(submenu.rect, DIALOG_BORDER, border_color));
        draw_menu_rows(&submenu.rows, colors, srgb_target, ui);
    }
}

fn draw_menu_rows(rows: &[MenuRow], colors: &UiColors, srgb_target: bool, ui: &mut UiLayer) {
    let color = |c| gpu_color(c, srgb_target);
    for row in rows {
        if row.separator {
            let line = Rect {
                x: row.rect.x + 1.0,
                y: row.rect.y + row.rect.height / 2.0,
                width: (row.rect.width - 2.0).max(0.0),
                height: 1.0,
            };
            ui.overlay_rects.push(rect(line, color(colors.pane_border)));
            continue;
        }
        if row.hovered {
            ui.overlay_rects.push(rect(
                row.rect,
                color(blend(
                    colors.palette_background,
                    colors.palette_selection_background,
                    MENU_HOVER_HIGHLIGHT,
                )),
            ));
        }
        if let Some(label) = &row.label {
            ui.overlay_texts.push(UiTextItem {
                text: label.clone(),
                x: row.text_x,
                y: row.text_y,
                color: if row.active_theme {
                    color(colors.palette_match)
                } else if row.enabled {
                    color(colors.palette_text)
                } else {
                    color(colors.tab_inactive_text)
                },
                max_width: row.max_width,
                font: UiFont::Palette,
            });
        }
        if let Some((shortcut, x)) = &row.shortcut {
            ui.overlay_texts.push(UiTextItem {
                text: shortcut.clone(),
                x: *x,
                y: row.text_y,
                color: color(colors.tab_inactive_text),
                max_width: (row.rect.x + row.rect.width - x).max(0.0),
                font: UiFont::Palette,
            });
        }
        if row.submenu_arrow {
            ui.overlay_texts.push(UiTextItem {
                text: "›".to_owned(),
                x: row.rect.x + row.rect.width - 14.0,
                y: row.text_y,
                color: color(colors.tab_inactive_text),
                max_width: 12.0,
                font: UiFont::Palette,
            });
        }
    }
}

/// Background and text colors of a pane bar. The active pane's bar is filled with the
/// accent color and labelled in the palette background, like the default dialog
/// button; an inactive one mixes a little accent into the tab bar background and is
/// labelled in the shown tab's text color. Built-in themes keep both readable.
/// A bar of a pane in the broadcast set uses `pane_broadcast` instead of the accent.
pub fn pane_bar_colors(colors: &UiColors, active: bool, broadcast: bool) -> (Rgb, Rgb) {
    let accent = if broadcast {
        colors.pane_broadcast
    } else {
        colors.palette_match
    };
    if active {
        (accent, colors.palette_background)
    } else {
        let share = if broadcast {
            INACTIVE_BROADCAST_BAR
        } else {
            INACTIVE_PANE_BAR
        };
        let background = blend(colors.tab_bar_background, accent, share);
        if broadcast {
            (background, readable_on(colors.tab_active_text, background))
        } else {
            (background, colors.tab_active_text)
        }
    }
}

/// Minimum WCAG contrast ratio for the text of a pane bar.
const BAR_TEXT_CONTRAST: f64 = 4.5;

/// `text`, pushed toward white on a dark `background` or toward black on a light one
/// until it reaches [`BAR_TEXT_CONTRAST`] (or the extreme): a strongly tinted bar must
/// stay readable whatever the theme's own text color is.
fn readable_on(text: Rgb, background: Rgb) -> Rgb {
    let target = if luminance(background) < 0.18 { 255 } else { 0 };
    let target = Rgb {
        r: target,
        g: target,
        b: target,
    };
    (0..=10)
        .map(|step| blend(text, target, step as f32 / 10.0))
        .find(|candidate| contrast_ratio(*candidate, background) >= BAR_TEXT_CONTRAST)
        .unwrap_or(target)
}

fn luminance(color: Rgb) -> f64 {
    let channel = |value: u8| {
        let c = f64::from(value) / 255.0;
        if c <= 0.039_28 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Glyph announcing the broadcast set, followed by its size, before the title.
const BROADCAST_GLYPH: &str = "»";

/// The text of a pane bar: the broadcast badge, when the pane is in the set, then the title.
pub fn pane_bar_text(bar: &PaneBarLayout) -> String {
    match bar.broadcast {
        Some(count) => format!("{BROADCAST_GLYPH}{count} {}", bar.title),
        None => bar.title.clone(),
    }
}

fn blend(background: Rgb, highlight: Rgb, opacity: f32) -> Rgb {
    let channel = |background: u8, highlight: u8| {
        (f32::from(background) * (1.0 - opacity) + f32::from(highlight) * opacity).round() as u8
    };
    Rgb {
        r: channel(background.r, highlight.r),
        g: channel(background.g, highlight.g),
        b: channel(background.b, highlight.b),
    }
}

/// The command palette, in the overlay: its box and border, the search field and its
/// text cursor, the selected row and its accent bar, then the texts: the query or its hint, and each row's
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
            .push(rect(row.rect, color(colors.palette_selection_background)));
    }
    let matching = color(colors.palette_match);
    if let Some(marker) = palette.marker {
        ui.overlay_rects.push(rect(marker, matching));
    }
    ui.overlay_texts.push(UiTextItem {
        text: palette.input_text.clone(),
        x: palette.text_x,
        y: palette.input_text_y,
        color: if palette.placeholder { dim } else { text },
        max_width: palette.text_width,
        font: UiFont::Palette,
    });
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

/// The tab rename box, in the overlay, drawn like the palette's search field: its box
/// and border, its heading, the field, the highlight of a selected text, the text
/// cursor and the text.
fn rename_tab(rename: &RenameTabLayout, colors: &UiColors, srgb_target: bool, ui: &mut UiLayer) {
    let color = |c| gpu_color(c, srgb_target);
    let text = color(colors.palette_text);
    ui.overlay_rects
        .push(rect(rename.rect, color(colors.palette_background)));
    ui.overlay_rects.extend(border(
        rename.rect,
        DIALOG_BORDER,
        color(colors.pane_border),
    ));
    ui.overlay_rects
        .push(rect(rename.input, color(colors.tab_bar_background)));
    if let Some(selection) = rename.selection {
        ui.overlay_rects
            .push(rect(selection, color(colors.palette_selection_background)));
    }
    ui.overlay_rects.push(rect(rename.caret, text));
    ui.overlay_texts.push(UiTextItem {
        text: rename.heading.clone(),
        x: rename.heading_x,
        y: rename.heading_y,
        color: color(colors.tab_inactive_text),
        max_width: rename.text_width,
        font: UiFont::Palette,
    });
    ui.overlay_texts.push(UiTextItem {
        text: rename.input_text.clone(),
        x: rename.text_x,
        y: rename.input_text_y,
        color: text,
        max_width: rename.text_width,
        font: UiFont::Palette,
    });
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

/// The ASCII drawing, in the overlay: its box and border, then each line in the
/// fixed-width interface font.
fn art(art: &ArtLayout, colors: &UiColors, srgb_target: bool, ui: &mut UiLayer) {
    let color = |c| gpu_color(c, srgb_target);
    ui.overlay_rects
        .push(rect(art.rect, color(colors.palette_background)));
    ui.overlay_rects
        .extend(border(art.rect, DIALOG_BORDER, color(colors.pane_border)));
    let text = color(colors.palette_text);
    ui.overlay_texts
        .extend(art.lines.iter().map(|line| UiTextItem {
            text: line.text.to_owned(),
            x: art.text_x,
            y: line.y,
            color: text,
            max_width: art.text_width,
            font: UiFont::Mono,
        }));
}

/// The translucent accent rectangle `area` showing where a dragged pane would land:
/// pushed after [`build`], it covers the pane chrome and the tab bar, under the overlay.
pub fn drop_highlight(area: Rect, colors: &UiColors, srgb_target: bool, ui: &mut UiLayer) {
    let mut color = gpu_color(colors.palette_match, srgb_target);
    color[3] = DROP_HIGHLIGHT;
    ui.rects.push(rect(area, color));
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

/// The tab bar: its background, the settings tab, the shown tab's background, each
/// title and close button, then the `+` button.
fn tabs(bar: &TabBarLayout, colors: &UiColors, srgb_target: bool, ui: &mut UiLayer) {
    let color = |c| gpu_color(c, srgb_target);
    ui.rects
        .push(rect(bar.rect, color(colors.tab_bar_background)));
    settings_tab(bar, colors, srgb_target, ui);
    for tab in &bar.tabs {
        let text = if tab.active {
            ui.rects
                .push(rect(tab.rect, color(colors.tab_active_background)));
            color(colors.tab_active_text)
        } else {
            color(colors.tab_inactive_text)
        };
        if tab.close_hovered {
            button_hover(tab.close, text, ui);
        }
        if let Some(pill) = &tab.pill {
            let (fill, label) = badge_colors(colors, colors.palette_match);
            badge(pill, fill, label, srgb_target, ui);
        }
        if let Some(bar) = &tab.progress {
            progress_bar(bar, bar.colors, srgb_target, ui);
        }
        ui.texts.push(UiTextItem {
            text: tab.title.clone(),
            x: tab.title_x,
            y: tab.title_y,
            color: text,
            max_width: tab.title_width,
            font: UiFont::Chrome,
        });
        // The glyph is about half the line height wide: centered in its square.
        ui.texts.push(UiTextItem {
            text: CLOSE.to_owned(),
            x: (tab.close.x + tab.close.width / 4.0).round(),
            y: tab.title_y,
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
    if bar.new_tab_hovered {
        button_hover(plus, text, ui);
    }
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

/// The settings tab, first in the bar: a sliders icon, three lines each crossed by a
/// knob, on the shown tab's background when it is the one shown.
fn settings_tab(bar: &TabBarLayout, colors: &UiColors, srgb_target: bool, ui: &mut UiLayer) {
    let color = |c| gpu_color(c, srgb_target);
    let slot = bar.settings;
    let text = if bar.settings_active {
        ui.rects
            .push(rect(slot, color(colors.tab_active_background)));
        color(colors.tab_active_text)
    } else {
        color(colors.tab_inactive_text)
    };
    if bar.settings_hovered && !bar.settings_active {
        button_hover(slot, text, ui);
    }
    let side = slot.width.min(slot.height);
    let stroke = (side * PLUS_STROKE).round().max(1.0);
    let width = (side * 0.45).round();
    let gap = (side / 6.0).round();
    let knob = 3.0 * stroke;
    let left = (slot.x + (slot.width - width) / 2.0).round();
    let cy = (slot.y + slot.height / 2.0).round();
    for (row, at) in [(-1.0, 0.3), (0.0, 0.7), (1.0, 0.45)] {
        let y = cy + row * gap;
        ui.rects.push(RectInstance::new(
            [left, y - stroke / 2.0],
            [width, stroke],
            text,
        ));
        ui.rects.push(RectInstance::new(
            [(left + width * at - knob / 2.0).round(), y - knob / 2.0],
            [knob, knob],
            text,
        ));
    }
}

/// The chrome over the panes of `layout`: the title bar of every pane (see
/// [`pane_bar`]), then, while it shows several panes, its dividers, in the translucent
/// inactive tab text color, and the border of the `active` pane, inside its rectangle, in the accent
/// color (the one highlighting command palette matches).
pub fn panes(
    layout: &Layout,
    active: Option<PaneId>,
    pane_bars: &[PaneBarLayout],
    colors: &UiColors,
    srgb_target: bool,
    ui: &mut UiLayer,
) {
    for bar in pane_bars {
        pane_bar(bar, colors, srgb_target, ui);
    }
    if layout.panes.len() < 2 {
        return;
    }
    let [r, g, b, _] = gpu_color(colors.tab_inactive_text, srgb_target);
    let divider = [r, g, b, DIVIDER_OPACITY];
    ui.rects
        .extend(layout.dividers.iter().map(|d| rect(d.rect, divider)));
    if let Some(pane) = active.and_then(|id| layout.rect_of(id)) {
        let accent = gpu_color(colors.palette_match, srgb_target);
        ui.rects.extend(border(pane, ACTIVE_BORDER, accent));
    }
}

/// One pane title bar: its background, then its title and close button.
fn pane_bar(bar: &PaneBarLayout, colors: &UiColors, srgb_target: bool, ui: &mut UiLayer) {
    let (background, text_rgb) = pane_bar_colors(colors, bar.active, bar.broadcast.is_some());
    let text = gpu_color(text_rgb, srgb_target);
    ui.rects
        .push(rect(bar.rect, gpu_color(background, srgb_target)));
    if bar.close_hovered {
        button_hover(bar.close, text, ui);
    }
    if let Some(pill) = &bar.pill {
        let (fill, label) = badge_colors(colors, pane_badge_fill(colors, background, text_rgb));
        badge(pill, fill, label, srgb_target, ui);
    }
    if let Some(progress) = &bar.progress {
        progress_bar(
            progress,
            pane_progress_colors(progress.colors, background, text_rgb),
            srgb_target,
            ui,
        );
    }
    ui.texts.push(UiTextItem {
        text: pane_bar_text(bar),
        x: bar.title_x,
        y: bar.title_y,
        color: text,
        max_width: bar.title_width,
        font: UiFont::Chrome,
    });
    // The glyph is about half the line height wide: centered in its square.
    ui.texts.push(UiTextItem {
        text: CLOSE.to_owned(),
        x: (bar.close.x + bar.close.width / 4.0).round(),
        y: bar.title_y,
        color: text,
        max_width: bar.close.width,
        font: UiFont::Chrome,
    });
}

/// Colors of the progress bar of a pane bar: the accent of a percentage or of an
/// indeterminate progress takes the bar's text color on a bar already filled with it
/// (the active pane's bar), where it would not show.
pub fn pane_progress_colors(colors: ProgressColors, background: Rgb, text: Rgb) -> ProgressColors {
    if colors.normal == background {
        ProgressColors {
            normal: text,
            ..colors
        }
    } else {
        colors
    }
}

/// A progress bar of the interface, in `colors`.
fn progress_bar(bar: &ProgressBar, colors: ProgressColors, srgb_target: bool, ui: &mut UiLayer) {
    ui.rects
        .extend(progress_rects(bar.rect, bar.view, &colors, srgb_target));
}

/// Fill of the badge of a pane bar: the accent color, unless the bar is already
/// filled with it (the active pane's bar), where the pill takes the bar's text color.
fn pane_badge_fill(colors: &UiColors, background: Rgb, text: Rgb) -> Rgb {
    if background == colors.palette_match {
        text
    } else {
        colors.palette_match
    }
}

/// Fill and label colors of a badge filled with `fill`: the label is the palette
/// background, made readable on the fill.
pub fn badge_colors(colors: &UiColors, fill: Rgb) -> (Rgb, Rgb) {
    (fill, readable_on(colors.palette_background, fill))
}

/// A counter badge: its rounded pill in `fill`, then its label in `label`.
fn badge(pill: &Pill, fill: Rgb, label: Rgb, srgb_target: bool, ui: &mut UiLayer) {
    ui.rects
        .push(rect(pill.rect, gpu_color(fill, srgb_target)).with_radius(pill.rect.height / 2.0));
    ui.texts.push(UiTextItem {
        text: pill.label.clone(),
        x: pill.label_x,
        y: pill.rect.y,
        color: gpu_color(label, srgb_target),
        max_width: pill.rect.width,
        font: UiFont::Chrome,
    });
}

/// The background of a hovered button: its rectangle in the translucent `text` color
/// of its glyph.
fn button_hover(button: Rect, text: [f32; 4], ui: &mut UiLayer) {
    let [r, g, b, _] = text;
    ui.rects.push(rect(button, [r, g, b, BUTTON_HOVER]));
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
    use crate::progress::{ProgressState, ProgressView};
    use crate::workspace::TabBarPosition;
    use crate::workspace::tree::{Axis, Branch, Divider};

    const BORDER: Rgb = Rgb { r: 1, g: 2, b: 3 };
    const MUTED: Rgb = Rgb { r: 4, g: 5, b: 6 };
    const ACCENT: Rgb = Rgb {
        r: 200,
        g: 100,
        b: 50,
    };

    fn colors() -> UiColors {
        UiColors {
            pane_border: BORDER,
            tab_inactive_text: MUTED,
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

    /// Rectangles of the pane chrome alone.
    fn pane_rects(
        layout: &Layout,
        active: Option<PaneId>,
        bars: &[PaneBarLayout],
        colors: &UiColors,
        srgb_target: bool,
    ) -> Vec<RectInstance> {
        let mut ui = UiLayer::default();
        panes(layout, active, bars, colors, srgb_target, &mut ui);
        ui.rects
    }

    /// (position, size, color) of each rect.
    fn drawn(rects: &[RectInstance]) -> Vec<([f32; 2], [f32; 2], [f32; 4])> {
        rects
            .iter()
            .map(|r| (r.position, r.size, r.color))
            .collect()
    }

    #[test]
    fn a_single_pane_without_bar_draws_nothing() {
        let layout = Layout {
            panes: vec![(PaneId(1), area(0.0, 0.0, 200.0, 100.0))],
            dividers: Vec::new(),
            divider: 1.0,
        };
        assert!(pane_rects(&layout, Some(PaneId(1)), &[], &colors(), false).is_empty());
        assert_eq!(
            build(&layout, Some(PaneId(1)), &[], None, None, &colors(), false),
            UiLayer::default()
        );
    }

    #[test]
    fn several_panes_draw_the_dividers_then_the_active_border() {
        let rects = pane_rects(&two_columns(), Some(PaneId(2)), &[], &colors(), false);
        let [r, g, b, _] = gpu_color(MUTED, false);
        let accent = gpu_color(ACCENT, false);
        assert_eq!(
            drawn(&rects),
            vec![
                // A single 1 px line, in the translucent inactive text color.
                ([100.0, 0.0], [1.0, 100.0], [r, g, b, DIVIDER_OPACITY]),
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
        for active in [Some(PaneId(9)), None] {
            let rects = pane_rects(&two_columns(), active, &[], &colors(), false);
            assert_eq!(rects.len(), 1);
            assert_eq!(rects[0].color[3], DIVIDER_OPACITY);
        }
    }

    #[test]
    fn colors_follow_the_target_encoding() {
        let rects = pane_rects(&two_columns(), Some(PaneId(1)), &[], &colors(), true);
        let [r, g, b, _] = gpu_color(MUTED, true);
        assert_eq!(rects[0].color, [r, g, b, DIVIDER_OPACITY]);
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
        let window = Rect {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 600.0,
        };
        TabBarLayout::new(window, TabBarPosition::Top, &titles, 1, metrics)
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
            &[],
            Some(&bar),
            None,
            &tab_colors(),
            false,
        );
        let color = |c| gpu_color(c, false);
        assert_eq!(ui.rects[0].position, [0.0, 0.0]);
        assert_eq!(ui.rects[0].size, [1000.0, bar.rect.height]);
        assert_eq!(ui.rects[0].color, color(BAR));
        // The settings icon, three lines and their knobs, inside its slot.
        assert!(ui.rects[1..7].iter().all(|r| {
            bar.settings.contains(r.position[0], r.position[1]) && r.color == color(INACTIVE_TEXT)
        }));
        // Only the shown tab (the second) gets a background.
        assert_eq!(ui.rects[7].position, [bar.tabs[1].rect.x, 0.0]);
        assert_eq!(ui.rects[7].color, color(ACTIVE_TAB));
        // Then the two strokes of the `+` button, inside it.
        assert_eq!(ui.rects.len(), 10);
        assert!(
            ui.rects[8..]
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
        assert!(ui.texts.iter().all(|t| t.y == bar.tabs[0].title_y));
    }

    #[test]
    fn a_tab_badge_is_an_accent_pill_with_its_count_after_the_title() {
        let single = Layout {
            panes: vec![(PaneId(1), area(0.0, 30.0, 1000.0, 100.0))],
            dividers: Vec::new(),
            divider: 1.0,
        };
        let colors = UiColors {
            palette_background: LABEL,
            ..tab_colors()
        };
        let mut bar = bar();
        bar.set_badges(&[0, 150]);
        let ui = build(
            &single,
            Some(PaneId(1)),
            &[],
            Some(&bar),
            None,
            &colors,
            false,
        );
        let pill = bar.tabs[1].pill.as_ref().unwrap();
        let drawn_pill = ui
            .rects
            .iter()
            .find(|r| r.position == [pill.rect.x, pill.rect.y])
            .unwrap();
        assert_eq!(drawn_pill.color, gpu_color(ACCENT, false));
        assert_eq!(drawn_pill.radius, pill.rect.height / 2.0);
        let label = ui.texts.iter().find(|t| t.text == "99+").unwrap();
        assert_eq!(label.x, pill.label_x);
        assert_eq!(label.color, gpu_color(readable_on(LABEL, ACCENT), false));
        // The tab without unseen notifications has no pill.
        assert!(bar.tabs[0].pill.is_none());
        assert_eq!(ui.texts.len(), 5);
    }

    #[test]
    fn tab_bar_comes_after_the_pane_rects() {
        let bar = bar();
        let ui = build(
            &two_columns(),
            Some(PaneId(1)),
            &[],
            Some(&bar),
            None,
            &tab_colors(),
            false,
        );
        let [r, g, b, _] = gpu_color(INACTIVE_TEXT, false);
        assert_eq!(ui.rects[0].color, [r, g, b, DIVIDER_OPACITY]);
        assert_eq!(ui.rects[1 + 4].color, gpu_color(BAR, false));
        assert!(ui.overlay_rects.is_empty() && ui.overlay_texts.is_empty());
    }

    #[test]
    fn drop_highlight_is_a_translucent_accent_rect_after_the_tab_bar() {
        let bar = bar();
        let mut ui = build(
            &two_columns(),
            Some(PaneId(1)),
            &[],
            Some(&bar),
            None,
            &colors(),
            false,
        );
        let drawn_before = ui.rects.len();
        let target = area(101.0, 0.0, 50.0, 100.0);
        drop_highlight(target, &colors(), false, &mut ui);
        assert_eq!(ui.rects.len(), drawn_before + 1);
        let [r, g, b, _] = gpu_color(ACCENT, false);
        assert_eq!(
            drawn(&ui.rects[drawn_before..]),
            vec![([101.0, 0.0], [50.0, 100.0], [r, g, b, DROP_HIGHLIGHT])]
        );
        assert!(ui.overlay_rects.is_empty());
    }

    const LABEL: Rgb = Rgb { r: 70, g: 0, b: 0 };

    fn pane_bar_metrics() -> UiMetrics {
        UiMetrics {
            line_height: 18.0,
            scale: 1.0,
        }
    }

    fn pane_bars_of(layout: &Layout, active: Option<PaneId>) -> Vec<PaneBarLayout> {
        layout
            .panes
            .iter()
            .map(|&(id, rect)| {
                let title = format!("pane {}", id.0);
                PaneBarLayout::new(id, rect, title, Some(id) == active, pane_bar_metrics())
            })
            .collect()
    }

    #[test]
    fn a_single_pane_draws_its_title_bar_but_no_border() {
        let layout = Layout {
            panes: vec![(PaneId(1), area(0.0, 0.0, 200.0, 100.0))],
            dividers: Vec::new(),
            divider: 1.0,
        };
        let colors = UiColors {
            palette_background: LABEL,
            ..tab_colors()
        };
        let bars = pane_bars_of(&layout, Some(PaneId(1)));
        let ui = build(&layout, Some(PaneId(1)), &bars, None, None, &colors, false);
        let color = |c| gpu_color(c, false);
        assert_eq!(
            drawn(&ui.rects),
            vec![([0.0, 0.0], [200.0, bars[0].rect.height], color(ACCENT))]
        );
        let texts: Vec<(&str, [f32; 4], f32)> = ui
            .texts
            .iter()
            .map(|t| (t.text.as_str(), t.color, t.max_width))
            .collect();
        assert_eq!(
            texts,
            [
                ("pane 1", color(LABEL), bars[0].title_width),
                (CLOSE, color(LABEL), bars[0].close.width),
            ]
        );
        assert!(ui.texts.iter().all(|t| t.font == UiFont::Chrome));
        assert_eq!(ui.texts[0].x, bars[0].title_x);
        assert!(ui.texts.iter().all(|t| t.y == bars[0].title_y));
    }

    #[test]
    fn pane_bars_come_before_the_dividers_and_the_active_border() {
        let layout = two_columns();
        let colors = tab_colors();
        let bars = pane_bars_of(&layout, Some(PaneId(2)));
        let rects = pane_rects(&layout, Some(PaneId(2)), &bars, &colors, false);
        let color = |c| gpu_color(c, false);
        let inactive = color(blend(BAR, ACCENT, INACTIVE_PANE_BAR));
        assert_eq!(rects.len(), 2 + 1 + 4);
        assert_eq!(rects[0].position, [0.0, 0.0]);
        assert_eq!(rects[0].color, inactive);
        assert_eq!(rects[1].position, [101.0, 0.0]);
        assert_eq!(rects[1].color, color(ACCENT));
        let [r, g, b, _] = color(INACTIVE_TEXT);
        assert_eq!(rects[2].color, [r, g, b, DIVIDER_OPACITY]);
        assert!(rects[3..].iter().all(|r| r.color == color(ACCENT)));
    }

    #[test]
    fn a_pane_badge_is_an_accent_pill_or_the_text_color_on_the_active_bar() {
        let layout = two_columns();
        let colors = UiColors {
            palette_background: LABEL,
            ..tab_colors()
        };
        let bars: Vec<PaneBarLayout> = pane_bars_of(&layout, Some(PaneId(2)))
            .into_iter()
            .map(|bar| bar.with_badge(3, pane_bar_metrics()))
            .collect();
        let ui = build(&layout, Some(PaneId(2)), &bars, None, None, &colors, false);
        let fill_of = |bar: &PaneBarLayout| {
            let pill = bar.pill.as_ref().unwrap();
            ui.rects
                .iter()
                .find(|r| r.position == [pill.rect.x, pill.rect.y] && r.radius > 0.0)
                .unwrap()
                .color
        };
        // Inactive bar: the accent. Active bar, already the accent: its text color.
        assert_eq!(fill_of(&bars[0]), gpu_color(ACCENT, false));
        assert_eq!(fill_of(&bars[1]), gpu_color(LABEL, false));
        let labels: Vec<[f32; 4]> = ui
            .texts
            .iter()
            .filter(|t| t.text == "3")
            .map(|t| t.color)
            .collect();
        assert_eq!(
            labels,
            [
                gpu_color(readable_on(LABEL, ACCENT), false),
                gpu_color(readable_on(LABEL, LABEL), false),
            ]
        );
    }

    #[test]
    fn active_pane_bar_is_the_accent_and_an_inactive_one_a_muted_accent() {
        let colors = UiColors {
            palette_background: LABEL,
            ..tab_colors()
        };
        assert_eq!(pane_bar_colors(&colors, true, false), (ACCENT, LABEL));
        let (background, text) = pane_bar_colors(&colors, false, false);
        assert_eq!(text, ACTIVE_TEXT);
        assert_eq!(background, blend(BAR, ACCENT, INACTIVE_PANE_BAR));
        assert_ne!(background, ACCENT);
        assert_ne!(background, BAR);
    }

    const WARM: Rgb = Rgb {
        r: 200,
        g: 100,
        b: 0,
    };

    #[test]
    fn a_broadcast_pane_bar_uses_the_broadcast_color() {
        let colors = UiColors {
            palette_background: LABEL,
            pane_broadcast: WARM,
            ..tab_colors()
        };
        assert_eq!(pane_bar_colors(&colors, true, true), (WARM, LABEL));
        let (background, text) = pane_bar_colors(&colors, false, true);
        assert_eq!(background, blend(BAR, WARM, INACTIVE_BROADCAST_BAR));
        assert!(contrast_ratio(text, background) >= BAR_TEXT_CONTRAST);
        assert_ne!(background, blend(BAR, ACCENT, INACTIVE_PANE_BAR));
    }

    #[test]
    fn readable_on_keeps_a_readable_text_and_fixes_a_dim_one() {
        let dark = Rgb {
            r: 10,
            g: 40,
            b: 45,
        };
        let good = Rgb {
            r: 240,
            g: 240,
            b: 240,
        };
        assert_eq!(readable_on(good, dark), good);
        let dim = Rgb {
            r: 90,
            g: 110,
            b: 110,
        };
        let fixed = readable_on(dim, dark);
        assert!(contrast_ratio(fixed, dark) >= BAR_TEXT_CONTRAST);
        let light = Rgb {
            r: 235,
            g: 230,
            b: 215,
        };
        let pale = Rgb {
            r: 170,
            g: 165,
            b: 150,
        };
        let fixed = readable_on(pale, light);
        assert!(contrast_ratio(fixed, light) >= BAR_TEXT_CONTRAST);
        assert!(luminance(fixed) < luminance(pale));
    }

    const RED: Rgb = Rgb { r: 200, g: 0, b: 0 };

    fn progress_colors() -> ProgressColors {
        ProgressColors {
            normal: ACCENT,
            error: RED,
            paused: WARM,
            track: INACTIVE_TEXT,
        }
    }

    fn progress_view(state: ProgressState, value: u8) -> ProgressView {
        ProgressView {
            state,
            value,
            offset: 0.0,
        }
    }

    /// Color of the filled part of the progress bar drawn on `band`.
    fn progress_fill(ui: &UiLayer, band: Rect) -> [f32; 4] {
        let pair = ui
            .rects
            .windows(2)
            .find(|pair| pair[0].position == [band.x, band.y] && pair[0].size[1] == band.height)
            .expect("a progress bar");
        pair[1].color
    }

    #[test]
    fn a_tab_shows_its_progress_on_its_bottom_edge() {
        let mut tabs = bar();
        tabs.set_progress(
            &[Some(progress_view(ProgressState::Paused, 50)), None],
            progress_colors(),
        );
        let band = tabs.tabs[0].progress.unwrap().rect;
        let ui = build(
            &two_columns(),
            None,
            &[],
            Some(&tabs),
            None,
            &tab_colors(),
            false,
        );
        let [r, g, b, _] = gpu_color(INACTIVE_TEXT, false);
        let track = ui
            .rects
            .iter()
            .find(|rect| rect.position == [band.x, band.y])
            .expect("the track");
        assert_eq!(track.size, [band.width, band.height]);
        assert_eq!(
            track.color,
            [r, g, b, crate::progress::PROGRESS_TRACK_OPACITY]
        );
        assert_eq!(progress_fill(&ui, band), gpu_color(WARM, false));
        let bars = ui
            .rects
            .iter()
            .filter(|rect| rect.size[1] == band.height && rect.position[1] == band.y)
            .count();
        assert_eq!(bars, 2, "only the first tab has a progress bar");
    }

    #[test]
    fn a_pane_bar_shows_its_progress_readable_on_the_accent() {
        let layout = two_columns();
        let colors = UiColors {
            palette_background: LABEL,
            ..tab_colors()
        };
        let metrics = pane_bar_metrics();
        let bars: Vec<_> = pane_bars_of(&layout, Some(PaneId(2)))
            .into_iter()
            .map(|bar| {
                bar.with_progress(
                    Some(progress_view(ProgressState::Normal, 30)),
                    progress_colors(),
                    metrics,
                )
            })
            .collect();
        let ui = build(&layout, Some(PaneId(2)), &bars, None, None, &colors, false);
        let inactive = bars[0].progress.unwrap().rect;
        let active = bars[1].progress.unwrap().rect;
        assert_eq!(
            inactive.y + inactive.height,
            bars[0].rect.y + bars[0].rect.height
        );
        // The inactive bar is barely tinted: the accent shows on it.
        assert_eq!(progress_fill(&ui, inactive), gpu_color(ACCENT, false));
        // The active bar is filled with the accent: its label color instead.
        assert_eq!(progress_fill(&ui, active), gpu_color(LABEL, false));
    }

    #[test]
    fn an_error_keeps_its_color_on_the_active_pane_bar() {
        let colors = pane_progress_colors(progress_colors(), ACCENT, LABEL);
        assert_eq!(colors.bar(ProgressState::Error), RED);
        assert_eq!(colors.bar(ProgressState::Normal), LABEL);
        assert_eq!(colors.bar(ProgressState::Indeterminate), LABEL);
        assert_eq!(
            pane_progress_colors(progress_colors(), BAR, LABEL),
            progress_colors()
        );
    }

    #[test]
    fn a_broadcast_pane_bar_shows_the_set_size_before_the_title() {
        let layout = two_columns();
        let colors = UiColors {
            pane_broadcast: WARM,
            ..tab_colors()
        };
        let mut bars = pane_bars_of(&layout, Some(PaneId(2)));
        let plain = build(&layout, Some(PaneId(2)), &bars, None, None, &colors, false);
        assert_eq!(plain.texts[0].text, "pane 1");
        bars[0].broadcast = Some(2);
        bars[1].broadcast = Some(2);
        let ui = build(&layout, Some(PaneId(2)), &bars, None, None, &colors, false);
        assert_eq!(ui.texts[0].text, "»2 pane 1");
        assert_eq!(ui.texts[0].max_width, bars[0].title_width);
        assert_eq!(
            ui.rects[1].color,
            gpu_color(WARM, false),
            "the active bar of the set"
        );
    }

    #[test]
    fn a_hovered_pane_close_button_gets_a_translucent_text_background() {
        let layout = two_columns();
        let colors = UiColors {
            palette_background: LABEL,
            ..tab_colors()
        };
        let mut bars = pane_bars_of(&layout, Some(PaneId(2)));
        let plain = pane_rects(&layout, Some(PaneId(2)), &bars, &colors, false);
        bars[1].close_hovered = true;
        let rects = pane_rects(&layout, Some(PaneId(2)), &bars, &colors, false);
        assert_eq!(rects.len(), plain.len() + 1);
        // Right after the background of the hovered bar, in its text color.
        let [r, g, b, _] = gpu_color(LABEL, false);
        assert_eq!(
            drawn(&rects[2..3]),
            vec![(
                [bars[1].close.x, bars[1].close.y],
                [bars[1].close.width, bars[1].close.height],
                [r, g, b, BUTTON_HOVER]
            )]
        );
        assert_eq!(rects[3..], plain[2..]);
    }

    #[test]
    fn a_hovered_tab_close_button_gets_a_translucent_text_background() {
        let single = Layout {
            panes: vec![(PaneId(1), area(0.0, 30.0, 1000.0, 100.0))],
            dividers: Vec::new(),
            divider: 1.0,
        };
        let mut bar = bar();
        bar.tabs[0].close_hovered = true;
        let ui = build(&single, None, &[], Some(&bar), None, &tab_colors(), false);
        // Bar background, settings icon, hover of the first tab's button (the first tab
        // is not shown: inactive text color), shown tab background, then the `+` strokes.
        let [r, g, b, _] = gpu_color(INACTIVE_TEXT, false);
        let close = bar.tabs[0].close;
        assert_eq!(ui.rects.len(), 11);
        assert_eq!(
            drawn(&ui.rects[7..8]),
            vec![(
                [close.x, close.y],
                [close.width, close.height],
                [r, g, b, BUTTON_HOVER]
            )]
        );
    }

    #[test]
    fn a_hovered_new_tab_button_gets_a_translucent_text_background() {
        let single = Layout {
            panes: vec![(PaneId(1), area(0.0, 30.0, 1000.0, 100.0))],
            dividers: Vec::new(),
            divider: 1.0,
        };
        let mut bar = bar();
        bar.new_tab_hovered = true;
        let ui = build(&single, None, &[], Some(&bar), None, &tab_colors(), false);
        // Bar background, settings icon, shown tab background, hover of the `+` button,
        // its strokes.
        let [r, g, b, _] = gpu_color(INACTIVE_TEXT, false);
        let plus = bar.new_tab;
        assert_eq!(ui.rects.len(), 11);
        assert_eq!(
            drawn(&ui.rects[8..9]),
            vec![(
                [plus.x, plus.y],
                [plus.width, plus.height],
                [r, g, b, BUTTON_HOVER]
            )]
        );
    }

    #[test]
    fn the_shown_settings_tab_gets_the_shown_tab_background() {
        let mut bar = bar();
        bar.set_settings_active(true);
        let ui = build(
            &Layout::default(),
            None,
            &[],
            Some(&bar),
            None,
            &tab_colors(),
            false,
        );
        let color = |c| gpu_color(c, false);
        let slot = bar.settings;
        assert_eq!(
            drawn(&ui.rects[1..2]),
            vec![(
                [slot.x, slot.y],
                [slot.width, slot.height],
                color(ACTIVE_TAB)
            )]
        );
        // The icon in the shown tab's text color; no tab background after it.
        assert!(ui.rects[2..8].iter().all(|r| r.color == color(ACTIVE_TEXT)));
        assert_eq!(ui.rects.len(), 10);
    }

    #[test]
    fn a_hovered_settings_tab_gets_a_translucent_text_background() {
        let mut bar = bar();
        bar.settings_hovered = true;
        let ui = build(
            &Layout::default(),
            None,
            &[],
            Some(&bar),
            None,
            &tab_colors(),
            false,
        );
        let [r, g, b, _] = gpu_color(INACTIVE_TEXT, false);
        let slot = bar.settings;
        assert_eq!(
            drawn(&ui.rects[1..2]),
            vec![(
                [slot.x, slot.y],
                [slot.width, slot.height],
                [r, g, b, BUTTON_HOVER]
            )]
        );
    }

    const DIALOG_BACKGROUND: Rgb = Rgb { r: 50, g: 0, b: 0 };
    const DIALOG_TEXT: Rgb = Rgb { r: 60, g: 0, b: 0 };

    #[test]
    fn art_is_drawn_in_the_overlay_with_the_mono_font() {
        use crate::easter_eggs::art::GRAPHENE_ART;
        let colors = UiColors {
            palette_background: DIALOG_BACKGROUND,
            palette_text: DIALOG_TEXT,
            ..tab_colors()
        };
        let metrics = UiMetrics {
            line_height: 18.0,
            scale: 1.0,
        };
        let art = ArtLayout::new(area(0.0, 0.0, 1000.0, 600.0), metrics, |t| {
            t.chars().count() as f32 * 8.0
        });
        let ui = build(
            &two_columns(),
            Some(PaneId(1)),
            &[],
            None,
            Some(Overlay::Art(&art)),
            &colors,
            false,
        );
        let background = gpu_color(DIALOG_BACKGROUND, false);
        assert_eq!(
            drawn(&ui.overlay_rects[..1]),
            vec![(
                [art.rect.x, art.rect.y],
                [art.rect.width, art.rect.height],
                background
            )]
        );
        let texts: Vec<&str> = ui.overlay_texts.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, GRAPHENE_ART);
        assert!(ui.overlay_texts.iter().all(|t| t.font == UiFont::Mono
            && t.color == gpu_color(DIALOG_TEXT, false)
            && t.x == art.text_x));
    }

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
            &[],
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
            &[],
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
    fn rename_box_is_drawn_in_the_overlay() {
        use crate::chrome::rename_tab::{RenameTab, RenameTabLayout};
        use crate::i18n::Lang;
        const SELECTION: Rgb = Rgb { r: 70, g: 0, b: 0 };
        let colors = UiColors {
            palette_background: DIALOG_BACKGROUND,
            palette_text: DIALOG_TEXT,
            palette_selection_background: SELECTION,
            ..tab_colors()
        };
        let metrics = UiMetrics {
            line_height: 18.0,
            scale: 1.0,
        };
        let draw = |rename: &RenameTab| {
            let layout = RenameTabLayout::new(
                rename,
                Lang::En,
                area(0.0, 0.0, 1000.0, 600.0),
                metrics,
                |t| t.chars().count() as f32 * 8.0,
            );
            build(
                &two_columns(),
                Some(PaneId(1)),
                &[],
                None,
                Some(Overlay::RenameTab(&layout)),
                &colors,
                false,
            )
        };
        let color = |c| gpu_color(c, false);
        let mut rename = RenameTab::new(PaneId(1), "~/dev");
        let ui = draw(&rename);
        // Box, its 4 border sides, the field, the selection and the text cursor.
        assert_eq!(ui.overlay_rects.len(), 8);
        assert_eq!(ui.overlay_rects[0].color, color(DIALOG_BACKGROUND));
        assert_eq!(ui.overlay_rects[5].color, color(BAR));
        assert_eq!(ui.overlay_rects[6].color, color(SELECTION));
        assert_eq!(ui.overlay_rects[7].color, color(DIALOG_TEXT));
        let texts: Vec<&str> = ui.overlay_texts.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["Rename the tab", "~/dev"]);
        assert!(ui.overlay_texts.iter().all(|t| t.font == UiFont::Palette));
        // Once edited, the selection is gone.
        rename.key(&winit::keyboard::Key::Character("x".into()), Some("x"));
        assert_eq!(draw(&rename).overlay_rects.len(), 7);
    }

    #[test]
    fn palette_is_drawn_in_the_overlay() {
        use crate::chrome::command_palette::{Command, CommandPalette, Entry, PaletteLayout};
        use crate::i18n::Lang;
        use crate::input::Action;
        use winit::keyboard::Key;
        const SELECTED_ROW: Rgb = Rgb { r: 50, g: 0, b: 0 };
        let colors = UiColors {
            palette_background: DIALOG_BACKGROUND,
            palette_text: DIALOG_TEXT,
            palette_selection_background: SELECTED_ROW,
            ..tab_colors()
        };
        let entry = |label: &str, action, shortcut: Option<&str>| Entry {
            command: Command::Action(action),
            label: label.to_owned(),
            shortcut: shortcut.map(str::to_owned),
        };
        let mut palette = CommandPalette::new(
            Lang::En,
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
            &[],
            None,
            Some(Overlay::Palette(&layout)),
            &colors,
            false,
        );
        let color = |c| gpu_color(c, false);
        // Box, its 4 border sides, the search field, the text cursor, the selected row
        // and its accent bar.
        assert_eq!(ui.overlay_rects.len(), 9);
        assert_eq!(ui.overlay_rects[0].color, color(DIALOG_BACKGROUND));
        assert_eq!(ui.overlay_rects[5].color, color(BAR));
        assert_eq!(ui.overlay_rects[6].color, color(DIALOG_TEXT));
        assert_eq!(ui.overlay_rects[7].color, color(SELECTED_ROW));
        assert_ne!(
            ui.overlay_rects[7].color,
            color(ACTIVE_TAB),
            "the selection does not reuse the active tab color"
        );
        assert_eq!(
            ui.overlay_rects[7].position,
            [layout.rows[0].rect.x, layout.rows[0].rect.y]
        );
        let marker = layout.marker.expect("marker");
        assert_eq!(ui.overlay_rects[8].color, color(ACCENT));
        assert_eq!(
            ui.overlay_rects[8].position,
            [layout.rows[0].rect.x, layout.rows[0].rect.y]
        );
        assert_eq!(ui.overlay_rects[8].size, [marker.width, marker.height]);
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
        let palette = CommandPalette::new(Lang::En, Vec::new(), Vec::new());
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
            &[],
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
    fn context_menu_uses_chrome_colors_and_palette_font() {
        use crate::chrome::context_menu::{
            Command, ContextMenu, Item, MenuHit, MenuLayout, MenuLevel,
        };
        use crate::input::Action;
        const HOVER_SELECTION: Rgb = Rgb {
            r: 150,
            g: 100,
            b: 50,
        };
        let colors = UiColors {
            palette_background: DIALOG_BACKGROUND,
            palette_selection_background: HOVER_SELECTION,
            palette_text: DIALOG_TEXT,
            tab_active_background: ACTIVE_TAB,
            tab_inactive_text: INACTIVE_TEXT,
            ..colors()
        };
        let mut menu = ContextMenu::new(
            (20.0, 20.0),
            vec![Item::Entry {
                label: "Copy".into(),
                shortcut: Some("Ctrl+C".into()),
                enabled: true,
                command: Command::Action(Action::Copy),
            }],
        );
        let metrics = UiMetrics {
            line_height: 18.0,
            scale: 1.0,
        };
        let layout = MenuLayout::new(&menu, area(0.0, 0.0, 300.0, 200.0), metrics, |text| {
            text.chars().count() as f32 * 8.0
        });
        menu.hover(MenuHit::Entry {
            level: MenuLevel::MAIN,
            index: layout.rows[0].index,
        });
        let layout = MenuLayout::new(&menu, area(0.0, 0.0, 300.0, 200.0), metrics, |text| {
            text.chars().count() as f32 * 8.0
        });
        let ui = build(
            &two_columns(),
            Some(PaneId(1)),
            &[],
            None,
            Some(Overlay::Menu(&layout)),
            &colors,
            false,
        );
        let color = |c| gpu_color(c, false);
        assert_eq!(ui.overlay_rects[0].color, color(DIALOG_BACKGROUND));
        assert_eq!(ui.overlay_rects[1].color, color(BORDER));
        assert_eq!(
            ui.overlay_rects[5].color,
            color(Rgb {
                r: 95,
                g: 45,
                b: 23,
            })
        );
        let texts: Vec<_> = ui
            .overlay_texts
            .iter()
            .map(|text| (text.text.as_str(), text.color, text.font))
            .collect();
        assert_eq!(
            texts,
            [
                ("Copy", color(DIALOG_TEXT), UiFont::Palette),
                ("Ctrl+C", color(INACTIVE_TEXT), UiFont::Palette)
            ]
        );
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
        let mut ui = build(
            &two_columns(),
            Some(PaneId(1)),
            &[],
            None,
            None,
            &colors,
            false,
        );
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
