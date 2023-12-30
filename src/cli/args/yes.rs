use clap::{Arg, ArgAction};

pub struct Yes(pub bool);

impl Yes {
    pub fn arg() -> Arg {
        Arg::new("yes")
            .short('y')
            .long("yes")
            .help("Answer yes to all confirmation prompts")
            .action(ArgAction::SetTrue)
            .global(true)
    }
}
