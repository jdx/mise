use clap::builder::ValueParser;
use clap::Arg;
use log::{LevelFilter, ParseLevelError};
use once_cell::sync::Lazy;

use crate::env;

#[derive(Clone)]
pub struct LogLevel(pub LevelFilter);

fn parse_log_level(input: &str) -> core::result::Result<LevelFilter, ParseLevelError> {
    input.parse::<LevelFilter>()
}

impl LogLevel {
    pub fn arg() -> clap::Arg {
        Arg::new("log-level")
            .long("log-level")
            .value_name("LEVEL")
            .help("Set the log output verbosity")
            .default_value(DEFAULT_LOG_LEVEL.as_str())
            .global(true)
            .value_parser(ValueParser::new(parse_log_level))
            .hide(true)
    }
}

pub static DEFAULT_LOG_LEVEL: Lazy<String> =
    Lazy::new(|| env::RTX_LOG_LEVEL.to_string().to_lowercase());
