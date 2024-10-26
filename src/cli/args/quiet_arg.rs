use clap::{Arg, ArgAction};
use once_cell::sync::Lazy;

#[derive(Clone)]
pub struct QuietArg;

pub static QUIET_ARG: Lazy<Arg> = Lazy::new(QuietArg::arg);

impl QuietArg {
    fn arg() -> Arg {
        Arg::new("quiet")
            .short('q')
            .long("quiet")
            .help("Suppress non-error messages")
            .global(true)
            .overrides_with("verbose")
            .action(ArgAction::SetTrue)
    }
}
