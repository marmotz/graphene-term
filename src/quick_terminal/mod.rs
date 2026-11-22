//! The quick terminal: a window attached to a screen edge, shown and hidden by
//! `graphene --quick-toggle`, held by a single resident `graphene --quick` process.
//!
//! - [`control`]: single instance lock, command socket and the `--quick-toggle` client.
//! - This module: the commands of the socket and the placement of the window, computed
//!   without a window so that it is tested everywhere.

pub mod control;
#[cfg(target_os = "macos")]
pub mod macos;

use crate::config::{QuickTerminalEdge, QuickTerminalScreen};

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
        let right = i64::from(self.x) + i64::from(self.width);
        let bottom = i64::from(self.y) + i64::from(self.height);
        x >= self.x && y >= self.y && i64::from(x) < right && i64::from(y) < bottom
    }

    /// The rectangle without `insets` on its sides, never smaller than one pixel.
    #[cfg_attr(
        not(target_os = "macos"),
        allow(dead_code, reason = "only macOS screens have a menu bar and a Dock")
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
}

/// Space taken on each side of a screen by the desktop (menu bar, Dock), in pixels.
#[cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only macOS screens have a menu bar and a Dock")
)]
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
    pub rect: PixelRect,
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

/// The window frame on `monitor`: against `edge`, `size_percent` of the screen across
/// it (rounded, at least one pixel), the whole screen along it.
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
            },
            Monitor {
                name: Some("HDMI-1".to_owned()),
                rect: rect(1920, 0, 2560, 1440),
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
}
