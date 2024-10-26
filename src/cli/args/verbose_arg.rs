use clap::{Arg, ArgAction};
use once_cell::sync::Lazy;

#[derive(Clone)]
pub struct VerboseArg;

pub static VERBOSE_ARG: Lazy<clap::Arg> = Lazy::new(VerboseArg::arg);

impl VerboseArg {
    fn arg() -> clap::Arg {
        Arg::new("verbose")
            .short('v')
            .long("verbose")
            .help("Show extra output (use -vv for even more)")
            .global(true)
            .overrides_with("quiet")
            .action(ArgAction::Count)
    }
}
