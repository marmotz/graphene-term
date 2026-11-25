//! The quick terminal: a window attached to a screen edge, shown and hidden by
//! `graphene --quick-toggle`, held by a single resident `graphene --quick` process.
//!
//! - [`autostart`]: the session autostart entry that starts the resident process.
//! - [`control`]: single instance lock, command socket and the `--quick-toggle` client.
//! - This module: the commands of the socket and the placement of the window, computed
//!   without a window so that it is tested everywhere.

pub mod autostart;
pub mod control;
#[cfg(target_os = "macos")]
pub mod macos;

use std::time::{Duration, Instant};

use crate::config::{QuickTerminalEdge, QuickTerminalScreen};

/// Time between two positions of a slide.
pub const SLIDE_FRAME: Duration = Duration::from_millis(16);

/// How long after losing the focus the quick terminal hides: it absorbs the brief focus
/// losses at the time it is shown, and the focus taken by the settings window.
pub const BLUR_DELAY: Duration = Duration::from_millis(150);

/// A command of the quick terminal process, one line on its socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickCommand {
    /// Shows the window when hidden, hides it when shown.
    Toggle,
    Show,
    Hide,
}

impl QuickCommand {
    /// The line sent on the socket, without its newline.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Show => "show",
            Self::Hide => "hide",
        }
    }

    /// Reads a line of the socket; surrounding whitespace is ignored.
    pub fn parse(line: &str) -> Option<Self> {
        match line.trim() {
            "toggle" => Some(Self::Toggle),
            "show" => Some(Self::Show),
            "hide" => Some(Self::Hide),
            _ => None,
        }
    }
}

/// A rectangle of the desktop, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    fn contains(self, (x, y): (i32, i32)) -> bool {
        x >= self.x && y >= self.y && i64::from(x) < self.right() && i64::from(y) < self.bottom()
    }

    /// The rectangle without `insets` on its sides, never smaller than one pixel.
    #[cfg_attr(
        all(not(target_os = "macos"), not(test)),
        allow(dead_code, reason = "used on X11 by the work area, wired in #261")
    )]
    pub fn inset(self, insets: Insets) -> Self {
        let width = self.width.saturating_sub(insets.left + insets.right).max(1);
        let height = self
            .height
            .saturating_sub(insets.top + insets.bottom)
            .max(1);
        Self {
            x: self.x.saturating_add_unsigned(insets.left),
            y: self.y.saturating_add_unsigned(insets.top),
            width,
            height,
        }
    }

    fn right(self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    fn bottom(self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }

    /// The part of the desktop both rectangles cover, `None` when they do not overlap.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "read by the X11 work area, wired in #261")
    )]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > i64::from(x) && bottom > i64::from(y)).then(|| Self {
            x,
            y,
            width: (right - i64::from(x)) as u32,
            height: (bottom - i64::from(y)) as u32,
        })
    }

    fn area(self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

/// Space taken on each side of a screen by the desktop (menu bar, Dock, panels), in
/// pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Insets {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

/// A screen as the placement sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    pub name: Option<String>,
    /// The whole screen: the cursor over a panel is still on it.
    pub rect: PixelRect,
    /// The part of the screen the desktop panels leave, where the window goes.
    pub work_area: PixelRect,
}

/// Rectangles from an X11 list of `x, y, width, height` cardinals, such as
/// `_GTK_WORKAREAS_D<n>`; an incomplete trailing group is ignored.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "read by the X11 work area, wired in #261")
)]
pub fn rects_from_cardinals(values: &[u32]) -> Vec<PixelRect> {
    let (groups, _) = values.as_chunks::<4>();
    groups
        .iter()
        .map(|&[x, y, width, height]| PixelRect {
            // X11 coordinates are signed 32-bit values carried as cardinals.
            x: x as i32,
            y: y as i32,
            width,
            height,
        })
        .collect()
}

/// The work area of `monitor` among the per-screen `areas` of `_GTK_WORKAREAS_D<n>`: the
/// one that overlaps it most, clipped to it. `None` when none overlaps it.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "read by the X11 work area, wired in #261")
)]
pub fn gtk_work_area(monitor: PixelRect, areas: &[PixelRect]) -> Option<PixelRect> {
    areas
        .iter()
        .filter_map(|area| monitor.intersection(*area))
        .max_by_key(|clipped| clipped.area())
}

/// Space a window reserves against the edges of the root window (`_NET_WM_STRUT_PARTIAL`):
/// the thickness of each band, and the range it spans along its edge, ends included.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "read by the X11 work area, wired in #261")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Strut {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
    pub left_start_y: u32,
    pub left_end_y: u32,
    pub right_start_y: u32,
    pub right_end_y: u32,
    pub top_start_x: u32,
    pub top_end_x: u32,
    pub bottom_start_x: u32,
    pub bottom_end_x: u32,
}

#[cfg_attr(
    not(test),
    allow(dead_code, reason = "read by the X11 work area, wired in #261")
)]
impl Strut {
    /// The 12 cardinals of `_NET_WM_STRUT_PARTIAL`, in their order.
    pub fn from_partial(values: &[u32; 12]) -> Self {
        let [
            left,
            right,
            top,
            bottom,
            left_start_y,
            left_end_y,
            right_start_y,
            right_end_y,
            top_start_x,
            top_end_x,
            bottom_start_x,
            bottom_end_x,
        ] = *values;
        Self {
            left,
            right,
            top,
            bottom,
            left_start_y,
            left_end_y,
            right_start_y,
            right_end_y,
            top_start_x,
            top_end_x,
            bottom_start_x,
            bottom_end_x,
        }
    }

    /// The 4 cardinals of `_NET_WM_STRUT`: each band spans the whole edge of the root
    /// window of size `root`.
    pub fn from_full(values: &[u32; 4], (width, height): (u32, u32)) -> Self {
        let [left, right, top, bottom] = *values;
        let last_y = height.saturating_sub(1);
        let last_x = width.saturating_sub(1);
        Self {
            left,
            right,
            top,
            bottom,
            left_start_y: 0,
            left_end_y: last_y,
            right_start_y: 0,
            right_end_y: last_y,
            top_start_x: 0,
            top_end_x: last_x,
            bottom_start_x: 0,
            bottom_end_x: last_x,
        }
    }
}

/// How deep a band reserved against one edge of the root window enters a screen from its
/// matching side. `band` is the band across that axis as `[start, end)`, `screen` the
/// screen as `[start, end)` on the same axis; `range` the band along the edge, ends
/// included, and `along` the screen on that axis. A band that misses the screen, does
/// not reach its matching side, or crosses it whole (the screen lies between the panel
/// and the root edge) gives nothing.
fn band_inset(
    band: (i64, i64),
    screen: (i64, i64),
    range: (u32, u32),
    along: (i64, i64),
    from_start: bool,
) -> u32 {
    let (start, end) = band;
    let overlaps_along = i64::from(range.0) < along.1 && i64::from(range.1) >= along.0;
    if start >= end || !overlaps_along {
        return 0;
    }
    let depth = if from_start {
        if start > screen.0 || end <= screen.0 {
            return 0;
        }
        end - screen.0
    } else {
        if end < screen.1 || start >= screen.1 {
            return 0;
        }
        screen.1 - start
    };
    if depth >= screen.1 - screen.0 {
        return 0;
    }
    depth as u32
}

/// The space the `struts` of the client windows take on `monitor`, the root window being
/// of size `root`: per side, the deepest band that enters the screen from that side.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "read by the X11 work area, wired in #261")
)]
pub fn strut_insets(monitor: PixelRect, root: (u32, u32), struts: &[Strut]) -> Insets {
    let (root_width, root_height) = (i64::from(root.0), i64::from(root.1));
    let xs = (i64::from(monitor.x), monitor.right());
    let ys = (i64::from(monitor.y), monitor.bottom());
    struts
        .iter()
        .fold(Insets::default(), |insets, strut| Insets {
            left: insets.left.max(band_inset(
                (0, i64::from(strut.left)),
                xs,
                (strut.left_start_y, strut.left_end_y),
                ys,
                true,
            )),
            right: insets.right.max(band_inset(
                (root_width - i64::from(strut.right), root_width),
                xs,
                (strut.right_start_y, strut.right_end_y),
                ys,
                false,
            )),
            top: insets.top.max(band_inset(
                (0, i64::from(strut.top)),
                ys,
                (strut.top_start_x, strut.top_end_x),
                xs,
                true,
            )),
            bottom: insets.bottom.max(band_inset(
                (root_height - i64::from(strut.bottom), root_height),
                ys,
                (strut.bottom_start_x, strut.bottom_end_x),
                xs,
                false,
            )),
        })
}

/// Index in `monitors` of the screen `screen` designates. `primary` is the index of the
/// primary screen, `cursor` the mouse position. The active screen is the one under the
/// cursor, else the primary one, else the first; a named screen that is not connected
/// and a primary screen that is unknown fall back to the active one. `None` when there
/// is no screen.
pub fn pick_monitor(
    screen: &QuickTerminalScreen,
    monitors: &[Monitor],
    primary: Option<usize>,
    cursor: Option<(i32, i32)>,
) -> Option<usize> {
    let chosen = match screen {
        QuickTerminalScreen::Active => None,
        QuickTerminalScreen::Primary => primary,
        QuickTerminalScreen::Named(name) => monitors
            .iter()
            .position(|monitor| monitor.name.as_deref() == Some(name.as_str())),
    };
    let active = || {
        cursor
            .and_then(|cursor| {
                monitors
                    .iter()
                    .position(|monitor| monitor.rect.contains(cursor))
            })
            .or(primary)
            .or((!monitors.is_empty()).then_some(0))
    };
    chosen
        .filter(|&index| index < monitors.len())
        .or_else(active)
}

/// The window frame on `monitor` (the work area of a screen): against `edge`,
/// `size_percent` of it across that edge (rounded, at least one pixel), all of it along
/// the edge.
pub fn frame(monitor: PixelRect, edge: QuickTerminalEdge, size_percent: u32) -> PixelRect {
    let share = |length: u32| {
        let part = (u64::from(length) * u64::from(size_percent.min(100)) + 50) / 100;
        (part as u32).clamp(1, length.max(1))
    };
    match edge {
        QuickTerminalEdge::Top => PixelRect {
            height: share(monitor.height),
            ..monitor
        },
        QuickTerminalEdge::Bottom => {
            let height = share(monitor.height);
            PixelRect {
                y: monitor
                    .y
                    .saturating_add_unsigned(monitor.height.saturating_sub(height)),
                height,
                ..monitor
            }
        }
        QuickTerminalEdge::Left => PixelRect {
            width: share(monitor.width),
            ..monitor
        },
        QuickTerminalEdge::Right => {
            let width = share(monitor.width);
            PixelRect {
                x: monitor
                    .x
                    .saturating_add_unsigned(monitor.width.saturating_sub(width)),
                width,
                ..monitor
            }
        }
    }
}

/// A point of the desktop, in physical pixels.
pub type Point = (i32, i32);

/// Where the window of `frame` waits off screen before sliding in from `edge`: just
/// past that edge, so that the window touches the screen without covering it.
pub fn hidden_origin(frame: PixelRect, edge: QuickTerminalEdge) -> Point {
    match edge {
        QuickTerminalEdge::Top => (frame.x, frame.y.saturating_sub_unsigned(frame.height)),
        QuickTerminalEdge::Bottom => (frame.x, frame.y.saturating_add_unsigned(frame.height)),
        QuickTerminalEdge::Left => (frame.x.saturating_sub_unsigned(frame.width), frame.y),
        QuickTerminalEdge::Right => (frame.x.saturating_add_unsigned(frame.width), frame.y),
    }
}

/// Fast start, slow end: `t` from 0 to 1.
fn ease_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// The position of a slide from `from` to `to` lasting `duration`, `elapsed` after its
/// start; `to` once it is over.
pub fn slide_position(from: Point, to: Point, elapsed: Duration, duration: Duration) -> Point {
    if duration.is_zero() || elapsed >= duration {
        return to;
    }
    let eased = ease_out_cubic(elapsed.as_secs_f64() / duration.as_secs_f64());
    let lerp =
        |a: i32, b: i32| (f64::from(a) + (f64::from(b) - f64::from(a)) * eased).round() as i32;
    (lerp(from.0, to.0), lerp(from.1, to.1))
}

fn distance(a: Point, b: Point) -> f64 {
    (f64::from(a.0) - f64::from(b.0)).hypot(f64::from(a.1) - f64::from(b.1))
}

/// A window moving between its hidden origin and its frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slide {
    pub from: Point,
    pub to: Point,
    pub start: Instant,
    pub duration: Duration,
    /// The window hides at the end of the slide.
    pub hiding: bool,
}

impl Slide {
    /// A slide from `from` to `to` starting at `start`. `full` is the duration of the
    /// whole way between `hidden` and `shown`: a slide starting halfway, such as one
    /// reversed by a toggle, takes the share of it left to go.
    pub fn new(
        from: Point,
        to: Point,
        start: Instant,
        full: Duration,
        (hidden, shown): (Point, Point),
        hiding: bool,
    ) -> Self {
        let whole = distance(hidden, shown);
        let share = if whole > 0.0 {
            (distance(from, to) / whole).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Self {
            from,
            to,
            start,
            duration: full.mul_f64(share),
            hiding,
        }
    }

    /// Where the window is at `now`.
    pub fn position(&self, now: Instant) -> Point {
        slide_position(
            self.from,
            self.to,
            now.saturating_duration_since(self.start),
            self.duration,
        )
    }

    /// Whether the window has reached `to` at `now`.
    pub fn is_over(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_read_from_their_line() {
        for command in [QuickCommand::Toggle, QuickCommand::Show, QuickCommand::Hide] {
            assert_eq!(QuickCommand::parse(command.as_str()), Some(command));
            assert_eq!(
                QuickCommand::parse(&format!(" {}\n", command.as_str())),
                Some(command)
            );
        }
        assert_eq!(QuickCommand::parse(""), None);
        assert_eq!(QuickCommand::parse("quit"), None);
    }

    fn rect(x: i32, y: i32, width: u32, height: u32) -> PixelRect {
        PixelRect {
            x,
            y,
            width,
            height,
        }
    }

    fn monitors() -> Vec<Monitor> {
        vec![
            Monitor {
                name: Some("DP-1".to_owned()),
                rect: rect(0, 0, 1920, 1080),
                work_area: rect(0, 0, 1920, 1040),
            },
            Monitor {
                name: Some("HDMI-1".to_owned()),
                rect: rect(1920, 0, 2560, 1440),
                work_area: rect(1920, 0, 2560, 1400),
            },
        ]
    }

    #[test]
    fn active_screen_is_the_one_under_the_cursor() {
        let monitors = monitors();
        let active = QuickTerminalScreen::Active;
        assert_eq!(
            pick_monitor(&active, &monitors, Some(0), Some((2000, 100))),
            Some(1)
        );
        assert_eq!(
            pick_monitor(&active, &monitors, Some(1), Some((10, 10))),
            Some(0)
        );
        // The right and bottom edges belong to the next screen.
        assert_eq!(
            pick_monitor(&active, &monitors, Some(0), Some((1920, 0))),
            Some(1)
        );
    }

    #[test]
    fn active_screen_without_cursor_is_the_primary_then_the_first() {
        let monitors = monitors();
        let active = QuickTerminalScreen::Active;
        assert_eq!(pick_monitor(&active, &monitors, Some(1), None), Some(1));
        // A cursor outside every screen.
        assert_eq!(
            pick_monitor(&active, &monitors, Some(1), Some((-5, -5))),
            Some(1)
        );
        assert_eq!(pick_monitor(&active, &monitors, None, None), Some(0));
        assert_eq!(pick_monitor(&active, &[], None, Some((0, 0))), None);
    }

    #[test]
    fn primary_screen_is_picked_and_falls_back_to_the_active_one() {
        let monitors = monitors();
        let primary = QuickTerminalScreen::Primary;
        assert_eq!(
            pick_monitor(&primary, &monitors, Some(1), Some((10, 10))),
            Some(1)
        );
        assert_eq!(
            pick_monitor(&primary, &monitors, None, Some((2000, 10))),
            Some(1)
        );
    }

    #[test]
    fn named_screen_is_picked_and_an_absent_name_is_the_active_one() {
        let monitors = monitors();
        assert_eq!(
            pick_monitor(
                &QuickTerminalScreen::Named("HDMI-1".to_owned()),
                &monitors,
                Some(0),
                Some((10, 10))
            ),
            Some(1)
        );
        assert_eq!(
            pick_monitor(
                &QuickTerminalScreen::Named("VGA-9".to_owned()),
                &monitors,
                Some(1),
                Some((10, 10))
            ),
            Some(0)
        );
    }

    #[test]
    fn frame_sticks_to_each_edge() {
        let screen = rect(1920, 100, 2000, 1000);
        assert_eq!(
            frame(screen, QuickTerminalEdge::Top, 40),
            rect(1920, 100, 2000, 400)
        );
        assert_eq!(
            frame(screen, QuickTerminalEdge::Bottom, 40),
            rect(1920, 700, 2000, 400)
        );
        assert_eq!(
            frame(screen, QuickTerminalEdge::Left, 25),
            rect(1920, 100, 500, 1000)
        );
        assert_eq!(
            frame(screen, QuickTerminalEdge::Right, 25),
            rect(3420, 100, 500, 1000)
        );
    }

    #[test]
    fn frame_at_the_size_bounds() {
        let screen = rect(0, 0, 1366, 768);
        assert_eq!(frame(screen, QuickTerminalEdge::Top, 100), screen);
        assert_eq!(frame(screen, QuickTerminalEdge::Right, 100), screen);
        // Rounded to the nearest pixel.
        assert_eq!(
            frame(screen, QuickTerminalEdge::Bottom, 10),
            rect(0, 691, 1366, 77)
        );
        assert_eq!(
            frame(screen, QuickTerminalEdge::Left, 10),
            rect(0, 0, 137, 768)
        );
        // Never empty, never larger than the screen.
        assert_eq!(
            frame(rect(0, 0, 4, 4), QuickTerminalEdge::Top, 10),
            rect(0, 0, 4, 1)
        );
        assert_eq!(
            frame(screen, QuickTerminalEdge::Top, 150),
            frame(screen, QuickTerminalEdge::Top, 100)
        );
    }

    #[test]
    fn insets_shrink_the_screen() {
        let screen = rect(0, 0, 2880, 1800);
        let insets = Insets {
            top: 74,
            bottom: 160,
            left: 0,
            right: 0,
        };
        assert_eq!(screen.inset(insets), rect(0, 74, 2880, 1566));
        let huge = Insets {
            top: 2000,
            ..Insets::default()
        };
        assert_eq!(screen.inset(huge).height, 1);
    }

    #[test]
    fn active_screen_under_a_panel_is_still_that_screen() {
        let monitors = monitors();
        let active = QuickTerminalScreen::Active;
        // The cursor over the bottom panel, outside the work areas.
        assert_eq!(
            pick_monitor(&active, &monitors, Some(1), Some((100, 1060))),
            Some(0)
        );
        assert_eq!(
            pick_monitor(&active, &monitors, Some(0), Some((3000, 1430))),
            Some(1)
        );
    }

    #[test]
    fn frame_at_full_size_fills_the_work_area_for_each_edge() {
        let work_area = rect(2560, 32, 2560, 1368);
        for edge in [
            QuickTerminalEdge::Top,
            QuickTerminalEdge::Bottom,
            QuickTerminalEdge::Left,
            QuickTerminalEdge::Right,
        ] {
            assert_eq!(frame(work_area, edge, 100), work_area, "{edge:?}");
        }
    }

    #[test]
    fn intersection_of_rectangles() {
        let screen = rect(2560, 0, 2560, 1440);
        assert_eq!(
            screen.intersection(rect(0, 0, 5120, 1400)),
            Some(rect(2560, 0, 2560, 1400))
        );
        assert_eq!(screen.intersection(screen), Some(screen));
        // Touching is not overlapping.
        assert_eq!(screen.intersection(rect(0, 0, 2560, 1440)), None);
        assert_eq!(screen.intersection(rect(6000, 0, 10, 10)), None);
    }

    #[test]
    fn rects_are_read_from_groups_of_four_cardinals() {
        assert_eq!(rects_from_cardinals(&[]), Vec::new());
        assert_eq!(
            rects_from_cardinals(&[0, 0, 2560, 1400]),
            vec![rect(0, 0, 2560, 1400)]
        );
        assert_eq!(
            rects_from_cardinals(&[0, 0, 2560, 1400, 2560, 0, 2560, 1400]),
            vec![rect(0, 0, 2560, 1400), rect(2560, 0, 2560, 1400)]
        );
        // An incomplete trailing group is ignored.
        assert_eq!(
            rects_from_cardinals(&[0, 0, 2560, 1400, 2560, 0]),
            vec![rect(0, 0, 2560, 1400)]
        );
        // Negative coordinates come as their two's complement.
        assert_eq!(
            rects_from_cardinals(&[(-1920_i32) as u32, 0, 1920, 1080]),
            vec![rect(-1920, 0, 1920, 1080)]
        );
    }

    const LEFT: PixelRect = PixelRect {
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };
    const RIGHT: PixelRect = PixelRect {
        x: 2560,
        y: 0,
        width: 2560,
        height: 1440,
    };
    const ROOT: (u32, u32) = (5120, 1440);

    #[test]
    fn gtk_work_area_is_the_one_of_the_screen_clipped_to_it() {
        let areas = [rect(0, 0, 2560, 1400), rect(2560, 0, 2560, 1400)];
        assert_eq!(gtk_work_area(LEFT, &areas), Some(rect(0, 0, 2560, 1400)));
        assert_eq!(
            gtk_work_area(RIGHT, &areas),
            Some(rect(2560, 0, 2560, 1400))
        );
        // An area wider than the screen is clipped to it.
        assert_eq!(
            gtk_work_area(RIGHT, &[rect(0, 0, 5120, 1400)]),
            Some(rect(2560, 0, 2560, 1400))
        );
        assert_eq!(gtk_work_area(rect(6000, 0, 800, 600), &areas), None);
        assert_eq!(gtk_work_area(LEFT, &[]), None);
    }

    #[test]
    fn bottom_panel_of_one_screen_insets_only_that_screen() {
        let panel = Strut {
            bottom: 40,
            bottom_start_x: 2560,
            bottom_end_x: 5119,
            ..Strut::default()
        };
        assert_eq!(strut_insets(LEFT, ROOT, &[panel]), Insets::default());
        assert_eq!(
            strut_insets(RIGHT, ROOT, &[panel]),
            Insets {
                bottom: 40,
                ..Insets::default()
            }
        );
    }

    #[test]
    fn strut_whose_range_misses_the_screen_gives_nothing() {
        let panel = Strut::from_partial(&[0, 0, 30, 0, 0, 0, 0, 0, 0, 2559, 0, 0]);
        assert_eq!(panel.top, 30);
        assert_eq!(panel.top_end_x, 2559);
        assert_eq!(strut_insets(RIGHT, ROOT, &[panel]), Insets::default());
        assert_eq!(
            strut_insets(LEFT, ROOT, &[panel]),
            Insets {
                top: 30,
                ..Insets::default()
            }
        );
    }

    #[test]
    fn panel_on_an_inner_edge_insets_its_screen_only() {
        // A 40 px panel on the left side of the right screen reserves 2600 px from the
        // left edge of the root window: it crosses the whole left screen.
        let panel = Strut {
            left: 2600,
            left_start_y: 0,
            left_end_y: 1439,
            ..Strut::default()
        };
        assert_eq!(strut_insets(LEFT, ROOT, &[panel]), Insets::default());
        assert_eq!(
            strut_insets(RIGHT, ROOT, &[panel]),
            Insets {
                left: 40,
                ..Insets::default()
            }
        );
    }

    #[test]
    fn full_strut_spans_the_whole_edge() {
        let panel = Strut::from_full(&[0, 0, 0, 50], ROOT);
        assert_eq!(panel.bottom_end_x, 5119);
        let expected = Insets {
            bottom: 50,
            ..Insets::default()
        };
        assert_eq!(strut_insets(LEFT, ROOT, &[panel]), expected);
        assert_eq!(strut_insets(RIGHT, ROOT, &[panel]), expected);
        assert_eq!(RIGHT.inset(expected), rect(2560, 0, 2560, 1390));
    }

    #[test]
    fn largest_strut_wins_per_side() {
        let struts = [
            Strut::from_full(&[0, 0, 0, 30], ROOT),
            Strut::from_full(&[0, 0, 0, 50], ROOT),
            Strut::from_full(&[0, 24, 0, 10], ROOT),
        ];
        assert_eq!(
            strut_insets(RIGHT, ROOT, &struts),
            Insets {
                bottom: 50,
                right: 24,
                ..Insets::default()
            }
        );
        assert_eq!(
            strut_insets(LEFT, ROOT, &struts),
            Insets {
                bottom: 50,
                ..Insets::default()
            }
        );
    }

    #[test]
    fn hidden_origin_is_past_each_edge() {
        let frame = rect(1920, 100, 2000, 400);
        assert_eq!(hidden_origin(frame, QuickTerminalEdge::Top), (1920, -300));
        assert_eq!(hidden_origin(frame, QuickTerminalEdge::Bottom), (1920, 500));
        assert_eq!(hidden_origin(frame, QuickTerminalEdge::Left), (-80, 100));
        assert_eq!(hidden_origin(frame, QuickTerminalEdge::Right), (3920, 100));
    }

    #[test]
    fn slide_starts_at_from_and_ends_at_to() {
        let (from, to) = ((0, -400), (0, 0));
        let duration = Duration::from_millis(200);
        assert_eq!(slide_position(from, to, Duration::ZERO, duration), from);
        assert_eq!(slide_position(from, to, duration, duration), to);
        assert_eq!(slide_position(from, to, duration * 2, duration), to);
        // Eased out: past the middle at half the time.
        let (_, y) = slide_position(from, to, duration / 2, duration);
        assert_eq!(y, -50);
        assert_eq!(slide_position(from, to, Duration::ZERO, Duration::ZERO), to);
    }

    #[test]
    fn reversed_slide_goes_back_from_where_it_is_in_its_share_of_the_time() {
        let start = Instant::now();
        let full = Duration::from_millis(200);
        let (hidden, shown) = ((0, -400), (0, 0));
        let showing = Slide::new(hidden, shown, start, full, (hidden, shown), false);
        assert_eq!(showing.duration, full);
        let now = start + Duration::from_millis(100);
        let here = showing.position(now);
        assert!(!showing.is_over(now));
        let hiding = Slide::new(here, hidden, now, full, (hidden, shown), true);
        assert_eq!(hiding.position(now), here);
        assert_eq!(hiding.duration, full.mul_f64(350.0 / 400.0));
        assert!(hiding.is_over(now + hiding.duration));
        assert_eq!(hiding.position(now + hiding.duration), hidden);
        // Already there: no time at all.
        let still = Slide::new(shown, shown, now, full, (hidden, shown), false);
        assert!(still.is_over(now));
    }
}
