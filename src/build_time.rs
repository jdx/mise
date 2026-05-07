use jiff::Timestamp;
use std::sync::LazyLock as Lazy;

pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

pub static BUILD_TIME: Lazy<Timestamp> = Lazy::new(|| {
    Timestamp::strptime("%a, %-d %b %Y %H:%M:%S %z", env!("MISE_BUILD_TIME_UTC")).unwrap()
});

pub static TARGET: &str = built_info::TARGET;
