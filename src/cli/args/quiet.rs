use clap::{Arg, ArgAction};

#[derive(Clone)]
pub struct Quiet;

impl Quiet {
    pub fn arg() -> Arg {
        Arg::new("quiet")
            .short('q')
            .long("quiet")
            .help("Suppress output")
            .global(true)
            .overrides_with("verbose")
            .action(ArgAction::SetTrue)
    }
}
