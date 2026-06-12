use clap::Subcommand;
use eyre::Result;

pub(super) mod driver;
mod install;
mod status;
mod upgrade;
#[path = "use.rs"]
mod r#use;

/// [experimental] Manage system packages from `[system.packages]`, files
/// from `[system.files]`, and macOS defaults from `[system.defaults]`
///
/// System packages are machine-global packages installed by the OS package
/// manager (apt, dnf, pacman) or mise's Homebrew-bottle installer (brew).
/// System files are config files (dotfiles) symlinked, copied, or rendered
/// to machine-global paths. macOS defaults are user preferences written
/// with `defaults write`. Unlike `[tools]`, none of these are version-pinned
/// per-project and they are only ever acted on when explicitly requested
/// with `mise system install`.
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
