//! Color theme and resolution of terminal colors into concrete RGB values.

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

/// Factor applied to a color to produce its dimmed variant.
const DIM_FACTOR: f32 = 0.66;

/// A complete color theme, read from a theme file by [`crate::theme`].
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    /// The 256 indexed colors (16 ANSI + 6x6x6 cube + 24 grays).
    pub indexed: [Rgb; 256],
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    /// Color of the text drawn on top of a block cursor.
    pub cursor_text: Rgb,
    /// Background of selected cells.
    pub selection_background: Rgb,
    /// Foreground of selected cells; `None` keeps each cell's own foreground.
    pub selection_foreground: Option<Rgb>,
    pub ui: UiColors,
}

/// Colors of the interface drawn around the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiColors {
    pub tab_bar_background: Rgb,
    pub tab_active_background: Rgb,
    pub tab_active_text: Rgb,
    pub tab_inactive_text: Rgb,
    pub pane_border: Rgb,
    pub palette_background: Rgb,
    pub palette_text: Rgb,
    /// Highlight of the characters matching the search in the command palette.
    pub palette_match: Rgb,
    pub banner_background: Rgb,
    pub banner_text: Rgb,
}

impl UiColors {
    /// Every color set to `color`.
    pub fn uniform(color: Rgb) -> Self {
        Self {
            tab_bar_background: color,
            tab_active_background: color,
            tab_active_text: color,
            tab_inactive_text: color,
            pane_border: color,
            palette_background: color,
            palette_text: color,
            palette_match: color,
            banner_background: color,
            banner_text: color,
        }
    }
}

impl Default for Palette {
    /// The "graphene night" theme.
    fn default() -> Self {
        crate::theme::default_palette().clone()
    }
}

impl Palette {
    /// An all-black palette, the base on which the built-in `graphene-night` is parsed.
    pub fn blank() -> Self {
        let black = Rgb { r: 0, g: 0, b: 0 };
        Self {
            indexed: build_indexed([black; 16]),
            foreground: black,
            background: black,
            cursor: black,
            cursor_text: black,
            selection_background: black,
            selection_foreground: None,
            ui: UiColors::uniform(black),
        }
    }
}

/// Builds the 256-color table from the 16 base ANSI colors.
pub fn build_indexed(ansi: [Rgb; 16]) -> [Rgb; 256] {
    let mut table = [Rgb { r: 0, g: 0, b: 0 }; 256];
    table[..16].copy_from_slice(&ansi);

    // 6x6x6 color cube.
    let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
    for r in 0..6u8 {
        for g in 0..6u8 {
            for b in 0..6u8 {
                let index = 16 + 36 * r as usize + 6 * g as usize + b as usize;
                table[index] = Rgb {
                    r: level(r),
                    g: level(g),
                    b: level(b),
                };
            }
        }
    }

    // Grayscale ramp.
    for i in 0..24u8 {
        let v = 8 + i * 10;
        table[232 + i as usize] = Rgb { r: v, g: v, b: v };
    }

    table
}

fn dim(color: Rgb) -> Rgb {
    let f = |c: u8| (c as f32 * DIM_FACTOR).round() as u8;
    Rgb {
        r: f(color.r),
        g: f(color.g),
        b: f(color.b),
    }
}

impl Palette {
    /// Resolves a terminal color, honoring runtime overrides set via OSC sequences.
    pub fn resolve(&self, color: Color, overrides: &Colors) -> Rgb {
        match color {
            Color::Spec(rgb) => rgb,
            Color::Indexed(index) => {
                overrides[index as usize].unwrap_or(self.indexed[index as usize])
            }
            Color::Named(named) => overrides[named].unwrap_or_else(|| self.named(named)),
        }
    }

    /// Default value of a named color.
    pub fn named(&self, named: NamedColor) -> Rgb {
        let index = named as usize;
        if index < 16 {
            return self.indexed[index];
        }
        match named {
            NamedColor::Foreground | NamedColor::BrightForeground => self.foreground,
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            NamedColor::DimForeground => dim(self.foreground),
            NamedColor::DimBlack => dim(self.indexed[0]),
            NamedColor::DimRed => dim(self.indexed[1]),
            NamedColor::DimGreen => dim(self.indexed[2]),
            NamedColor::DimYellow => dim(self.indexed[3]),
            NamedColor::DimBlue => dim(self.indexed[4]),
            NamedColor::DimMagenta => dim(self.indexed[5]),
            NamedColor::DimCyan => dim(self.indexed[6]),
            NamedColor::DimWhite => dim(self.indexed[7]),
            _ => self.foreground,
        }
    }

    /// Default color for a raw color index as used by OSC 4/10/11/12 queries.
    pub fn by_index(&self, index: usize) -> Rgb {
        match index {
            0..=255 => self.indexed[index],
            i if i == NamedColor::Background as usize => self.background,
            i if i == NamedColor::Cursor as usize => self.cursor,
            _ => self.foreground,
        }
    }

    /// Computes the final (foreground, background) pair for a cell.
    pub fn cell_colors(
        &self,
        fg: Color,
        bg: Color,
        flags: Flags,
        overrides: &Colors,
    ) -> (Rgb, Rgb) {
        let mut fg_rgb = self.resolve(fg, overrides);
        let mut bg_rgb = self.resolve(bg, overrides);

        if flags.contains(Flags::DIM) && !flags.contains(Flags::BOLD) {
            fg_rgb = dim(fg_rgb);
        }
        if flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg_rgb, &mut bg_rgb);
        }
        if flags.contains(Flags::HIDDEN) {
            fg_rgb = bg_rgb;
        }

        (fg_rgb, bg_rgb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_table_contains_cube_and_grays() {
        let p = Palette::default();
        assert_eq!(p.indexed[16], Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(
            p.indexed[231],
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert_eq!(p.indexed[196], Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(p.indexed[232], Rgb { r: 8, g: 8, b: 8 });
        assert_eq!(
            p.indexed[255],
            Rgb {
                r: 238,
                g: 238,
                b: 238
            }
        );
    }

    #[test]
    fn resolve_spec_and_named() {
        let p = Palette::default();
        let o = Colors::default();
        let spec = Rgb { r: 1, g: 2, b: 3 };
        assert_eq!(p.resolve(Color::Spec(spec), &o), spec);
        assert_eq!(p.resolve(Color::Named(NamedColor::Red), &o), p.indexed[1]);
        assert_eq!(
            p.resolve(Color::Named(NamedColor::Foreground), &o),
            p.foreground
        );
        assert_eq!(
            p.resolve(Color::Named(NamedColor::Background), &o),
            p.background
        );
    }

    #[test]
    fn resolve_honors_overrides() {
        let p = Palette::default();
        let mut o = Colors::default();
        let custom = Rgb { r: 9, g: 9, b: 9 };
        o[NamedColor::Foreground] = Some(custom);
        o[42] = Some(custom);
        assert_eq!(p.resolve(Color::Named(NamedColor::Foreground), &o), custom);
        assert_eq!(p.resolve(Color::Indexed(42), &o), custom);
    }

    #[test]
    fn by_index_covers_special_colors() {
        let p = Palette::default();
        assert_eq!(p.by_index(3), p.indexed[3]);
        assert_eq!(p.by_index(NamedColor::Foreground as usize), p.foreground);
        assert_eq!(p.by_index(NamedColor::Background as usize), p.background);
        assert_eq!(p.by_index(NamedColor::Cursor as usize), p.cursor);
    }

    #[test]
    fn inverse_swaps_and_hidden_masks() {
        let p = Palette::default();
        let o = Colors::default();
        let fg = Color::Named(NamedColor::Foreground);
        let bg = Color::Named(NamedColor::Background);

        let (f, b) = p.cell_colors(fg, bg, Flags::INVERSE, &o);
        assert_eq!((f, b), (p.background, p.foreground));

        let (f, b) = p.cell_colors(fg, bg, Flags::HIDDEN, &o);
        assert_eq!(f, b);
    }

    #[test]
    fn dim_darkens_foreground() {
        let p = Palette::default();
        let o = Colors::default();
        let white = Color::Spec(Rgb {
            r: 200,
            g: 100,
            b: 0,
        });
        let (f, _) = p.cell_colors(white, Color::Named(NamedColor::Background), Flags::DIM, &o);
        assert_eq!(
            f,
            Rgb {
                r: 132,
                g: 66,
                b: 0
            }
        );
    }
}
