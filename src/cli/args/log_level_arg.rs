use clap::{Arg, ArgAction};
use once_cell::sync::Lazy;

pub static LOG_LEVEL_ARG: Lazy<Arg> = Lazy::new(LogLevelArg::arg);
pub static DEBUG_ARG: Lazy<Arg> = Lazy::new(DebugArg::arg);
pub static TRACE_ARG: Lazy<Arg> = Lazy::new(TraceArg::arg);

#[derive(Clone)]
pub struct LogLevelArg;

impl LogLevelArg {
    fn arg() -> clap::Arg {
        Arg::new("log-level")
            .long("log-level")
            .value_name("LEVEL")
            .help("Set the log output verbosity")
            .global(true)
            .hide(true)
            .value_parser(["error", "warn", "info", "debug", "trace"])
    }
}

pub struct DebugArg;

impl DebugArg {
    fn arg() -> clap::Arg {
        Arg::new("debug")
            .long("debug")
            .help("Sets log level to debug")
            .hide(true)
            .action(ArgAction::SetTrue)
            .global(true)
    }
}

pub struct TraceArg;

impl TraceArg {
    fn arg() -> clap::Arg {
        Arg::new("trace")
            .long("trace")
            .help("Sets log level to trace")
            .hide(true)
            .action(ArgAction::SetTrue)
            .global(true)
    }
}
