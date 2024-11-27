use clap::{Arg, ArgAction};
use once_cell::sync::Lazy;

#[derive(Clone, Debug)]
pub struct ProfileArg;

pub static PROFILE_ARG: Lazy<Arg> = Lazy::new(ProfileArg::arg);
pub static ENV_ARG: Lazy<Arg> =
    Lazy::new(|| clap::arg!(-E --env "Set the environment for loading mise.<ENV>.toml files."));

impl ProfileArg {
    fn arg() -> Arg {
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
