use clap::Subcommand;
use eyre::Result;

#[cfg(unix)]
mod brew;
pub(super) mod driver;
pub(super) mod install;
mod status;
mod upgrade;
#[path = "use.rs"]
mod r#use;

/// [experimental] Manage system packages from `[system.packages]`, macOS
/// defaults from `[system.defaults]`, and inspect `[system].login_shell`
///
/// System packages are machine-global packages installed by the OS package
/// manager (apt, dnf, pacman) or mise's Homebrew-bottle installer (brew).
/// macOS defaults are user preferences written with `defaults write`. Unlike
/// `[tools]`, packages and defaults are not version-pinned per-project and
/// are only acted on when explicitly requested with `mise system install` or
/// `mise bootstrap`. Login shell changes are applied by `mise bootstrap`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct System {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[cfg(unix)]
    Brew(brew::SystemBrew),
    Install(install::SystemInstall),
    Status(status::SystemStatus),
    Upgrade(upgrade::SystemUpgrade),
    Use(r#use::SystemUse),
}

impl System {
    pub async fn run(self) -> Result<()> {
        match self.command {
            #[cfg(unix)]
            Commands::Brew(cmd) => cmd.run().await,
            Commands::Install(cmd) => cmd.run().await,
            Commands::Status(cmd) => cmd.run().await,
            Commands::Upgrade(cmd) => cmd.run().await,
            Commands::Use(cmd) => cmd.run().await,
        }
    }
}
