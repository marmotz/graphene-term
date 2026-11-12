//! Placement of rasterized glyphs in the glyph atlas (pure, no GPU access).
//!
//! The atlas has two square pages: one for alpha masks (`R8Unorm`) and one for color
//! glyphs such as emoji (`Rgba8Unorm`). Each page is filled by a shelf allocator: glyphs
//! are packed left to right on horizontal shelves, a new shelf opening below the last
//! one when no existing shelf fits. Nothing is ever freed: when a page is full, the
//! owner resets the whole atlas and drops its glyph cache (rare, a terminal draws few
//! distinct glyphs).

/// Empty pixels kept around each glyph so linear sampling never bleeds a neighbor in.
const GUTTER: u32 = 1;

/// Page of the atlas holding a glyph, which also tells how its pixels are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    /// 8-bit alpha mask, tinted with the cell color (`R8Unorm`).
    Mask,
    /// 32-bit RGBA bitmap drawn as is (`Rgba8Unorm`).
    Color,
}

impl Page {
    /// Bytes per pixel of the page texture.
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Page::Mask => 1,
            Page::Color => 4,
        }
    }
}

/// Pixel rectangle of a glyph inside its page, gutter excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
struct Shelf {
    y: u32,
    height: u32,
    /// First free column.
    next_x: u32,
}

/// Shelf allocator of one square page.
#[derive(Debug, Clone)]
pub struct ShelfAllocator {
    size: u32,
    shelves: Vec<Shelf>,
    /// First row below the last shelf.
    next_y: u32,
}

impl ShelfAllocator {
    pub fn new(size: u32) -> Self {
        Self {
            size,
            shelves: Vec::new(),
            next_y: 0,
        }
    }

    /// Side of the page, in pixels.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Reserves a `width` x `height` region. `None` when the page has no room left.
    pub fn allocate(&mut self, width: u32, height: u32) -> Option<Region> {
        let (w, h) = (width + 2 * GUTTER, height + 2 * GUTTER);
        if w > self.size || h > self.size {
            return None;
        }
        // Best fit: the lowest shelf tall enough that still has room, so tall shelves
        // are not wasted on small glyphs.
        let best = self
            .shelves
            .iter_mut()
            .filter(|shelf| shelf.height >= h && self.size - shelf.next_x >= w)
            .min_by_key(|shelf| shelf.height);
        let shelf = match best {
            Some(shelf) => shelf,
            None => {
                if self.size - self.next_y < h {
                    return None;
                }
                self.shelves.push(Shelf {
                    y: self.next_y,
                    height: h,
                    next_x: 0,
                });
                self.next_y += h;
                self.shelves.last_mut().expect("shelf just pushed")
            }
        };
        let region = Region {
            x: shelf.next_x + GUTTER,
            y: shelf.y + GUTTER,
            width,
            height,
        };
        shelf.next_x += w;
        Some(region)
    }

    /// Frees every region.
    pub fn reset(&mut self) {
        self.shelves.clear();
        self.next_y = 0;
    }
}

/// The two pages of the glyph atlas.
#[derive(Debug, Clone)]
pub struct Atlas {
    mask: ShelfAllocator,
    color: ShelfAllocator,
}

impl Atlas {
    /// Creates an atlas whose pages are `size` x `size` pixels.
    pub fn new(size: u32) -> Self {
        Self {
            mask: ShelfAllocator::new(size),
            color: ShelfAllocator::new(size),
        }
    }

    /// Side of each page, in pixels.
    pub fn size(&self) -> u32 {
        self.mask.size()
    }

    /// Reserves a region on `page`. `None` when that page is full.
    pub fn allocate(&mut self, page: Page, width: u32, height: u32) -> Option<Region> {
        match page {
            Page::Mask => self.mask.allocate(width, height),
            Page::Color => self.color.allocate(width, height),
        }
    }

    /// Frees every region of both pages.
    pub fn reset(&mut self) {
        self.mask.reset();
        self.color.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlaps(a: Region, b: Region) -> bool {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    }

    #[test]
    fn regions_do_not_overlap_and_stay_in_the_page() {
        let mut page = ShelfAllocator::new(64);
        let mut regions = Vec::new();
        for (w, h) in [(10, 12), (7, 12), (10, 20), (5, 5), (12, 12), (30, 8)] {
            regions.push(page.allocate(w, h).expect("room left"));
        }
        for (i, a) in regions.iter().enumerate() {
            assert!(a.x + a.width < 64 && a.y + a.height < 64, "{a:?}");
            for b in &regions[i + 1..] {
                assert!(!overlaps(*a, *b), "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn regions_keep_a_gutter() {
        let mut page = ShelfAllocator::new(64);
        let a = page.allocate(10, 10).unwrap();
        let b = page.allocate(10, 10).unwrap();
        assert_eq!((a.x, a.y), (GUTTER, GUTTER));
        assert_eq!(b.x, a.x + a.width + 2 * GUTTER);
        assert_eq!(b.y, a.y);
    }

    #[test]
    fn small_glyphs_reuse_a_taller_shelf() {
        let mut page = ShelfAllocator::new(64);
        let tall = page.allocate(10, 20).unwrap();
        let small = page.allocate(10, 8).unwrap();
        assert_eq!(small.y, tall.y);
    }

    #[test]
    fn full_page_refuses_then_reset_frees_it() {
        let mut page = ShelfAllocator::new(32);
        let mut count = 0;
        while page.allocate(8, 8).is_some() {
            count += 1;
        }
        // 10x10 slots with the gutter: 3 per shelf, 3 shelves.
        assert_eq!(count, 9);
        page.reset();
        assert_eq!(
            page.allocate(8, 8),
            Some(Region {
                x: GUTTER,
                y: GUTTER,
                width: 8,
                height: 8
            })
        );
    }

    #[test]
    fn glyph_larger_than_the_page_is_refused() {
        let mut page = ShelfAllocator::new(16);
        assert_eq!(page.allocate(15, 4), None);
        assert_eq!(page.allocate(4, 15), None);
        assert!(page.allocate(14, 14).is_some());
    }

    #[test]
    fn atlas_pages_are_independent() {
        let mut atlas = Atlas::new(16);
        assert!(atlas.allocate(Page::Mask, 14, 14).is_some());
        assert_eq!(atlas.allocate(Page::Mask, 1, 1), None);
        assert!(atlas.allocate(Page::Color, 14, 14).is_some());
        atlas.reset();
        assert!(atlas.allocate(Page::Mask, 14, 14).is_some());
        assert!(atlas.allocate(Page::Color, 14, 14).is_some());
    }

    #[test]
    fn bytes_per_pixel_match_the_page_formats() {
        assert_eq!(Page::Mask.bytes_per_pixel(), 1);
        assert_eq!(Page::Color.bytes_per_pixel(), 4);
    }
}
