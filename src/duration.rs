pub use std::time::Duration;

use eyre::{Result, bail};
use jiff::{Span, Timestamp, Zoned, civil::date};

pub const HOURLY: Duration = Duration::from_secs(60 * 60);
pub const DAILY: Duration = Duration::from_secs(60 * 60 * 24);
pub const WEEKLY: Duration = Duration::from_secs(60 * 60 * 24 * 7);

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

    // Try parsing as duration and subtract from now
    if let Ok(span) = s.parse::<Span>() {
        // Validate that duration is positive (negative would result in future date)
        let duration = span.to_duration(date(2025, 1, 1))?;
        if duration.is_negative() {
            bail!("duration must not be negative: {}", s);
        }
        let now = Timestamp::now();
        // Convert to Zoned to support calendar units (days, months, years)
        let now_zoned = now.to_zoned(jiff::tz::TimeZone::UTC);
        let past = now_zoned.checked_sub(span)?;
        return Ok(past.timestamp());
    }

    bail!(
        "Invalid date or duration: {s}. Expected formats: '2024-06-01', '2024-06-01T12:00:00Z', '90d', '1y'"
    )
}
