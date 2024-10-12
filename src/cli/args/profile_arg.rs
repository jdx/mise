use clap::{Arg, ArgAction};

#[derive(Clone, Debug)]
pub struct ProfileArg;

impl ProfileArg {
    pub fn arg() -> Arg {
        Arg::new("profile")
            .short('P')
            .long("profile")
            .help("Set the profile (environment)")
            .value_parser(clap::value_parser!(String))
            .value_name("PROFILE")
            .action(ArgAction::Set)
            .global(true)
    }
}
