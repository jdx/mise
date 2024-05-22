use clap::Subcommand;
use eyre::Result;

use crate::config::Config;

mod activate;
mod envrc;
mod exec;

/// Output direnv function to use mise inside direnv
///
/// See https://mise.jdx.dev/direnv.html for more information
///
/// Because this generates the legacy files based on currently installed plugins,
/// you should run this command after installing new plugins. Otherwise
/// direnv may not know to update environment variables when legacy file versions change.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Direnv {
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Envrc(envrc::Envrc),
    Exec(exec::DirenvExec),
    Activate(activate::DirenvActivate),
}

impl Commands {
    pub async fn run(self, config: &Config) -> Result<()> {
        match self {
            Self::Activate(cmd) => cmd.run().await,
            Self::Envrc(cmd) => cmd.run(config).await,
            Self::Exec(cmd) => cmd.run(config).await,
        }
    }
}

impl Direnv {
    pub async fn run(self) -> Result<()> {
        let config = Config::try_get().await?;
        let cmd = self
            .command
            .unwrap_or(Commands::Activate(activate::DirenvActivate {}));
        cmd.run(&config).await
    }
}
