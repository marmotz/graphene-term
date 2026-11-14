//! Interface text (tab titles, later palette rows and dialog text): lines of the
//! interface font placed at pixel positions, truncated with `…` to a maximum width.
//!
//! Lines are shaped once per text and width, then turned into glyph instances drawn by
//! the cell pipeline from the shared atlas.

use std::collections::HashMap;

use super::glyphs::{GlyphKey, Glyphs, UiLine};
use super::scene::GlyphInstance;

/// Marks a truncated text.
pub const ELLIPSIS: char = '…';

/// Shaped lines kept before the cache is dropped, so changing texts do not pile up.
const CACHE_CAPACITY: usize = 512;

/// One line of interface text to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct UiTextItem {
    pub text: String,
    /// Top-left corner of the line box, in pixels.
    pub x: f32,
    pub y: f32,
    /// GPU color (see [`super::scene::gpu_color`]).
    pub color: [f32; 4],
    /// Width the text is truncated to, in pixels.
    pub max_width: f32,
}

/// `text`, or its longest prefix followed by [`ELLIPSIS`] that `measure` finds no wider
/// than `max_width` (the ellipsis alone when no prefix fits, nothing when even the
/// ellipsis does not fit). Trailing spaces before the ellipsis are dropped.
pub fn truncate(text: &str, max_width: f32, mut measure: impl FnMut(&str) -> f32) -> String {
    if measure(text) <= max_width {
        return text.to_owned();
    }
    let with_ellipsis = |count: usize| {
        let prefix: String = text.chars().take(count).collect();
        let mut cut = prefix.trim_end().to_owned();
        cut.push(ELLIPSIS);
        cut
    };
    let ellipsis = ELLIPSIS.to_string();
    if measure(&ellipsis) > max_width {
        return String::new();
    }
    // Binary search on the number of characters kept: widths grow with the prefix.
    let (mut fits, mut too_wide) = (0, text.chars().count());
    while too_wide - fits > 1 {
        let middle = (fits + too_wide) / 2;
        if measure(&with_ellipsis(middle)) <= max_width {
            fits = middle;
        } else {
            too_wide = middle;
        }
    }
    with_ellipsis(fits)
}

/// Cache of the shaped interface lines.
#[derive(Default)]
pub struct UiText {
    /// Lines by text and maximum width (bits of the `f32`).
    lines: HashMap<(String, u32), UiLine>,
    /// Interface font size the cached lines were shaped with.
    font_size: f32,
}

impl UiText {
    /// The line of `text` truncated to `max_width`, shaped with the interface font.
    fn line(&mut self, glyphs: &mut Glyphs, text: &str, max_width: f32) -> &UiLine {
        if self.font_size != glyphs.ui_font_size() || self.lines.len() >= CACHE_CAPACITY {
            self.lines.clear();
            self.font_size = glyphs.ui_font_size();
        }
        self.lines
            .entry((text.to_owned(), max_width.to_bits()))
            .or_insert_with(|| {
                let shown = truncate(text, max_width, |t| glyphs.shape_ui(t).width);
                glyphs.shape_ui(&shown)
            })
    }

    /// Glyph instances of `items`, appended to `out`; spaces draw nothing.
    pub fn glyphs(
        &mut self,
        glyphs: &mut Glyphs,
        items: &[UiTextItem],
        out: &mut Vec<GlyphInstance>,
    ) {
        for item in items {
            let line = self.line(glyphs, &item.text, item.max_width);
            out.extend(
                line.glyphs
                    .iter()
                    .filter(|&&(_, c)| !c.is_whitespace())
                    .map(|&(x, c)| GlyphInstance {
                        origin: [item.x + x, item.y],
                        key: GlyphKey::ui(c),
                        color: item.color,
                    }),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character 10 px wide.
    fn width(text: &str) -> f32 {
        text.chars().count() as f32 * 10.0
    }

    #[test]
    fn text_that_fits_is_kept() {
        assert_eq!(truncate("hello", 50.0, width), "hello");
        assert_eq!(truncate("", 0.0, width), "");
    }

    #[test]
    fn long_text_is_cut_with_an_ellipsis() {
        assert_eq!(truncate("hello world", 50.0, width), "hell…");
        assert_eq!(truncate("hello world", 49.0, width), "hel…");
        // The space before the cut is dropped.
        assert_eq!(truncate("hello world", 70.0, width), "hello…");
    }

    #[test]
    fn narrow_width_keeps_the_ellipsis_alone_or_nothing() {
        assert_eq!(truncate("hello", 10.0, width), "…");
        assert_eq!(truncate("hello", 9.0, width), "");
    }

    #[test]
    fn multibyte_characters_are_cut_on_character_boundaries() {
        assert_eq!(truncate("ééééé", 30.0, width), "éé…");
    }

    fn item(text: &str, max_width: f32) -> UiTextItem {
        UiTextItem {
            text: text.to_owned(),
            x: 100.0,
            y: 20.0,
            color: [1.0; 4],
            max_width,
        }
    }

    #[test]
    fn items_become_ui_glyphs_at_their_position() {
        let mut glyphs = Glyphs::new(None, 14.0);
        let mut text = UiText::default();
        let mut out = Vec::new();
        text.glyphs(&mut glyphs, &[item("a b", 1000.0)], &mut out);
        let chars: Vec<char> = out.iter().map(|g| g.key.c).collect();
        assert_eq!(chars, ['a', 'b']);
        assert!(out.iter().all(|g| g.key.ui && g.origin[1] == 20.0));
        assert_eq!(out[0].origin[0], 100.0);
        assert!(out[1].origin[0] > out[0].origin[0]);
    }

    #[test]
    fn items_are_truncated_to_their_width() {
        let mut glyphs = Glyphs::new(None, 14.0);
        let full = glyphs.shape_ui("a long tab title").width;
        let mut text = UiText::default();
        let mut out = Vec::new();
        text.glyphs(
            &mut glyphs,
            &[item("a long tab title", full / 2.0)],
            &mut out,
        );
        assert_eq!(out.last().map(|g| g.key.c), Some(ELLIPSIS));
        assert!(out.len() < "a long tab title".len());
    }
}
