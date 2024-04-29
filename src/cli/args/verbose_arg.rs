use clap::{Arg, ArgAction};

#[derive(Clone)]
pub struct VerboseArg;

impl VerboseArg {
    pub fn arg() -> clap::Arg {
        Arg::new("verbose")
            .short('v')
            .long("verbose")
            .help("Show extra output (use -vv for even more)")
            .global(true)
            .overrides_with("quiet")
            .action(ArgAction::Count)
    }
}
