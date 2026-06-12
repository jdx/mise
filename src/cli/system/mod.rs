use clap::Subcommand;
use eyre::Result;

mod driver;
mod install;
mod status;
mod upgrade;
#[path = "use.rs"]
mod r#use;

/// [experimental] Manage system packages from `[system.packages]` and macOS
/// defaults from `[system.defaults]`
///
/// System packages are machine-global packages installed by the OS package
/// manager (apt, dnf, pacman) or mise's Homebrew-bottle installer (brew).
/// macOS defaults are user preferences written with `defaults write`.
/// Unlike `[tools]`, they are not version-pinned per-project and are only
/// ever applied when explicitly requested with `mise system install`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct System {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Install(install::SystemInstall),
    Status(status::SystemStatus),
    Upgrade(upgrade::SystemUpgrade),
    Use(r#use::SystemUse),
}

impl System {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Install(cmd) => cmd.run().await,
            Commands::Status(cmd) => cmd.run().await,
            Commands::Upgrade(cmd) => cmd.run().await,
            Commands::Use(cmd) => cmd.run().await,
        }
    }
}
