//! One-line banner drawn at the top of the window, over the grid.
//!
//! The banner content is plain data; [`BannerLayout`] places it in pixels so both the
//! renderer and the mouse handling use the same geometry.

use semver::Version;

use crate::config::ConfigError;
use crate::i18n::{self, Lang, Msg};
use crate::layout::CellSize;
use crate::update::{self, Release};

/// Label of the action closing the update banner for the session.
const CLOSE_LABEL: &str = "×";

/// Content of the banner: a message followed by clickable actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub text: String,
    /// Labels of the click targets, drawn after the text, left to right.
    pub actions: Vec<String>,
}

impl Banner {
    /// Banner reporting configuration errors: the first one, plus "(+N more)" when there are
    /// several. `None` when there is no error.
    pub fn config_errors(errors: &[ConfigError]) -> Option<Self> {
        let first = errors.first()?;
        let text = match errors.len() - 1 {
            0 => first.to_string(),
            more => format!("{first} (+{more} more)"),
        };
        Some(Self {
            text,
            actions: Vec::new(),
        })
    }
}

impl Banner {
    /// Banner announcing `release`, with its actions in [`UpdateAction`] order.
    pub fn update(release: &Release, lang: Lang) -> Self {
        Self {
            text: i18n::text(lang, &Msg::UpdateAvailable(release.tag.clone())).into_owned(),
            actions: UpdateAction::ALL
                .iter()
                .map(|action| match action {
                    UpdateAction::Open => i18n::text(lang, &Msg::OpenRelease).into_owned(),
                    UpdateAction::Ignore => i18n::text(lang, &Msg::IgnoreVersion).into_owned(),
                    UpdateAction::Close => CLOSE_LABEL.to_owned(),
                })
                .collect(),
        }
    }
}

/// Which banner is shown: the configuration error banner has priority over the update
/// banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shown {
    Config(Banner),
    Update(Banner),
}

impl Shown {
    pub fn pick(config: Option<&Banner>, update: &UpdateBanner, lang: Lang) -> Option<Self> {
        if let Some(banner) = config {
            return Some(Shown::Config(banner.clone()));
        }
        update
            .visible()
            .map(|release| Shown::Update(Banner::update(release, lang)))
    }

    pub fn banner(&self) -> &Banner {
        match self {
            Shown::Config(banner) | Shown::Update(banner) => banner,
        }
    }
}

/// Click targets of the update banner, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    /// Opens the release page; the banner stays.
    Open,
    /// Ignores this version for good.
    Ignore,
    /// Hides the banner until the next launch.
    Close,
}

impl UpdateAction {
    pub const ALL: [UpdateAction; 3] = [
        UpdateAction::Open,
        UpdateAction::Ignore,
        UpdateAction::Close,
    ];
}

/// Side effect of an update banner action, run by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateEffect {
    /// Open this URL.
    Open(String),
    /// Record this tag as ignored.
    Ignore(String),
}

/// Visibility of the update banner: the release reported by the check thread, minus what
/// the user closed during this session.
#[derive(Debug, Clone, Default)]
pub struct UpdateBanner {
    release: Option<Release>,
    /// Version closed with ×: only a newer release shows the banner again.
    closed: Option<Version>,
}

impl UpdateBanner {
    /// New release from the check thread, `None` when nothing is to be shown.
    pub fn set(&mut self, release: Option<Release>) {
        self.release = release;
    }

    /// The release to show, if any.
    pub fn visible(&self) -> Option<&Release> {
        let release = self.release.as_ref()?;
        match &self.closed {
            None => Some(release),
            Some(closed) => update::parse_tag(&release.tag)
                .is_some_and(|version| version > *closed)
                .then_some(release),
        }
    }

    /// Runs the action at `index` of [`UpdateAction::ALL`]. Returns the side effect left
    /// to the caller.
    pub fn press(&mut self, index: usize) -> Option<UpdateEffect> {
        let release = self.visible()?.clone();
        match UpdateAction::ALL.get(index)? {
            UpdateAction::Open => Some(UpdateEffect::Open(release.url)),
            UpdateAction::Ignore => {
                self.release = None;
                Some(UpdateEffect::Ignore(release.tag))
            }
            UpdateAction::Close => {
                self.closed = update::parse_tag(&release.tag);
                // A tag that is not a version cannot be compared: hide it for the session.
                if self.closed.is_none() {
                    self.release = None;
                }
                None
            }
        }
    }
}

/// Axis-aligned rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl PixelRect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// A piece of banner text and the pixel position of its top-left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedText {
    pub x: f32,
    pub y: f32,
    pub text: String,
}

/// What a click on the banner hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerHit {
    /// The action at this index in [`Banner::actions`].
    Action(usize),
    /// The banner outside any action.
    Body,
}

/// Pixel geometry of a [`Banner`].
#[derive(Debug, Clone, PartialEq)]
pub struct BannerLayout {
    /// Background, spanning the window width.
    pub rect: PixelRect,
    /// Message then action labels.
    pub texts: Vec<PlacedText>,
    /// Click target of each action, in [`Banner::actions`] order.
    pub actions: Vec<PixelRect>,
}

impl BannerLayout {
    /// Places `banner` at the top of a window `width` pixels wide. The text uses the grid
    /// font, so a character is one cell wide.
    pub fn new(banner: &Banner, cell: CellSize, width: f32) -> Self {
        let margin = (cell.height / 4.0).round();
        let height = cell.height + 2.0 * margin;
        let gap = 2.0 * cell.width;
        let text_width = |text: &str| text.chars().count() as f32 * cell.width;

        let mut x = 2.0 * cell.width;
        let mut texts = vec![PlacedText {
            x,
            y: margin,
            text: banner.text.clone(),
        }];
        x += text_width(&banner.text);
        let mut actions = Vec::with_capacity(banner.actions.len());
        for label in &banner.actions {
            x += gap;
            texts.push(PlacedText {
                x,
                y: margin,
                text: label.clone(),
            });
            // The target extends half a gap around the label, over the full banner height.
            actions.push(PixelRect {
                x: x - gap / 2.0,
                y: 0.0,
                width: text_width(label) + gap,
                height,
            });
            x += text_width(label);
        }

        Self {
            rect: PixelRect {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
            texts,
            actions,
        }
    }

    /// What the pixel (`x`, `y`) hits, `None` outside the banner.
    pub fn hit(&self, x: f32, y: f32) -> Option<BannerHit> {
        if !self.rect.contains(x, y) {
            return None;
        }
        Some(
            self.actions
                .iter()
                .position(|target| target.contains(x, y))
                .map_or(BannerHit::Body, BannerHit::Action),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn error(line: usize, message: &str) -> ConfigError {
        ConfigError {
            file: PathBuf::from("/home/me/.config/graphene/config.toml"),
            line: Some(line),
            message: message.to_owned(),
        }
    }

    const CELL: CellSize = CellSize {
        width: 10.0,
        height: 20.0,
    };

    #[test]
    fn no_error_gives_no_banner() {
        assert_eq!(Banner::config_errors(&[]), None);
    }

    #[test]
    fn one_error_is_shown_as_is() {
        let banner = Banner::config_errors(&[error(12, "unknown key `font.sise`")]).unwrap();
        assert_eq!(banner.text, "config.toml:12: unknown key `font.sise`");
        assert!(banner.actions.is_empty());
    }

    #[test]
    fn several_errors_show_the_first_and_a_count() {
        let banner = Banner::config_errors(&[
            error(2, "unknown key `a`"),
            error(5, "unknown key `b`"),
            error(9, "unknown key `c`"),
        ])
        .unwrap();
        assert_eq!(banner.text, "config.toml:2: unknown key `a` (+2 more)");
    }

    fn release(tag: &str) -> Release {
        Release {
            tag: tag.to_owned(),
            url: format!("https://github.com/marmotz/graphene-term/releases/tag/{tag}"),
        }
    }

    #[test]
    fn update_banner_text_and_actions() {
        let banner = Banner::update(&release("v0.2.0"), Lang::En);
        assert_eq!(banner.text, "Graphene v0.2.0 is available");
        assert_eq!(
            banner.actions,
            vec!["Open release", "Ignore this version", "×"]
        );
        let banner = Banner::update(&release("v0.2.0"), Lang::Fr);
        assert_eq!(banner.text, "Graphene v0.2.0 est disponible");
        assert_eq!(banner.actions[1], "Ignorer cette version");
    }

    #[test]
    fn update_banner_shows_the_reported_release() {
        let mut banner = UpdateBanner::default();
        assert_eq!(banner.visible(), None);
        banner.set(Some(release("v0.2.0")));
        assert_eq!(banner.visible(), Some(&release("v0.2.0")));
        banner.set(None);
        assert_eq!(banner.visible(), None);
    }

    #[test]
    fn config_banner_has_priority() {
        let config = Banner::config_errors(&[error(2, "unknown key `a`")]).unwrap();
        let mut update = UpdateBanner::default();
        update.set(Some(release("v0.2.0")));
        assert_eq!(
            Shown::pick(Some(&config), &update, Lang::En),
            Some(Shown::Config(config.clone()))
        );
        // Once the error banner is gone, the update banner shows.
        assert_eq!(
            Shown::pick(None, &update, Lang::En),
            Some(Shown::Update(Banner::update(&release("v0.2.0"), Lang::En)))
        );
        assert_eq!(Shown::pick(None, &UpdateBanner::default(), Lang::En), None);
    }

    #[test]
    fn open_keeps_the_banner() {
        let mut banner = UpdateBanner::default();
        banner.set(Some(release("v0.2.0")));
        assert_eq!(
            banner.press(0),
            Some(UpdateEffect::Open(release("v0.2.0").url))
        );
        assert_eq!(banner.visible(), Some(&release("v0.2.0")));
    }

    #[test]
    fn ignore_hides_the_banner_and_reports_the_tag() {
        let mut banner = UpdateBanner::default();
        banner.set(Some(release("v0.2.0")));
        assert_eq!(
            banner.press(1),
            Some(UpdateEffect::Ignore("v0.2.0".to_owned()))
        );
        assert_eq!(banner.visible(), None);
        // A newer release reported later shows again.
        banner.set(Some(release("v0.3.0")));
        assert_eq!(banner.visible(), Some(&release("v0.3.0")));
    }

    #[test]
    fn close_hides_until_a_newer_release() {
        let mut banner = UpdateBanner::default();
        banner.set(Some(release("v0.2.0-dev.20261112093000")));
        assert_eq!(banner.press(2), None);
        assert_eq!(banner.visible(), None);
        // The same release reported again stays hidden.
        banner.set(Some(release("v0.2.0-dev.20261112093000")));
        assert_eq!(banner.visible(), None);
        // A newer one shows the banner again.
        banner.set(Some(release("v0.2.0-dev.20261113093000")));
        assert_eq!(
            banner.visible(),
            Some(&release("v0.2.0-dev.20261113093000"))
        );
    }

    #[test]
    fn press_without_visible_release_or_out_of_range_does_nothing() {
        let mut banner = UpdateBanner::default();
        assert_eq!(banner.press(0), None);
        banner.set(Some(release("v0.2.0")));
        assert_eq!(banner.press(3), None);
        assert_eq!(banner.visible(), Some(&release("v0.2.0")));
    }

    #[test]
    fn update_banner_targets_are_hit_tested() {
        let banner = Banner::update(&release("v0.2.0"), Lang::En);
        let layout = BannerLayout::new(&banner, CELL, 2000.0);
        for (index, target) in layout.actions.iter().enumerate() {
            let (x, y) = (target.x + target.width / 2.0, target.y + 1.0);
            assert_eq!(layout.hit(x, y), Some(BannerHit::Action(index)));
        }
        assert_eq!(layout.actions.len(), UpdateAction::ALL.len());
        assert_eq!(layout.hit(25.0, 10.0), Some(BannerHit::Body));
    }

    #[test]
    fn layout_spans_the_window_top() {
        let banner = Banner {
            text: "hello".into(),
            actions: Vec::new(),
        };
        let layout = BannerLayout::new(&banner, CELL, 800.0);
        assert_eq!(
            layout.rect,
            PixelRect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 30.0,
            }
        );
        assert_eq!(
            layout.texts,
            vec![PlacedText {
                x: 20.0,
                y: 5.0,
                text: "hello".into(),
            }]
        );
        assert!(layout.actions.is_empty());
    }

    #[test]
    fn actions_follow_the_text() {
        let banner = Banner {
            text: "abc".into(),
            actions: vec!["open".into(), "x".into()],
        };
        let layout = BannerLayout::new(&banner, CELL, 800.0);
        // Text from 20 to 50, then a 20 px gap before each action.
        let xs: Vec<f32> = layout.texts.iter().map(|t| t.x).collect();
        assert_eq!(xs, vec![20.0, 70.0, 130.0]);
        assert_eq!(layout.actions[0].x, 60.0);
        assert_eq!(layout.actions[0].width, 60.0);
        assert_eq!(layout.actions[1].x, 120.0);
        assert_eq!(layout.actions[1].width, 30.0);
    }

    #[test]
    fn hit_testing() {
        let banner = Banner {
            text: "abc".into(),
            actions: vec!["open".into(), "x".into()],
        };
        let layout = BannerLayout::new(&banner, CELL, 800.0);
        assert_eq!(layout.hit(30.0, 10.0), Some(BannerHit::Body));
        assert_eq!(layout.hit(75.0, 10.0), Some(BannerHit::Action(0)));
        assert_eq!(layout.hit(135.0, 29.0), Some(BannerHit::Action(1)));
        assert_eq!(layout.hit(500.0, 0.0), Some(BannerHit::Body));
        assert_eq!(layout.hit(30.0, 30.0), None);
    }
}
