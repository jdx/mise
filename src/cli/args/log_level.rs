use clap::{Arg, ArgAction};
use log::LevelFilter;
use once_cell::sync::Lazy;

use crate::env;

#[derive(Clone)]
pub struct LogLevel(pub LevelFilter);

impl LogLevel {
    pub fn arg() -> clap::Arg {
        Arg::new("log-level")
            .long("log-level")
            .value_name("LEVEL")
            .help("Set the log output verbosity")
            .default_value(DEFAULT_LOG_LEVEL.as_str())
            .global(true)
            .hide(true)
            .value_parser(["error", "warn", "info", "debug", "trace"])
    }
}

pub static DEFAULT_LOG_LEVEL: Lazy<String> =
    Lazy::new(|| env::RTX_LOG_LEVEL.to_string().to_lowercase());

pub struct Debug;

impl Debug {
    pub fn arg() -> clap::Arg {
        Arg::new("debug")
            .long("debug")
            .help("Sets log level to debug")
            .hide(true)
            .action(ArgAction::SetTrue)
            .global(true)
    }
}

pub struct Trace;

impl Trace {
    pub fn arg() -> clap::Arg {
        Arg::new("trace")
            .long("trace")
            .help("Sets log level to trace")
            .hide(true)
            .action(ArgAction::SetTrue)
            .global(true)
    }
}
