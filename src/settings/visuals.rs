//! egui look of the settings tab, built from the active theme.

use alacritty_terminal::vte::ansi::Rgb;
use egui::{Color32, Stroke, Theme, Visuals};

use crate::frame::blend;
use crate::palette::Palette;

/// egui theme matching a background: dark when its luminance is below half.
pub fn theme(background: Rgb) -> Theme {
    // Rec. 709 luma of the gamma-encoded channels: enough to pick dark or light.
    let luma = 0.2126 * f32::from(background.r)
        + 0.7152 * f32::from(background.g)
        + 0.0722 * f32::from(background.b);
    if luma < 127.5 {
        Theme::Dark
    } else {
        Theme::Light
    }
}

/// The egui visuals of `palette`: egui's dark or light base, recolored with the
/// theme's background, text, accent and borders.
pub fn visuals(palette: &Palette) -> Visuals {
    let ui = &palette.ui;
    let mut visuals = match theme(palette.background) {
        Theme::Dark => Visuals::dark(),
        Theme::Light => Visuals::light(),
    };
    let background = color(palette.background);
    let text = color(palette.foreground);
    let accent = color(ui.palette_match);
    let border = Stroke::new(1.0, color(ui.pane_border));
    // Controls at rest: the text dimmed halfway toward the background, visible on it.
    let rest_border = Stroke::new(
        1.0,
        color(blend(palette.foreground, palette.background, 0.5)),
    );

    visuals.panel_fill = background;
    visuals.window_fill = color(ui.palette_background);
    visuals.window_stroke = border;
    visuals.faint_bg_color = color(ui.tab_bar_background);
    visuals.extreme_bg_color = color(ui.tab_bar_background);
    visuals.hyperlink_color = accent;
    visuals.selection.bg_fill = color(palette.selection_background);
    visuals.selection.stroke = Stroke::new(1.0, accent);

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = background;
    widgets.noninteractive.weak_bg_fill = background;
    widgets.noninteractive.bg_stroke = border;
    widgets.noninteractive.fg_stroke.color = text;
    for state in [&mut widgets.inactive, &mut widgets.open] {
        state.bg_fill = color(ui.tab_bar_background);
        state.weak_bg_fill = color(ui.tab_bar_background);
        state.fg_stroke.color = text;
        state.bg_stroke = rest_border;
    }
    for state in [&mut widgets.hovered, &mut widgets.active] {
        state.bg_fill = color(ui.tab_active_background);
        state.weak_bg_fill = color(ui.tab_active_background);
        state.bg_stroke = Stroke::new(1.0, accent);
        state.fg_stroke.color = color(ui.tab_active_text);
    }
    visuals
}

fn color(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }

    #[test]
    fn dark_or_light_follows_the_background() {
        assert_eq!(theme(rgb(0x1e, 0x1e, 0x2e)), Theme::Dark);
        assert_eq!(theme(rgb(0, 0, 0)), Theme::Dark);
        assert_eq!(theme(rgb(0xfa, 0xfa, 0xfa)), Theme::Light);
        // Bright pure blue is still dark.
        assert_eq!(theme(rgb(0, 0, 0xff)), Theme::Dark);
    }

    #[test]
    fn visuals_take_the_theme_colors() {
        let mut palette = Palette {
            background: rgb(10, 20, 30),
            foreground: rgb(200, 210, 220),
            selection_background: rgb(1, 2, 3),
            ..Palette::default()
        };
        palette.ui.palette_match = rgb(250, 100, 0);
        palette.ui.pane_border = rgb(60, 60, 60);
        palette.ui.tab_bar_background = rgb(5, 5, 5);
        palette.ui.tab_active_background = rgb(40, 40, 40);
        palette.ui.tab_active_text = rgb(255, 255, 255);
        palette.ui.palette_background = rgb(15, 15, 15);

        let visuals = visuals(&palette);
        assert!(visuals.dark_mode);
        assert_eq!(visuals.panel_fill, Color32::from_rgb(10, 20, 30));
        assert_eq!(visuals.window_fill, Color32::from_rgb(15, 15, 15));
        assert_eq!(visuals.extreme_bg_color, Color32::from_rgb(5, 5, 5));
        assert_eq!(visuals.hyperlink_color, Color32::from_rgb(250, 100, 0));
        assert_eq!(visuals.selection.bg_fill, Color32::from_rgb(1, 2, 3));
        let widgets = &visuals.widgets;
        assert_eq!(
            widgets.noninteractive.fg_stroke.color,
            Color32::from_rgb(200, 210, 220)
        );
        assert_eq!(
            widgets.noninteractive.bg_stroke.color,
            Color32::from_rgb(60, 60, 60)
        );
        assert_eq!(widgets.inactive.bg_fill, Color32::from_rgb(5, 5, 5));
        // Halfway between the text (200, 210, 220) and the background (10, 20, 30).
        for state in [&widgets.inactive, &widgets.open] {
            assert_eq!(state.bg_stroke.color, Color32::from_rgb(105, 115, 125));
            assert!(state.bg_stroke.width > 0.0);
        }
        assert_eq!(widgets.hovered.bg_fill, Color32::from_rgb(40, 40, 40));
        assert_eq!(
            widgets.active.fg_stroke.color,
            Color32::from_rgb(255, 255, 255)
        );
        assert_eq!(
            widgets.hovered.bg_stroke.color,
            Color32::from_rgb(250, 100, 0)
        );
    }

    #[test]
    fn a_light_background_gives_light_visuals() {
        let palette = Palette {
            background: rgb(0xff, 0xff, 0xf0),
            ..Palette::default()
        };
        assert!(!visuals(&palette).dark_mode);
    }
}
