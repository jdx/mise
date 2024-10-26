use clap::{Arg, ArgAction};
use once_cell::sync::Lazy;

pub struct YesArg;

pub static YES_ARG: Lazy<Arg> = Lazy::new(YesArg::arg);

impl YesArg {
    fn arg() -> Arg {
        Arg::new("yes")
            .short('y')
            .long("yes")
            .help("Answer yes to all confirmation prompts")
            .action(ArgAction::SetTrue)
            .global(true)
    }
}
