use clap::builder::ValueParser;
use clap::Arg;
use color_eyre::eyre::eyre;
use color_eyre::eyre::Result;
use lazy_static::lazy_static;
use log::LevelFilter;

use crate::env;

#[derive(Clone)]
pub struct LogLevel(pub LevelFilter);

fn parse_log_level(input: &str) -> Result<LevelFilter> {
    match input {
        "trace" => Ok(LevelFilter::Trace),
        "debug" => Ok(LevelFilter::Debug),
        "info" => Ok(LevelFilter::Info),
        "warn" => Ok(LevelFilter::Warn),
        "error" => Ok(LevelFilter::Error),
        _ => Err(eyre!(
            "invalid log level: {}\nvalid input: trace, debug, info, warn, error",
            input
        )),
    }
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

lazy_static! {
    pub static ref DEFAULT_LOG_LEVEL: String = env::RTX_LOG_LEVEL.to_string().to_lowercase();
}
