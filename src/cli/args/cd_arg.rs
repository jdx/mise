use std::path::PathBuf;

use clap::{Arg, ArgAction};
use once_cell::sync::Lazy;

#[derive(Clone)]
pub struct CdArg;

pub static CD_ARG: Lazy<Arg> = Lazy::new(CdArg::arg);

impl CdArg {
    fn arg() -> Arg {
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
