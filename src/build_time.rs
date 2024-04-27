use chrono::{DateTime, FixedOffset};
use once_cell::sync::Lazy;

pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[cfg(feature = "git2")]
pub fn git_sha() -> &'static Option<&'static str> {
    &built_info::GIT_COMMIT_HASH_SHORT
}

#[cfg(not(feature = "git2"))]
pub fn git_sha() -> &'static Option<&'static str> {
    &None
}

pub static BUILD_TIME: Lazy<DateTime<FixedOffset>> =
    Lazy::new(|| DateTime::parse_from_rfc2822(built_info::BUILT_TIME_UTC).unwrap());
