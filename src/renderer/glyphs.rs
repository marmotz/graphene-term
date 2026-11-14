//! Glyph cache of the terminal grid (CPU side, no GPU access).
//!
//! A terminal cell holds one character, so no shaping is needed per frame: each
//! `(char, bold, italic)` is resolved once by shaping that single character (font
//! fallback included), rasterized once and placed in the [`Atlas`]. The pixels of newly
//! placed glyphs wait in [`Glyphs::take_uploads`] for the GPU side to copy them.
//!
//! [`Glyphs`] also owns the font system and the cell metrics, and the interface font:
//! interface text uses a sans-serif font at a fixed size per [`UiFont`], independent of
//! the terminal font and its zoom. Their glyphs share the cache and the atlas with the
//! grid's, under keys with `ui` set.

use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, SwashContent, Weight,
    fontdb,
};

use super::atlas::{Atlas, Page, Region};
use crate::layout::CellSize;

/// Side of each atlas page, in pixels.
const ATLAS_SIZE: u32 = 1024;

/// Size of the interface font of the chrome (tab bar, dialog), in logical pixels.
pub const UI_FONT_SIZE: f32 = 13.0;

/// Size of the command palette's font, in logical pixels: larger than the chrome's, the
/// palette being read and typed in.
pub const PALETTE_FONT_SIZE: f32 = 16.0;

/// Interface font, by where its text is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiFont {
    /// Tab bar and dialog, at [`UI_FONT_SIZE`].
    Chrome,
    /// Command palette, at [`PALETTE_FONT_SIZE`].
    Palette,
}

impl UiFont {
    /// Size of the font, in logical pixels.
    pub fn size(self) -> f32 {
        match self {
            Self::Chrome => UI_FONT_SIZE,
            Self::Palette => PALETTE_FONT_SIZE,
        }
    }
}

/// Line height of a font of `font_size` pixels, for the grid and the interface alike.
fn line_height(font_size: f32) -> f32 {
    (font_size * 1.3).ceil()
}

/// What a cell asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub c: char,
    pub bold: bool,
    pub italic: bool,
    /// Interface font (sans-serif) instead of the grid font.
    pub ui: Option<UiFont>,
}

impl GlyphKey {
    /// A character of the interface font `font`, regular.
    pub fn ui(font: UiFont, c: char) -> Self {
        Self {
            c,
            bold: false,
            italic: false,
            ui: Some(font),
        }
    }
}

/// A line of interface text shaped with the interface font: each character with its
/// x offset from the start of the line, and the width of the line.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiLine {
    pub glyphs: Vec<(f32, char)>,
    pub width: f32,
}

/// A rasterized glyph placed in the atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub page: Page,
    pub region: Region,
    /// Offset of the bitmap from the cell origin (top-left corner), in pixels. A glyph
    /// wider than the cell overflows it instead of shifting its neighbors.
    pub left: i32,
    pub top: i32,
}

/// Pixels of a newly placed glyph, to copy into `region` of the `page` texture. Rows are
/// tightly packed (`region.width * page.bytes_per_pixel()` bytes each).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub page: Page,
    pub region: Region,
    pub data: Vec<u8>,
}

pub struct Glyphs {
    font_system: FontSystem,
    swash_cache: SwashCache,
    /// Font family name, see [`resolve_family`]; `None` uses cosmic-text's monospace lookup.
    family: Option<String>,
    metrics: Metrics,
    cell: CellSize,
    /// Scale factor of the window, applied to the interface fonts.
    ui_scale: f32,
    /// `None` for characters with nothing to draw (space, empty bitmap).
    cache: HashMap<GlyphKey, Option<Glyph>>,
    atlas: Atlas,
    uploads: Vec<Upload>,
    /// Bumped each time the atlas is reset: glyphs obtained before are stale.
    generation: u64,
}

fn family(name: &Option<String>) -> Family<'_> {
    match name {
        Some(name) => Family::Name(name),
        None => Family::Monospace,
    }
}

/// Family to shape with: the configured one, else the system monospace family by name.
///
/// Shaping with [`Family::Monospace`] makes cosmic-text load every monospaced font of
/// the system (each mapped and parsed, ~20 MB with a hundred of them) as soon as a
/// character is missing from the default font, which happens on the first shaping.
/// A named family falls back through the script fallback lists instead, loading only
/// the fonts that actually cover the character. `None` when the system monospace family
/// is not installed, keeping cosmic-text's own lookup.
fn resolve_family(db: &fontdb::Database, configured: Option<String>) -> Option<String> {
    if configured.is_some() {
        return configured;
    }
    let name = db.family_name(&Family::Monospace);
    db.faces()
        .any(|face| face.families.iter().any(|(family, _)| family == name))
        .then(|| name.to_owned())
}

impl Glyphs {
    pub fn new(family: Option<String>, font_size: f32) -> Self {
        Self::with_atlas_size(family, font_size, ATLAS_SIZE)
    }

    pub(super) fn with_atlas_size(family: Option<String>, font_size: f32, atlas_size: u32) -> Self {
        let mut font_system = FontSystem::new();
        let family = resolve_family(font_system.db(), family);
        let (metrics, cell) = Self::measure(&mut font_system, &family, font_size);
        Self {
            font_system,
            swash_cache: SwashCache::new(),
            family,
            metrics,
            cell,
            ui_scale: 1.0,
            cache: HashMap::new(),
            atlas: Atlas::new(atlas_size),
            uploads: Vec::new(),
            generation: 0,
        }
    }

    /// Computes the cell size from the advance of a reference glyph.
    fn measure(
        font_system: &mut FontSystem,
        name: &Option<String>,
        font_size: f32,
    ) -> (Metrics, CellSize) {
        let line_height = line_height(font_size);
        let metrics = Metrics::new(font_size, line_height);
        let mut buffer = Buffer::new(font_system, metrics);
        buffer.set_text(
            "M",
            &Attrs::new().family(family(name)),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(font_system, false);
        // Keep the exact advance: rounding it would make text drift off the grid.
        let width = buffer
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first().map(|g| g.w))
            .unwrap_or(font_size * 0.6)
            .max(1.0);
        (
            metrics,
            CellSize {
                width,
                height: line_height,
            },
        )
    }

    pub fn cell_size(&self) -> CellSize {
        self.cell
    }

    pub fn metrics(&self) -> Metrics {
        self.metrics
    }

    /// Changes the font size: new metrics, and every cached glyph is dropped.
    pub fn set_font_size(&mut self, font_size: f32) {
        let (metrics, cell) = Self::measure(&mut self.font_system, &self.family, font_size);
        self.metrics = metrics;
        self.cell = cell;
        self.clear();
    }

    /// Changes the font family (`None`: the system monospace font) and size: new
    /// metrics, and every cached glyph is dropped.
    pub fn set_font(&mut self, family: Option<String>, font_size: f32) {
        self.family = resolve_family(self.font_system.db(), family);
        self.set_font_size(font_size);
    }

    /// Scales the interface font to a window of scale factor `scale`; the cache is
    /// dropped when its size changes.
    pub fn set_ui_scale(&mut self, scale: f32) {
        if scale != self.ui_scale {
            self.ui_scale = scale;
            self.clear();
        }
    }

    /// Scale factor the interface fonts are scaled with.
    pub fn ui_scale(&self) -> f32 {
        self.ui_scale
    }

    /// Line height of the interface font `font`, in pixels.
    pub fn ui_line_height(&self, font: UiFont) -> f32 {
        self.ui_metrics(font).line_height
    }

    /// Size of the interface font `font`, in pixels.
    pub fn ui_font_size(&self, font: UiFont) -> f32 {
        self.ui_metrics(font).font_size
    }

    /// Metrics of the interface font `font`, scaled to the window.
    fn ui_metrics(&self, font: UiFont) -> Metrics {
        let font_size = font.size() * self.ui_scale;
        Metrics::new(font_size, line_height(font_size))
    }

    /// Shapes `text` on one line with the interface font `font`. A cluster of several
    /// characters (ligature, combining mark) keeps its first character only.
    pub fn shape_ui(&mut self, font: UiFont, text: &str) -> UiLine {
        let metrics = self.ui_metrics(font);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_text(
            text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);
        let Some(run) = buffer.layout_runs().next() else {
            return UiLine::default();
        };
        let glyphs = run
            .glyphs
            .iter()
            .filter_map(|glyph| Some((glyph.x, text.get(glyph.start..glyph.end)?.chars().next()?)))
            .collect();
        UiLine {
            glyphs,
            width: run.line_w,
        }
    }

    /// Drops every cached glyph and frees the atlas.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.atlas.reset();
        self.uploads.clear();
        self.generation += 1;
    }

    /// Number of atlas resets so far. When it changed, glyphs obtained before (and the
    /// atlas textures) are stale.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Side of each atlas page, in pixels.
    pub fn atlas_size(&self) -> u32 {
        self.atlas.size()
    }

    /// Pixels placed since the last call, to copy into the atlas textures.
    pub fn take_uploads(&mut self) -> Vec<Upload> {
        std::mem::take(&mut self.uploads)
    }

    /// The font system and base attributes, to shape text the way the cache does.
    #[cfg(test)]
    fn text_system(&mut self) -> (&mut FontSystem, Attrs<'_>) {
        (
            &mut self.font_system,
            Attrs::new().family(family(&self.family)),
        )
    }

    /// The glyph of `key`, rasterized and placed in the atlas on first use. `None` when
    /// there is nothing to draw.
    pub fn get(&mut self, key: GlyphKey) -> Option<Glyph> {
        if let Some(glyph) = self.cache.get(&key) {
            return *glyph;
        }
        let glyph = self.rasterize(key);
        self.cache.insert(key, glyph);
        glyph
    }

    fn rasterize(&mut self, key: GlyphKey) -> Option<Glyph> {
        let (family, metrics) = match key.ui {
            Some(font) => (Family::SansSerif, self.ui_metrics(font)),
            None => (family(&self.family), self.metrics),
        };
        let mut attrs = Attrs::new().family(family);
        if key.bold {
            attrs = attrs.weight(Weight::BOLD);
        }
        if key.italic {
            attrs = attrs.style(Style::Italic);
        }
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        let mut text = [0; 4];
        // Advanced shaping keeps the font fallback for characters missing from the font.
        buffer.set_text(
            key.c.encode_utf8(&mut text),
            &attrs,
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);
        let run = buffer.layout_runs().next()?;
        let physical = run.glyphs.first()?.physical((0.0, 0.0), 1.0);
        let line_y = run.line_y.round() as i32;
        let image = self
            .swash_cache
            .get_image_uncached(&mut self.font_system, physical.cache_key)?;
        let placement = image.placement;
        if placement.width == 0 || placement.height == 0 {
            return None;
        }
        let page = match image.content {
            SwashContent::Color => Page::Color,
            SwashContent::Mask | SwashContent::SubpixelMask => Page::Mask,
        };
        let region = self.allocate(page, placement.width, placement.height)?;
        let data = match (image.content, page) {
            // Not produced by cosmic-text's rasterizer (alpha format); keep one channel.
            (SwashContent::SubpixelMask, _) => image.data.chunks(4).map(|px| px[3]).collect(),
            _ => image.data,
        };
        self.uploads.push(Upload { page, region, data });
        Some(Glyph {
            page,
            region,
            left: physical.x + placement.left,
            top: line_y + physical.y - placement.top,
        })
    }

    /// Reserves an atlas region, resetting the atlas (and the cache) when the page is
    /// full. `None` when the glyph does not fit even an empty page.
    fn allocate(&mut self, page: Page, width: u32, height: u32) -> Option<Region> {
        if let Some(region) = self.atlas.allocate(page, width, height) {
            return Some(region);
        }
        log::debug!("glyph atlas full, resetting it");
        self.clear();
        let region = self.atlas.allocate(page, width, height);
        if region.is_none() {
            log::warn!("glyph of {width}x{height} px does not fit the atlas, skipped");
        }
        region
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT_SIZE: f32 = 14.0;

    fn key(c: char) -> GlyphKey {
        GlyphKey {
            c,
            bold: false,
            italic: false,
            ui: None,
        }
    }

    #[test]
    fn configured_family_is_kept() {
        let db = fontdb::Database::new();
        assert_eq!(
            resolve_family(&db, Some("Fira Code".into())),
            Some("Fira Code".into())
        );
    }

    #[test]
    fn system_monospace_family_is_resolved_to_its_name() {
        let mut db = FontSystem::new().db().clone();
        let installed = db
            .faces()
            .find_map(|face| face.families.first())
            .map(|(name, _)| name.clone())
            .expect("tests need at least one system font");
        db.set_monospace_family(installed.clone());
        assert_eq!(resolve_family(&db, None), Some(installed));
    }

    #[test]
    fn missing_system_monospace_family_keeps_the_generic_lookup() {
        let mut db = fontdb::Database::new();
        db.set_monospace_family("No Such Family");
        assert_eq!(resolve_family(&db, None), None);
    }

    #[test]
    fn cache_hit_returns_the_same_glyph_without_new_upload() {
        let mut glyphs = Glyphs::new(None, FONT_SIZE);
        let first = glyphs.get(key('a')).expect("'a' has pixels");
        assert_eq!(glyphs.take_uploads().len(), 1);
        assert_eq!(glyphs.get(key('a')), Some(first));
        assert!(glyphs.take_uploads().is_empty());
    }

    #[test]
    fn upload_matches_the_region() {
        let mut glyphs = Glyphs::new(None, FONT_SIZE);
        let glyph = glyphs.get(key('W')).unwrap();
        let uploads = glyphs.take_uploads();
        let upload = &uploads[0];
        assert_eq!((upload.page, upload.region), (glyph.page, glyph.region));
        assert_eq!(glyph.page, Page::Mask);
        let expected = glyph.region.width * glyph.region.height * glyph.page.bytes_per_pixel();
        assert_eq!(upload.data.len(), expected as usize);
    }

    #[test]
    fn glyph_sits_inside_its_cell() {
        let mut glyphs = Glyphs::new(None, FONT_SIZE);
        let cell = glyphs.cell_size();
        let glyph = glyphs.get(key('M')).unwrap();
        assert!(glyph.left >= -1 && glyph.top >= 0, "{glyph:?}");
        assert!(glyph.left as f32 + glyph.region.width as f32 <= cell.width + 1.0);
        assert!(glyph.top as f32 + glyph.region.height as f32 <= cell.height);
    }

    #[test]
    fn bold_and_italic_are_distinct_entries() {
        let mut glyphs = Glyphs::new(None, FONT_SIZE);
        let regular = glyphs.get(key('a')).unwrap();
        let bold = glyphs
            .get(GlyphKey {
                bold: true,
                ..key('a')
            })
            .unwrap();
        let italic = glyphs
            .get(GlyphKey {
                italic: true,
                ..key('a')
            })
            .unwrap();
        assert_ne!(regular.region, bold.region);
        assert_ne!(regular.region, italic.region);
        assert_ne!(bold.region, italic.region);
        assert_eq!(glyphs.take_uploads().len(), 3);
    }

    #[test]
    fn space_has_nothing_to_draw() {
        let mut glyphs = Glyphs::new(None, FONT_SIZE);
        assert_eq!(glyphs.get(key(' ')), None);
        assert!(glyphs.take_uploads().is_empty());
    }

    #[test]
    fn missing_glyph_falls_back_without_panic() {
        let mut glyphs = Glyphs::new(None, FONT_SIZE);
        // Private use area: no font is expected to cover it.
        let _ = glyphs.get(key('\u{10FFFD}'));
        // Box drawing and braille usually come from a fallback font.
        let _ = glyphs.get(key('─'));
        let _ = glyphs.get(key('⣿'));
        assert!(glyphs.get(key('a')).is_some());
    }

    #[test]
    fn metrics_match_the_reference_glyph() {
        let mut glyphs = Glyphs::new(None, FONT_SIZE);
        let cell = glyphs.cell_size();
        assert_eq!(cell.height, (FONT_SIZE * 1.3).ceil());
        assert_eq!(glyphs.metrics(), Metrics::new(FONT_SIZE, cell.height));

        // Same computation as before the glyph cache: advance of a shaped "M".
        let (font_system, attrs) = glyphs.text_system();
        let mut buffer = Buffer::new(font_system, Metrics::new(FONT_SIZE, cell.height));
        buffer.set_text("M", &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(font_system, false);
        let advance = buffer.layout_runs().next().unwrap().glyphs[0].w;
        assert_eq!(cell.width, advance);
    }

    #[test]
    fn font_size_change_updates_metrics_and_clears_the_cache() {
        let mut glyphs = Glyphs::new(None, FONT_SIZE);
        let small = glyphs.get(key('a')).unwrap();
        let generation = glyphs.generation();
        glyphs.set_font_size(FONT_SIZE * 2.0);
        assert_eq!(glyphs.cell_size().height, (FONT_SIZE * 2.0 * 1.3).ceil());
        assert!(glyphs.generation() > generation);
        let large = glyphs.get(key('a')).unwrap();
        assert!(large.region.height > small.region.height);
        assert_eq!(glyphs.take_uploads().len(), 1);
    }

    #[test]
    fn font_change_updates_the_family_and_metrics_and_clears_the_cache() {
        let mut glyphs = Glyphs::new(Some("No Such Family".into()), FONT_SIZE);
        glyphs.get(key('a')).unwrap();
        let generation = glyphs.generation();
        glyphs.set_font(None, FONT_SIZE * 2.0);
        assert_eq!(glyphs.family, resolve_family(glyphs.font_system.db(), None));
        assert_eq!(glyphs.cell_size().height, (FONT_SIZE * 2.0 * 1.3).ceil());
        assert!(glyphs.generation() > generation);
    }

    #[test]
    fn full_atlas_resets_and_drops_the_cache() {
        let mut glyphs = Glyphs::with_atlas_size(None, FONT_SIZE, 48);
        let first = glyphs.get(key('A')).unwrap();
        let generation = glyphs.generation();
        for c in 'B'..='Z' {
            glyphs.get(key(c));
        }
        assert!(glyphs.generation() > generation, "atlas never filled up");
        // Uploads from before the reset are gone; the cache was dropped too, so 'A' is
        // rasterized and uploaded again.
        let uploads_after_reset = glyphs.take_uploads().len();
        assert!(uploads_after_reset < 25);
        let again = glyphs.get(key('A')).unwrap();
        assert_eq!(glyphs.take_uploads().len(), 1);
        assert_eq!(again.region.width, first.region.width);
    }

    #[test]
    fn glyph_larger_than_the_atlas_is_skipped() {
        let mut glyphs = Glyphs::with_atlas_size(None, FONT_SIZE, 4);
        assert_eq!(glyphs.get(key('M')), None);
        assert!(glyphs.take_uploads().is_empty());
    }

    #[test]
    fn ui_font_has_a_fixed_size_scaled_to_the_window() {
        let mut glyphs = Glyphs::new(None, FONT_SIZE);
        assert_eq!(glyphs.ui_font_size(UiFont::Chrome), UI_FONT_SIZE);
        glyphs.set_font_size(FONT_SIZE * 3.0);
        assert_eq!(glyphs.ui_font_size(UiFont::Chrome), UI_FONT_SIZE);
        let generation = glyphs.generation();
        glyphs.set_ui_scale(2.0);
        assert_eq!(glyphs.ui_font_size(UiFont::Chrome), UI_FONT_SIZE * 2.0);
        assert_eq!(
            glyphs.ui_line_height(UiFont::Chrome),
            (UI_FONT_SIZE * 2.0 * 1.3).ceil()
        );
        assert!(glyphs.generation() > generation);
        // Same scale: the cache is kept.
        let generation = glyphs.generation();
        glyphs.set_ui_scale(2.0);
        assert_eq!(glyphs.generation(), generation);
    }

    #[test]
    fn palette_font_is_larger_than_the_chrome_font() {
        let mut glyphs = Glyphs::new(None, FONT_SIZE);
        glyphs.set_ui_scale(2.0);
        assert_eq!(
            glyphs.ui_font_size(UiFont::Palette),
            PALETTE_FONT_SIZE * 2.0
        );
        assert!(glyphs.ui_line_height(UiFont::Palette) > glyphs.ui_line_height(UiFont::Chrome));
        let chrome = glyphs.get(GlyphKey::ui(UiFont::Chrome, 'M')).unwrap();
        let palette = glyphs.get(GlyphKey::ui(UiFont::Palette, 'M')).unwrap();
        assert!(palette.region.height > chrome.region.height);
        assert!(
            glyphs.shape_ui(UiFont::Palette, "abc").width
                > glyphs.shape_ui(UiFont::Chrome, "abc").width
        );
    }

    #[test]
    fn ui_glyphs_are_distinct_from_grid_glyphs() {
        let mut glyphs = Glyphs::new(None, 30.0);
        let grid = glyphs.get(key('a')).unwrap();
        let ui = glyphs.get(GlyphKey::ui(UiFont::Chrome, 'a')).unwrap();
        assert_ne!(grid.region, ui.region);
        // The interface font is smaller than this grid font.
        assert!(ui.region.height < grid.region.height);
        assert_eq!(glyphs.take_uploads().len(), 2);
    }

    #[test]
    fn shaped_ui_line_places_each_character() {
        let mut glyphs = Glyphs::new(None, FONT_SIZE);
        let line = glyphs.shape_ui(UiFont::Chrome, "ab c");
        let chars: Vec<char> = line.glyphs.iter().map(|&(_, c)| c).collect();
        assert_eq!(chars, ['a', 'b', ' ', 'c']);
        assert_eq!(line.glyphs[0].0, 0.0);
        assert!(line.glyphs.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(line.width > line.glyphs[3].0);
        assert_eq!(glyphs.shape_ui(UiFont::Chrome, ""), UiLine::default());
    }
}
