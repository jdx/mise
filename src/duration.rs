pub use std::time::Duration;

use eyre::{Result, bail};
use jiff::{Span, civil::date};

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
