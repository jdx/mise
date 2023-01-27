use build_time::build_time_utc;
use chrono::{DateTime, FixedOffset, Months, Utc};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref BUILD_TIME: DateTime<FixedOffset> =
        DateTime::parse_from_rfc3339(build_time_utc!()).unwrap();
}

#[ctor::ctor]
fn init() {
    if BUILD_TIME.checked_add_months(Months::new(12)).unwrap() < Utc::now() {
        eprintln!("rtx has not been updated in over a year. Please update to the latest version.");
    }
}
