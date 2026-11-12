// Build version helpers, shared by `build.rs` (through `include!`) and the crate's tests.
// Keep this file free of `crate::` paths and `use` items so it compiles in both places.

/// Release channel of a build, selected by `GRAPHENE_CHANNEL` at build time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildChannel {
    Dev,
    Stable,
}

impl BuildChannel {
    /// Parses `GRAPHENE_CHANNEL`: unset or `dev` is a dev build, `stable` a stable one.
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("") | Some("dev") => Ok(Self::Dev),
            Some("stable") => Ok(Self::Stable),
            Some(other) => Err(format!(
                "invalid GRAPHENE_CHANNEL {other:?}: expected \"dev\" or \"stable\""
            )),
        }
    }
}

/// Converts a number of days since 1970-01-01 to a proleptic Gregorian
/// `(year, month, day)` (Howard Hinnant's `civil_from_days`).
pub fn days_to_civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Formats a Unix timestamp as a UTC `YYYYMMDDHHMMSS` string.
pub fn utc_timestamp(epoch_secs: u64) -> String {
    let secs = epoch_secs as i64;
    let (year, month, day) = days_to_civil(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{year:04}{month:02}{day:02}{:02}{:02}{:02}",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// The version embedded in the binary: `<pkg>-dev.<UTC timestamp>` for a dev build,
/// `<pkg>` for a stable one.
pub fn build_version(pkg_version: &str, channel: BuildChannel, epoch_secs: u64) -> String {
    match channel {
        BuildChannel::Dev => format!("{pkg_version}-dev.{}", utc_timestamp(epoch_secs)),
        BuildChannel::Stable => pkg_version.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_to_civil_epoch_zero() {
        assert_eq!(days_to_civil(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_civil_leap_day() {
        // 2024-02-29 is day 19782 since the epoch.
        assert_eq!(days_to_civil(19_782), (2024, 2, 29));
        assert_eq!(days_to_civil(19_783), (2024, 3, 1));
        // 2000 is a leap year (divisible by 400).
        assert_eq!(days_to_civil(11_016), (2000, 2, 29));
    }

    #[test]
    fn days_to_civil_end_of_year() {
        assert_eq!(days_to_civil(20_088), (2024, 12, 31));
        assert_eq!(days_to_civil(20_089), (2025, 1, 1));
    }

    #[test]
    fn days_to_civil_before_epoch() {
        assert_eq!(days_to_civil(-1), (1969, 12, 31));
    }

    #[test]
    fn utc_timestamp_formats_date_and_time() {
        assert_eq!(utc_timestamp(0), "19700101000000");
        // 2026-11-12T16:32:49Z
        assert_eq!(utc_timestamp(1_794_501_169), "20261112163249");
        // Last second of 2024.
        assert_eq!(utc_timestamp(1_735_689_599), "20241231235959");
    }

    #[test]
    fn build_version_dev_channel() {
        assert_eq!(
            build_version("0.1.0", BuildChannel::Dev, 1_794_501_169),
            "0.1.0-dev.20261112163249"
        );
    }

    #[test]
    fn build_version_stable_channel() {
        assert_eq!(
            build_version("0.1.0", BuildChannel::Stable, 1_794_501_169),
            "0.1.0"
        );
    }

    #[test]
    fn channel_parse() {
        assert_eq!(BuildChannel::parse(None), Ok(BuildChannel::Dev));
        assert_eq!(BuildChannel::parse(Some("")), Ok(BuildChannel::Dev));
        assert_eq!(BuildChannel::parse(Some("dev")), Ok(BuildChannel::Dev));
        assert_eq!(
            BuildChannel::parse(Some("stable")),
            Ok(BuildChannel::Stable)
        );
        assert!(BuildChannel::parse(Some("beta")).is_err());
    }
}
