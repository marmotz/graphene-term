//! macOS taskbar backend: a progress bar drawn over the application icon in the Dock.
//!
//! While a progress shows, the Dock tile's content view is the application icon with a
//! rounded track and bar on top of it, and the tile is redrawn at each change; without
//! progress, the content view is removed and the Dock draws the plain icon again. The
//! counter of unseen notices (the badge label, set by the notification backend) is drawn
//! by the Dock above any content view, so both coexist.
//!
//! An error or a pause takes the color it has in the interface; the Dock tile is not
//! animated, so an indeterminate progress is a full bar, dimmed.
//!
//! AppKit wants the Dock on the main thread: the backend runs on the UI thread. The
//! layout ([`dock_bar`]) is compiled on every system so that it is tested everywhere.

use alacritty_terminal::vte::ansi::Rgb;

use super::Shown;
use crate::progress::{ProgressColors, ProgressState};

/// Space left on each side of the bar, as a share of the tile width.
const SIDE_INSET: f64 = 0.12;
/// Height of the bar, as a share of the tile height.
const BAR_HEIGHT: f64 = 0.1;
/// Space under the bar, as a share of the tile height.
const BOTTOM_INSET: f64 = 0.08;
/// Opacity of the track under the bar: dark enough to read over any icon.
const TRACK_OPACITY: f64 = 0.6;
/// Opacity of the full bar of an indeterminate progress.
const INDETERMINATE_OPACITY: f64 = 0.5;

/// A rectangle of the Dock tile, in points, from its bottom-left corner as AppKit
/// counts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A color and its opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fill {
    pub rgb: Rgb,
    pub alpha: f64,
}

impl Fill {
    /// Red, green, blue and alpha, from 0 to 1.
    pub fn components(self) -> [f64; 4] {
        let channel = |value: u8| f64::from(value) / 255.0;
        [
            channel(self.rgb.r),
            channel(self.rgb.g),
            channel(self.rgb.b),
            self.alpha,
        ]
    }
}

/// The progress bar of the Dock tile laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockBar {
    /// The whole length of the bar, translucent.
    pub track: Frame,
    pub track_fill: Fill,
    /// The filled part, from the left of the track.
    pub bar: Frame,
    pub bar_fill: Fill,
    /// Corner radius of the track and the bar: their ends are round.
    pub radius: f64,
}

/// The bar showing `shown` on a Dock tile `width` by `height` points, in the interface
/// `colors`.
pub fn dock_bar(width: f64, height: f64, shown: Shown, colors: &ProgressColors) -> DockBar {
    let track = Frame {
        x: width * SIDE_INSET,
        y: height * BOTTOM_INSET,
        width: width * (1.0 - 2.0 * SIDE_INSET),
        height: height * BAR_HEIGHT,
    };
    let (filled, alpha) = match shown.state {
        ProgressState::Indeterminate => (1.0, INDETERMINATE_OPACITY),
        _ => (f64::from(shown.value.min(100)) / 100.0, 1.0),
    };
    DockBar {
        track,
        track_fill: Fill {
            rgb: colors.track,
            alpha: TRACK_OPACITY,
        },
        bar: Frame {
            width: track.width * filled,
            ..track
        },
        bar_fill: Fill {
            rgb: colors.bar(shown.state),
            alpha,
        },
        radius: track.height / 2.0,
    }
}

#[cfg(target_os = "macos")]
pub use self::native::{Backend, spawn};

#[cfg(target_os = "macos")]
mod native {
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSBox, NSBoxType, NSColor, NSImageScaling, NSImageView, NSTitlePosition,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    use super::{DockBar, Fill, Frame, dock_bar};
    use crate::progress::ProgressColors;
    use crate::taskbar::{Shown, Taskbar};

    /// Starts the macOS backend. Must run on the main thread, like the event loop.
    pub fn spawn() -> Taskbar {
        let Some(main_thread) = MainThreadMarker::new() else {
            log::warn!("the Dock progress must start on the main thread");
            return Taskbar::null();
        };
        Taskbar::main_thread(Backend {
            main_thread,
            tile: None,
        })
    }

    /// Draws the progress on the Dock tile.
    pub struct Backend {
        main_thread: MainThreadMarker,
        /// The content view, built at the first progress and kept for the next ones.
        tile: Option<Tile>,
    }

    /// The application icon with the track and the bar on top of it.
    struct Tile {
        icon: Retained<NSImageView>,
        track: Retained<NSBox>,
        bar: Retained<NSBox>,
    }

    impl Backend {
        /// Shows `shown` in `colors` on the Dock tile, `None` giving back the plain icon.
        pub fn show(&mut self, shown: Option<Shown>, colors: &ProgressColors) {
            let dock = NSApplication::sharedApplication(self.main_thread).dockTile();
            let Some(shown) = shown else {
                dock.setContentView(None);
                dock.display();
                return;
            };
            if self.tile.is_none() {
                self.tile = Tile::new(self.main_thread);
            }
            let Some(tile) = &self.tile else {
                return;
            };
            let size = dock.size();
            tile.icon.setFrame(rect(Frame {
                x: 0.0,
                y: 0.0,
                width: size.width,
                height: size.height,
            }));
            tile.layout(&dock_bar(size.width, size.height, shown, colors));
            dock.setContentView(Some(&tile.icon));
            dock.display();
        }
    }

    impl Tile {
        /// The content view, `None` when the application has no icon to draw.
        fn new(main_thread: MainThreadMarker) -> Option<Self> {
            let Some(image) = NSApplication::sharedApplication(main_thread).applicationIconImage()
            else {
                log::debug!("no application icon: no progress on the Dock");
                return None;
            };
            let icon = NSImageView::imageViewWithImage(&image, main_thread);
            icon.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
            let track = rounded_box(main_thread);
            let bar = rounded_box(main_thread);
            icon.addSubview(&track);
            icon.addSubview(&bar);
            Some(Self { icon, track, bar })
        }

        /// Places and colors the track and the bar.
        fn layout(&self, layout: &DockBar) {
            for (view, frame, fill) in [
                (&self.track, layout.track, layout.track_fill),
                (&self.bar, layout.bar, layout.bar_fill),
            ] {
                view.setFrame(rect(frame));
                view.setCornerRadius(layout.radius);
                view.setFillColor(&color(fill));
                view.setHidden(frame.width <= 0.0);
            }
        }
    }

    /// A borderless box without title, filled with a single color.
    fn rounded_box(main_thread: MainThreadMarker) -> Retained<NSBox> {
        let view = NSBox::initWithFrame(NSBox::alloc(main_thread), NSRect::ZERO);
        view.setBoxType(NSBoxType::Custom);
        view.setTitlePosition(NSTitlePosition::NoTitle);
        view.setBorderWidth(0.0);
        view.setContentViewMargins(NSSize::new(0.0, 0.0));
        view
    }

    fn rect(frame: Frame) -> NSRect {
        NSRect::new(
            NSPoint::new(frame.x, frame.y),
            NSSize::new(frame.width, frame.height),
        )
    }

    fn color(fill: Fill) -> Retained<NSColor> {
        let [red, green, blue, alpha] = fill.components();
        NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors() -> ProgressColors {
        ProgressColors {
            normal: Rgb { r: 0, g: 0, b: 255 },
            error: Rgb { r: 255, g: 0, b: 0 },
            paused: Rgb {
                r: 255,
                g: 255,
                b: 0,
            },
            track: Rgb {
                r: 128,
                g: 128,
                b: 128,
            },
        }
    }

    fn shown(state: ProgressState, value: u8) -> Shown {
        Shown { state, value }
    }

    #[test]
    fn the_track_spans_the_bottom_of_the_tile() {
        let bar = dock_bar(100.0, 200.0, shown(ProgressState::Normal, 0), &colors());
        assert_eq!(
            bar.track,
            Frame {
                x: 12.0,
                y: 16.0,
                width: 76.0,
                height: 20.0,
            }
        );
        assert_eq!(bar.radius, 10.0);
        assert_eq!(
            bar.track_fill,
            Fill {
                rgb: colors().track,
                alpha: TRACK_OPACITY,
            }
        );
    }

    #[test]
    fn the_bar_fills_the_track_up_to_the_value() {
        for (value, width) in [(0, 0.0), (25, 19.0), (100, 76.0), (200, 76.0)] {
            let bar = dock_bar(100.0, 100.0, shown(ProgressState::Normal, value), &colors());
            assert_eq!(bar.bar.x, bar.track.x, "{value}");
            assert_eq!(bar.bar.y, bar.track.y, "{value}");
            assert_eq!(bar.bar.height, bar.track.height, "{value}");
            assert!((bar.bar.width - width).abs() < 1e-9, "{value}");
        }
    }

    #[test]
    fn errors_and_pauses_show_their_value_in_their_color() {
        let error = dock_bar(100.0, 100.0, shown(ProgressState::Error, 50), &colors());
        assert_eq!(
            error.bar_fill,
            Fill {
                rgb: colors().error,
                alpha: 1.0
            }
        );
        assert!((error.bar.width - 38.0).abs() < 1e-9);
        let paused = dock_bar(100.0, 100.0, shown(ProgressState::Paused, 50), &colors());
        assert_eq!(
            paused.bar_fill,
            Fill {
                rgb: colors().paused,
                alpha: 1.0
            }
        );
        let normal = dock_bar(100.0, 100.0, shown(ProgressState::Normal, 50), &colors());
        assert_eq!(
            normal.bar_fill,
            Fill {
                rgb: colors().normal,
                alpha: 1.0
            }
        );
    }

    #[test]
    fn an_indeterminate_progress_is_a_full_dimmed_bar() {
        let bar = dock_bar(
            100.0,
            100.0,
            shown(ProgressState::Indeterminate, 0),
            &colors(),
        );
        assert_eq!(bar.bar, bar.track);
        assert_eq!(
            bar.bar_fill,
            Fill {
                rgb: colors().normal,
                alpha: INDETERMINATE_OPACITY,
            }
        );
    }

    #[test]
    fn fill_components_are_fractions() {
        let fill = Fill {
            rgb: Rgb {
                r: 255,
                g: 0,
                b: 51,
            },
            alpha: 0.5,
        };
        assert_eq!(fill.components(), [1.0, 0.0, 0.2, 0.5]);
    }
}
