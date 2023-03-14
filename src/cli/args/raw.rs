use clap::{Arg, ArgAction};

pub struct Raw(pub bool);

impl Raw {
    pub fn arg() -> Arg {
        Arg::new("raw")
            .short('r')
            .long("raw")
            .help("Directly pipe stdin/stdout/stderr to user.\nSets --jobs=1")
            .action(ArgAction::SetTrue)
            .global(true)
    }
}
