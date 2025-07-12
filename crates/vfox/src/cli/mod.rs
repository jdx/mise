use clap::Parser;
use vfox::VfoxResult;

mod available;
mod env_keys;
mod install;
mod plugins;

#[derive(Parser)]
#[command(version)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    Available(available::Available),
    EnvKeys(env_keys::EnvKeys),
    Install(install::Install),
    #[command(alias = "plugin")]
    Plugins(plugins::Plugins),
}

impl Commands {
    pub async fn run(self) -> VfoxResult<()> {
        match self {
            Commands::Available(available) => available.run().await,
            Commands::EnvKeys(env_keys) => env_keys.run().await,
            Commands::Install(install) => install.run().await,
            Commands::Plugins(plugins) => plugins.run().await,
        }
    }
}

pub async fn run() -> VfoxResult<()> {
    Cli::parse().command.run().await
}
