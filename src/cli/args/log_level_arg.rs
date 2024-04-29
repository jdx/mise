use clap::{Arg, ArgAction};

#[derive(Clone)]
pub struct LogLevelArg;

impl LogLevelArg {
    pub fn arg() -> clap::Arg {
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
    pub fn arg() -> clap::Arg {
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
    pub fn arg() -> clap::Arg {
        Arg::new("trace")
            .long("trace")
            .help("Sets log level to trace")
            .hide(true)
            .action(ArgAction::SetTrue)
            .global(true)
    }
}
