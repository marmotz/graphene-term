//! Update check: asks GitHub for the newest release of the running binary's channel and
//! decides whether to notify the user. The HTTP call is a thin adapter; the decisions
//! (`notice`, release selection, scheduling) are pure functions.

pub mod checker;
pub mod state;

use std::time::Duration;

use anyhow::Context;
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::config::UpdateInterval;
use state::State;

/// Timeout of one GitHub request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Number of releases fetched by a dev build.
const RELEASES_PER_PAGE: u32 = 30;

const HOUR: u64 = 3_600;
const DAY: u64 = 24 * HOUR;

/// Release channel of the running binary, embedded by `build.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Notified of every release, pre-releases included.
    Dev,
    /// Notified of stable releases only.
    Stable,
}

impl Channel {
    pub fn current() -> Self {
        Self::from_build(env!("GRAPHENE_BUILD_CHANNEL"))
    }

    fn from_build(value: &str) -> Self {
        if value == "stable" {
            Self::Stable
        } else {
            Self::Dev
        }
    }
}

/// A release to notify: its tag and its GitHub page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub tag: String,
    pub url: String,
}

/// The fields of a GitHub release used by the check.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
}

/// The running binary's version.
pub fn current_version() -> Version {
    Version::parse(crate::VERSION).expect("GRAPHENE_VERSION is a valid semver version")
}

/// Parses a release tag: `v0.1.0-dev.20261112093000` or `0.1.0`.
pub fn parse_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

/// Time between two checks. `auto` is 6 hours for a dev build, 24 hours for a stable one.
pub fn interval(interval: UpdateInterval, channel: Channel) -> Duration {
    let secs = match interval {
        UpdateInterval::Auto => match channel {
            Channel::Dev => 6 * HOUR,
            Channel::Stable => 24 * HOUR,
        },
        UpdateInterval::Hours6 => 6 * HOUR,
        UpdateInterval::Hours12 => 12 * HOUR,
        UpdateInterval::Hours24 => 24 * HOUR,
        UpdateInterval::Days2 => 2 * DAY,
        UpdateInterval::Days3 => 3 * DAY,
        UpdateInterval::Week1 => 7 * DAY,
    };
    Duration::from_secs(secs)
}

/// Unix time of the next check after a check made at `last_check`.
pub fn due(last_check: u64, interval: Duration) -> u64 {
    last_check.saturating_add(interval.as_secs())
}

/// The release to show: `latest` when it is newer than `current` and not the ignored
/// tag. A tag that is not a version is never shown.
pub fn notice(
    current: &Version,
    latest: Option<&Release>,
    ignored: Option<&str>,
) -> Option<Release> {
    let latest = latest?;
    if ignored == Some(latest.tag.as_str()) {
        return None;
    }
    let version = parse_tag(&latest.tag)?;
    (version > *current).then(|| latest.clone())
}

/// The highest version among `releases`, drafts and tags that are not a version
/// excluded. Pre-releases and stable releases compete: `0.1.0` beats `0.1.0-dev.X`.
fn select_latest(releases: &[GithubRelease]) -> Option<Release> {
    releases
        .iter()
        .filter(|release| !release.draft)
        .filter_map(|release| Some((parse_tag(&release.tag_name)?, release)))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, release)| release.into())
}

impl From<&GithubRelease> for Release {
    fn from(release: &GithubRelease) -> Self {
        Release {
            tag: release.tag_name.clone(),
            url: release.html_url.clone(),
        }
    }
}

/// Picks the release to compare against from a GitHub response body: the list of
/// releases for a dev build, the latest stable release for a stable build.
fn parse_response(channel: Channel, body: &str) -> anyhow::Result<Option<Release>> {
    match channel {
        Channel::Dev => {
            let releases: Vec<GithubRelease> = serde_json::from_str(body)?;
            Ok(select_latest(&releases))
        }
        Channel::Stable => {
            let release: GithubRelease = serde_json::from_str(body)?;
            Ok(select_latest(std::slice::from_ref(&release)))
        }
    }
}

/// `owner/name` of a GitHub repository URL.
fn repository_path(url: &str) -> Option<&str> {
    let path = url.strip_prefix("https://github.com/")?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    (path.split('/').count() == 2).then_some(path)
}

/// The GitHub API endpoint queried by `channel`.
fn releases_url(channel: Channel) -> String {
    let repository = repository_path(env!("CARGO_PKG_REPOSITORY"))
        .expect("Cargo.toml `repository` is a GitHub repository URL");
    let base = format!("https://api.github.com/repos/{repository}/releases");
    match channel {
        Channel::Dev => format!("{base}?per_page={RELEASES_PER_PAGE}"),
        Channel::Stable => format!("{base}/latest"),
    }
}

/// Asks GitHub for the newest release of `channel`. `Ok(None)` when there is none.
pub fn fetch_latest(channel: Channel) -> anyhow::Result<Option<Release>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let url = releases_url(channel);
    let mut response = agent
        .get(&url)
        .header("User-Agent", format!("graphene/{}", crate::VERSION))
        .header("Accept", "application/vnd.github+json")
        .call()
        .with_context(|| format!("GET {url}"))?;
    let status = response.status().as_u16();
    // `/releases/latest` answers 404 when there is no stable release yet.
    if status == 404 && channel == Channel::Stable {
        return Ok(None);
    }
    anyhow::ensure!(status == 200, "GET {url}: HTTP {status}");
    let body = response
        .body_mut()
        .read_to_string()
        .with_context(|| format!("GET {url}: reading the body"))?;
    parse_response(channel, &body).with_context(|| format!("GET {url}: parsing the body"))
}

/// Runs a check at `now` with `fetch` and records its result in `state`. A failed check
/// is logged at `debug` and leaves `state` unchanged, so the next one waits for the next
/// interval only if it succeeds. Returns whether `state` changed.
pub fn refresh(
    state: &mut State,
    now: u64,
    fetch: impl FnOnce() -> anyhow::Result<Option<Release>>,
) -> bool {
    match fetch() {
        Ok(latest) => {
            state.last_check = now;
            state.latest = latest;
            true
        }
        Err(error) => {
            log::debug!("update check failed: {error:#}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("releases.fixture.json");

    fn release(tag: &str) -> Release {
        Release {
            tag: tag.to_owned(),
            url: format!("https://github.com/marmotz/graphene-term/releases/tag/{tag}"),
        }
    }

    fn github(tag: &str, draft: bool) -> GithubRelease {
        GithubRelease {
            tag_name: tag.to_owned(),
            html_url: release(tag).url,
            draft,
        }
    }

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn current_build_is_a_version() {
        assert_eq!(current_version().to_string(), crate::VERSION);
    }

    #[test]
    fn channel_from_build() {
        assert_eq!(Channel::from_build("stable"), Channel::Stable);
        assert_eq!(Channel::from_build("dev"), Channel::Dev);
    }

    #[test]
    fn parse_tag_strips_the_v_prefix() {
        assert_eq!(parse_tag("v0.1.0"), Some(version("0.1.0")));
        assert_eq!(
            parse_tag("0.1.0-dev.20261112093000"),
            Some(version("0.1.0-dev.20261112093000"))
        );
        assert_eq!(parse_tag("nightly"), None);
    }

    #[test]
    fn notice_newer_release() {
        let current = version("0.1.0-dev.20261101000000");
        let latest = release("v0.1.0-dev.20261112000000");
        assert_eq!(notice(&current, Some(&latest), None), Some(latest));
    }

    #[test]
    fn notice_equal_or_older_release() {
        let current = version("0.1.0-dev.20261112000000");
        let equal = release("v0.1.0-dev.20261112000000");
        let older = release("v0.1.0-dev.20261101000000");
        assert_eq!(notice(&current, Some(&equal), None), None);
        assert_eq!(notice(&current, Some(&older), None), None);
        assert_eq!(notice(&current, None, None), None);
    }

    #[test]
    fn notice_ignored_release() {
        let current = version("0.1.0");
        let latest = release("v0.2.0");
        assert_eq!(notice(&current, Some(&latest), Some("v0.2.0")), None);
    }

    #[test]
    fn notice_release_newer_than_the_ignored_one() {
        let current = version("0.1.0");
        let latest = release("v0.3.0");
        assert_eq!(
            notice(&current, Some(&latest), Some("v0.2.0")),
            Some(latest)
        );
    }

    #[test]
    fn notice_stable_release_newer_than_dev() {
        let current = version("0.1.0-dev.20261112000000");
        let latest = release("v0.1.0");
        assert_eq!(notice(&current, Some(&latest), None), Some(latest));
    }

    #[test]
    fn notice_skips_a_tag_that_is_not_a_version() {
        let current = version("0.1.0");
        assert_eq!(notice(&current, Some(&release("latest")), None), None);
    }

    #[test]
    fn select_latest_takes_the_highest_version() {
        let releases = [
            github("v0.1.0-dev.20261101000000", false),
            github("v0.1.0", false),
            github("v0.1.0-dev.20261112000000", false),
        ];
        assert_eq!(select_latest(&releases), Some(release("v0.1.0")));
    }

    #[test]
    fn select_latest_skips_drafts_and_unparsable_tags() {
        let releases = [
            github("v0.1.0-dev.20261101000000", false),
            github("v0.2.0", true),
            github("nightly", false),
        ];
        assert_eq!(
            select_latest(&releases),
            Some(release("v0.1.0-dev.20261101000000"))
        );
        assert_eq!(select_latest(&[github("nightly", false)]), None);
        assert_eq!(select_latest(&[]), None);
    }

    #[test]
    fn parse_recorded_github_response() {
        assert_eq!(
            parse_response(Channel::Dev, FIXTURE).unwrap(),
            Some(release("v0.1.0-dev.4"))
        );
    }

    #[test]
    fn parse_latest_release_response() {
        let releases: Vec<serde_json::Value> = serde_json::from_str(FIXTURE).unwrap();
        let mut latest = releases[1].clone();
        latest["tag_name"] = "v0.1.0".into();
        latest["html_url"] = release("v0.1.0").url.into();
        let body = serde_json::to_string(&latest).unwrap();
        assert_eq!(
            parse_response(Channel::Stable, &body).unwrap(),
            Some(release("v0.1.0"))
        );
    }

    #[test]
    fn parse_invalid_response() {
        assert!(parse_response(Channel::Dev, "{\"message\": \"Not Found\"}").is_err());
        assert!(parse_response(Channel::Stable, "<html>").is_err());
    }

    #[test]
    fn repository_from_cargo_toml() {
        assert_eq!(
            repository_path(env!("CARGO_PKG_REPOSITORY")),
            Some("marmotz/graphene-term")
        );
        assert_eq!(
            repository_path("https://github.com/owner/name.git/"),
            Some("owner/name")
        );
        assert_eq!(repository_path("https://gitlab.com/owner/name"), None);
        assert_eq!(repository_path("https://github.com/owner"), None);
    }

    #[test]
    fn endpoints_per_channel() {
        assert_eq!(
            releases_url(Channel::Dev),
            "https://api.github.com/repos/marmotz/graphene-term/releases?per_page=30"
        );
        assert_eq!(
            releases_url(Channel::Stable),
            "https://api.github.com/repos/marmotz/graphene-term/releases/latest"
        );
    }

    #[test]
    fn interval_auto_depends_on_the_channel() {
        assert_eq!(
            interval(UpdateInterval::Auto, Channel::Dev),
            Duration::from_secs(6 * 3_600)
        );
        assert_eq!(
            interval(UpdateInterval::Auto, Channel::Stable),
            Duration::from_secs(24 * 3_600)
        );
    }

    #[test]
    fn fixed_intervals_ignore_the_channel() {
        let expected = [
            (UpdateInterval::Hours6, 6),
            (UpdateInterval::Hours12, 12),
            (UpdateInterval::Hours24, 24),
            (UpdateInterval::Days2, 48),
            (UpdateInterval::Days3, 72),
            (UpdateInterval::Week1, 168),
        ];
        for (value, hours) in expected {
            for channel in [Channel::Dev, Channel::Stable] {
                assert_eq!(interval(value, channel), Duration::from_secs(hours * 3_600));
            }
        }
    }

    #[test]
    fn due_is_one_interval_after_the_last_check() {
        let six_hours = Duration::from_secs(6 * 3_600);
        assert_eq!(due(1_000, six_hours), 1_000 + 6 * 3_600);
        // Never checked: due at once.
        assert_eq!(due(0, six_hours), 6 * 3_600);
        assert_eq!(due(u64::MAX, six_hours), u64::MAX);
    }

    #[test]
    fn refresh_records_a_successful_check() {
        let mut state = State {
            ignored: Some("v0.2.0".to_owned()),
            ..State::default()
        };
        assert!(refresh(&mut state, 5_000, || Ok(Some(release("v0.3.0")))));
        assert_eq!(
            state,
            State {
                last_check: 5_000,
                latest: Some(release("v0.3.0")),
                ignored: Some("v0.2.0".to_owned()),
            }
        );
        assert!(refresh(&mut state, 6_000, || Ok(None)));
        assert_eq!(state.last_check, 6_000);
        assert_eq!(state.latest, None);
    }

    #[test]
    fn refresh_keeps_the_state_on_error() {
        let mut state = State {
            last_check: 1_000,
            latest: Some(release("v0.2.0")),
            ignored: None,
        };
        let before = state.clone();
        assert!(!refresh(&mut state, 5_000, || anyhow::bail!("offline")));
        assert_eq!(state, before);
    }

    /// Calls the real GitHub API: run by hand with `cargo test -- --ignored`.
    #[test]
    #[ignore = "network"]
    fn fetch_latest_from_github() {
        let latest = fetch_latest(Channel::Dev).unwrap().unwrap();
        assert!(parse_tag(&latest.tag).is_some(), "{latest:?}");
        fetch_latest(Channel::Stable).unwrap();
    }
}
