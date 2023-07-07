use chrono::{DateTime, FixedOffset};
use once_cell::sync::Lazy;

pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

pub static BUILD_TIME: Lazy<DateTime<FixedOffset>> =
    Lazy::new(|| DateTime::parse_from_rfc2822(built_info::BUILT_TIME_UTC).unwrap());
