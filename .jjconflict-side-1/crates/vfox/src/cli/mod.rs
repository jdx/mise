use usage_rs::Subcommands;
use vfox::VfoxResult;

mod available;
mod env_keys;
mod install;
mod plugins;

#[derive(usage_rs::Cli)]
#[usage(name = "vfox", version, unknown_flags = "error")]
pub(crate) struct Cli {
    #[usage(subcommand)]
    command: Commands,
}

#[derive(Subcommands)]
enum Commands {
    Available(available::Available),
    EnvKeys(env_keys::EnvKeys),
    Install(install::Install),
    #[usage(alias = "plugin")]
    Plugins(plugins::Plugins),
}

impl Commands {
    pub(crate) async fn run(self) -> VfoxResult<()> {
        match self {
            Commands::Available(available) => available.run().await,
            Commands::EnvKeys(env_keys) => env_keys.run().await,
            Commands::Install(install) => install.run().await,
            Commands::Plugins(plugins) => plugins.run().await,
        }
    }
}

pub(crate) async fn run() -> VfoxResult<()> {
    Cli::parse().command.run().await
}
