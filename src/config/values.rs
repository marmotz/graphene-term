//! Values of the configuration options that are not plain numbers or strings.

use super::*;

/// Value of the `language` setting: a BCP 47 tag, case ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    /// Follows `LANG`.
    #[default]
    Auto,
    EnUs,
    EnGb,
    Fr,
    FrCa,
}

impl Language {
    /// Accepted values, in the order shown to the user.
    pub const VALUES: [(&'static str, Language); 5] = [
        ("auto", Language::Auto),
        ("en-US", Language::EnUs),
        ("en-GB", Language::EnGb),
        ("fr", Language::Fr),
        ("fr-CA", Language::FrCa),
    ];

    /// Former value of `en-US`, still accepted so that existing files stay valid.
    const ALIASES: [(&'static str, Language); 1] = [("en", Language::EnUs)];

    /// Value named `name`, case ignored, aliases included.
    fn from_name(name: &str) -> Option<Self> {
        Self::VALUES
            .iter()
            .chain(&Self::ALIASES)
            .find(|(value, _)| value.eq_ignore_ascii_case(name))
            .map(|&(_, language)| language)
    }
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        const NAMES: [&str; Language::VALUES.len()] = {
            let mut names = [""; Language::VALUES.len()];
            let mut i = 0;
            while i < names.len() {
                names[i] = Language::VALUES[i].0;
                i += 1;
            }
            names
        };
        let name = String::deserialize(deserializer)?;
        Self::from_name(&name).ok_or_else(|| serde::de::Error::unknown_variant(&name, &NAMES))
    }
}

impl Language {
    /// Language of the interface: the forced one, or `detected` (the locale's) for
    /// `auto`.
    pub fn resolve(self, detected: impl FnOnce() -> Lang) -> Lang {
        match self {
            Language::Auto => detected(),
            Language::EnUs => Lang::EnUs,
            Language::EnGb => Lang::EnGb,
            Language::Fr => Lang::Fr,
            Language::FrCa => Lang::FrCa,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorShape {
    #[default]
    Block,
    Beam,
    Underline,
}

impl CursorShape {
    /// Accepted values, in the order shown to the user.
    pub const VALUES: [(&'static str, CursorShape); 3] = [
        ("block", CursorShape::Block),
        ("beam", CursorShape::Beam),
        ("underline", CursorShape::Underline),
    ];
}

impl From<CursorShape> for TermCursorShape {
    fn from(shape: CursorShape) -> Self {
        match shape {
            CursorShape::Block => Self::Block,
            CursorShape::Beam => Self::Beam,
            CursorShape::Underline => Self::Underline,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl TabPosition {
    /// Accepted values, in the order shown to the user.
    pub const VALUES: [(&'static str, TabPosition); 4] = [
        ("top", TabPosition::Top),
        ("bottom", TabPosition::Bottom),
        ("left", TabPosition::Left),
        ("right", TabPosition::Right),
    ];
}

/// `window.startup_mode`: size and state of a new window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartupMode {
    /// The configured `width` and `height`.
    #[default]
    Default,
    /// Maximized; the configured size is used when the user unmaximizes it.
    Maximized,
    /// The size and maximized state of the last window closed.
    Remember,
}

impl StartupMode {
    /// Accepted values, in the order shown to the user.
    pub const VALUES: [(&'static str, StartupMode); 3] = [
        ("default", StartupMode::Default),
        ("maximized", StartupMode::Maximized),
        ("remember", StartupMode::Remember),
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontConfig {
    /// `None` uses the system monospace font.
    pub family: Option<String>,
    pub size: f32,
    /// Shows the ligatures of the fonts that have some.
    pub ligatures: bool,
    /// OpenType features applied to the grid text.
    pub features: FontFeatureList,
}

/// `font.features`: OpenType features and their values, each tag at most once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontFeatureList(pub Vec<(FeatureTag, u32)>);

impl FontFeatureList {
    /// Parses a list of features separated by spaces or commas: `tag` or `+tag` enables
    /// it, `-tag` disables it, `tag=N` sets the value `N`. A tag is 4 printable ASCII
    /// characters; the last occurrence of a tag wins. The error names the invalid item.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut features: Vec<(FeatureTag, u32)> = Vec::new();
        for item in text
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|item| !item.is_empty())
        {
            let invalid = || {
                format!(
                    "invalid feature \"{item}\", expected \"tag\", \"+tag\", \"-tag\" or \"tag=N\" with a tag of 4 ASCII characters"
                )
            };
            let (tag, value) = match (item.strip_prefix('+'), item.strip_prefix('-')) {
                (Some(tag), _) => (tag, 1),
                (_, Some(tag)) => (tag, 0),
                _ => match item.split_once('=') {
                    Some((tag, value)) => (tag, value.parse().map_err(|_| invalid())?),
                    None => (item, 1),
                },
            };
            let tag: [u8; 4] = tag
                .as_bytes()
                .try_into()
                .ok()
                .filter(|tag: &[u8; 4]| tag.iter().all(u8::is_ascii_graphic))
                .ok_or_else(invalid)?;
            let tag = FeatureTag::new(&tag);
            features.retain(|&(other, _)| other != tag);
            features.push((tag, value));
        }
        Ok(Self(features))
    }

    /// The features in the form cosmic-text shapes with.
    pub fn font_features(&self) -> FontFeatures {
        let mut features = FontFeatures::new();
        for &(tag, value) in &self.0 {
            features.set(tag, value);
        }
        features
    }
}

/// Canonical form, parsed back to the same list: `tag`, `-tag` or `tag=N`, separated
/// by spaces.
impl fmt::Display for FontFeatureList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, (tag, value)) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            let tag = String::from_utf8_lossy(tag.as_bytes());
            match value {
                0 => write!(f, "-{tag}")?,
                1 => write!(f, "{tag}")?,
                value => write!(f, "{tag}={value}")?,
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowConfig {
    pub startup_mode: StartupMode,
    /// Initial size, in logical pixels, when `startup_mode` is `Default`, or `Remember`
    /// with nothing remembered yet.
    pub width: u32,
    pub height: u32,
    /// Minimum padding around the grid, in logical pixels.
    pub padding: f32,
    /// Opacity of the default background, from 0.0 (transparent) to 1.0 (opaque).
    pub background_opacity: f32,
    /// Shows the progress reported by programs (OSC 9;4) in the tabs, panes, blocks and
    /// taskbar.
    pub progress: bool,
    /// Saves the windows, their tabs and panes, and reopens them at the next launch.
    /// Read at launch only.
    pub restore_session: bool,
}

/// `quick_terminal.edge`: the screen edge the quick terminal is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuickTerminalEdge {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl QuickTerminalEdge {
    /// Accepted values, in the order shown to the user.
    pub const VALUES: [(&'static str, QuickTerminalEdge); 4] = [
        ("top", QuickTerminalEdge::Top),
        ("bottom", QuickTerminalEdge::Bottom),
        ("left", QuickTerminalEdge::Left),
        ("right", QuickTerminalEdge::Right),
    ];
}

/// `quick_terminal.screen`: the screen the quick terminal appears on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum QuickTerminalScreen {
    /// The screen under the mouse cursor.
    #[default]
    Active,
    /// The primary screen.
    Primary,
    /// The screen of this name (`MonitorHandle::name`); the active one when absent.
    Named(String),
}

impl QuickTerminalScreen {
    /// Reads a value of the file: `"active"`, `"primary"` or a non-empty screen name.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "" => Err(
                "must not be empty, expected \"active\", \"primary\" or a screen name".to_owned(),
            ),
            "active" => Ok(Self::Active),
            "primary" => Ok(Self::Primary),
            name => Ok(Self::Named(name.to_owned())),
        }
    }
}

/// The value written to the file.
impl fmt::Display for QuickTerminalScreen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => f.write_str("active"),
            Self::Primary => f.write_str("primary"),
            Self::Named(name) => f.write_str(name),
        }
    }
}

/// `[quick_terminal]`: the drop-down terminal attached to a screen edge.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickTerminalConfig {
    /// Allows Graphene to start the quick terminal; off, it is never launched.
    pub enabled: bool,
    /// Native global shortcut, in the syntax of `keybindings.toml`; empty for none.
    pub hotkey: String,
    pub edge: QuickTerminalEdge,
    /// Percentage of the screen, from [`MIN_QUICK_TERMINAL_SIZE`] to
    /// [`MAX_QUICK_TERMINAL_SIZE`]: the height for `top`/`bottom`, the width for
    /// `left`/`right`.
    pub size: u32,
    pub screen: QuickTerminalScreen,
    /// Duration of the slide, in milliseconds; 0 for none, at most
    /// [`MAX_QUICK_TERMINAL_ANIMATION_MS`].
    pub animation_ms: u32,
    /// Hides the quick terminal when it loses the focus.
    pub hide_on_focus_loss: bool,
    /// Starts the hidden quick terminal with the graphical session.
    pub autostart: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CursorConfig {
    pub shape: CursorShape,
    pub blinking: bool,
}

/// `updates.interval`: time between two update checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateInterval {
    /// 6 hours for a dev build, 24 hours for a stable one.
    #[default]
    Auto,
    Hours6,
    Hours12,
    Hours24,
    Days2,
    Days3,
    Week1,
}

impl UpdateInterval {
    /// Accepted values, in the order shown to the user.
    pub const VALUES: [(&'static str, UpdateInterval); 7] = [
        ("auto", UpdateInterval::Auto),
        ("6h", UpdateInterval::Hours6),
        ("12h", UpdateInterval::Hours12),
        ("24h", UpdateInterval::Hours24),
        ("2d", UpdateInterval::Days2),
        ("3d", UpdateInterval::Days3),
        ("1w", UpdateInterval::Week1),
    ];

    pub fn parse(value: &str) -> Option<Self> {
        Self::VALUES
            .iter()
            .find(|(name, _)| *name == value)
            .map(|&(_, interval)| interval)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UpdatesConfig {
    /// `false`: no network call at all.
    pub check: bool,
    pub interval: UpdateInterval,
}

/// `[notifications]`: which events raise a desktop notification.
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationsConfig {
    /// Notifications requested by programs (OSC 9, OSC 777).
    pub program: bool,
    /// Notify when a command that ran at least `command_threshold` seconds finishes.
    pub command_finished: bool,
    /// Minimum duration of a command for its end to be notified, in seconds (at least 1).
    pub command_threshold: u64,
    /// Turn the terminal bell into a notification.
    pub bell: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeConfig {
    pub name: String,
    pub light: Option<String>,
    pub dark: Option<String>,
    /// Hidden themes unlocked by the user, listed with the others once unlocked.
    pub unlocked: Vec<String>,
}

impl ThemeConfig {
    /// Whether the theme follows the system appearance: both `light` and `dark` are set.
    pub fn follows_appearance(&self) -> bool {
        self.light.is_some() && self.dark.is_some()
    }

    /// The theme to load: `light` or `dark` by `appearance` when both are set,
    /// otherwise `name`.
    pub fn active(&self, appearance: Appearance) -> &str {
        match (&self.light, &self.dark, appearance) {
            (Some(light), Some(_), Appearance::Light) => light,
            (Some(_), Some(dark), Appearance::Dark) => dark,
            _ => &self.name,
        }
    }

    /// Key of the theme loaded for `appearance`: `theme.light` or `theme.dark` when
    /// following the system appearance, otherwise `theme.name`.
    pub fn active_key(&self, appearance: Appearance) -> &'static [&'static str] {
        if self.follows_appearance() {
            match appearance {
                Appearance::Light => &["theme", "light"],
                Appearance::Dark => &["theme", "dark"],
            }
        } else {
            &["theme", "name"]
        }
    }
}
