use std::sync::OnceLock;
pub use std::time::Duration;

use eyre::{Result, bail};
use jiff::{Span, Timestamp, Zoned, civil::date};

pub const HOURLY: Duration = Duration::from_secs(60 * 60);
pub const DAILY: Duration = Duration::from_secs(60 * 60 * 24);
pub const WEEKLY: Duration = Duration::from_secs(60 * 60 * 24 * 7);

/// Returns a stable "now" timestamp for the lifetime of the process.
///
/// This is used for resolving relative durations (e.g. `install_before = "3d"`)
/// consistently: every resolution of the same relative duration within a single
/// mise invocation produces the same absolute timestamp, and downstream code
/// that converts the absolute timestamp back to a duration (e.g. for npm's
/// `--min-release-age`) gets the exact duration the user specified rather than
/// a slightly-larger value due to wall clock drift between phases.
pub fn process_now() -> Timestamp {
    static PROCESS_NOW: OnceLock<Timestamp> = OnceLock::new();
    *PROCESS_NOW.get_or_init(Timestamp::now)
}

pub fn parse_duration(s: &str) -> Result<Duration> {
    match s.parse::<Span>() {
        Ok(span) => {
            // we must provide a relative date to determine the duration with months and years
            let duration = span.to_duration(date(2025, 1, 1))?;
            if duration.is_negative() {
                bail!("duration must not be negative: {}", s);
            }
            Ok(duration.unsigned_abs())
        }
        Err(_) => Ok(Duration::from_secs(s.parse()?)),
    }
}

/// Parse a date/duration string into a Timestamp.
/// Supports:
/// - RFC3339 timestamps: "2024-06-01T12:00:00Z"
/// - ISO dates: "2024-06-01" (treated as end of day in UTC)
/// - Relative durations: "90d", "1y", "6m" (subtracted from now)
///
/// Relative durations are anchored to [`process_now`] so all resolutions
/// within a single mise invocation agree on "now".
pub fn parse_into_timestamp(s: &str) -> Result<Timestamp> {
    // Try RFC3339 timestamp first
    if let Ok(ts) = s.parse::<Timestamp>() {
        return Ok(ts);
    }

    // Try parsing as a Zoned datetime (handles various formats)
    if let Ok(zoned) = s.parse::<Zoned>() {
        return Ok(zoned.timestamp());
    }

    // Try parsing as date only (YYYY-MM-DD) - use end of day UTC
    if let Ok(civil_date) = s.parse::<jiff::civil::Date>() {
        let datetime = civil_date.at(23, 59, 59, 0);
        let ts = datetime.to_zoned(jiff::tz::TimeZone::UTC)?.timestamp();
        return Ok(ts);
    }

    // Try parsing as duration and subtract from the process-local "now".
    // Using a stable "now" ensures a relative duration like "3d" resolves to
    // the exact same absolute Timestamp every time within this command.
    if let Ok(span) = s.parse::<Span>() {
        // Validate that duration is positive (negative would result in future date)
        let duration = span.to_duration(date(2025, 1, 1))?;
        if duration.is_negative() {
            bail!("duration must not be negative: {}", s);
        }
        let now_zoned = process_now().to_zoned(jiff::tz::TimeZone::UTC);
        let past = now_zoned.checked_sub(span)?;
        return Ok(past.timestamp());
    }

    bail!(
        "Invalid date or duration: {s}. Expected formats: '2024-06-01', '2024-06-01T12:00:00Z', '90d', '1y'"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_now_is_stable() {
        // process_now must return the same value across calls so that relative
        // durations resolve identically every time they are evaluated.
        let a = process_now();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b = process_now();
        assert_eq!(a, b);
    }

    #[test]
    fn test_parse_into_timestamp_relative_is_stable() {
        // Two resolutions of the same relative duration must produce identical
        // timestamps within a single process invocation. This is the invariant
        // that keeps version resolution consistent with the CLI flags passed
        // to the underlying package manager (see #9156).
        let a = parse_into_timestamp("3d").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b = parse_into_timestamp("3d").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn test_parse_into_timestamp_absolute_date() {
        let ts = parse_into_timestamp("2024-01-02").unwrap();
        assert_eq!(ts.to_string(), "2024-01-02T23:59:59Z");
    }

    #[test]
    fn test_parse_into_timestamp_rfc3339() {
        let ts = parse_into_timestamp("2024-01-02T03:04:05Z").unwrap();
        assert_eq!(ts.to_string(), "2024-01-02T03:04:05Z");
    }

    #[test]
    fn test_parse_into_timestamp_rejects_garbage() {
        assert!(parse_into_timestamp("not a date").is_err());
    }
}
