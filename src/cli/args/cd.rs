use clap::{Arg, ArgAction};

#[derive(Clone)]
pub struct Cd;

impl Cd {
    pub fn arg() -> Arg {
        Arg::new("cd")
            .short('C')
            .long("cd")
            .help("Change directory before running command")
            .global(true)
            .action(ArgAction::Set)
            .value_hint(clap::ValueHint::DirPath)
            .value_name("DIR")
    }
}
