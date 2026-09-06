use chrono::{DateTime, FixedOffset};
use std::sync::LazyLock as Lazy;

// `built` emits public constants because it also supports library crates. This
// module is private to the mise binary, so those generated visibilities cannot
// be narrowed at the source.
#[allow(unreachable_pub)]
pub(crate) mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

pub(crate) static BUILD_TIME: Lazy<DateTime<FixedOffset>> =
    Lazy::new(|| DateTime::parse_from_rfc2822(built_info::BUILT_TIME_UTC).unwrap());

pub(crate) static TARGET: &str = built_info::TARGET;
