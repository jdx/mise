use clap::{Arg, ArgAction};
use std::path::PathBuf;

#[derive(Clone)]
pub struct CdArg;

impl CdArg {
    pub fn arg() -> Arg {
        Arg::new("cd")
            .value_parser(clap::value_parser!(PathBuf))
            .short('C')
            .long("cd")
            .help("Change directory before running command")
            .global(true)
            .action(ArgAction::Set)
            .value_hint(clap::ValueHint::DirPath)
            .value_name("DIR")
    }
}
