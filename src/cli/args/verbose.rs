use clap::{Arg, ArgAction};

#[derive(Clone)]
pub struct Verbose(pub u8);

impl Verbose {
    pub fn arg() -> clap::Arg {
        Arg::new("verbose")
            .short('v')
            .long("verbose")
            .help("Show installation output")
            .global(true)
            .action(ArgAction::Count)
    }
}
