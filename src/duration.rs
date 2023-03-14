pub use std::time::Duration;

pub(crate) const HOURLY: Duration = Duration::from_secs(60 * 60);
pub(crate) const DAILY: Duration = Duration::from_secs(60 * 60 * 24);
pub(crate) const WEEKLY: Duration = Duration::from_secs(60 * 60 * 24 * 7);
