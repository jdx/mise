use clap::{Arg, ArgAction};

pub struct InstallMissing(pub bool);

impl InstallMissing {
    pub fn arg() -> Arg {
        Arg::new("install-missing")
            .long("install-missing")
            .help("Automatically install missing tools")
            .action(ArgAction::SetTrue)
            .global(true)
    }
}
