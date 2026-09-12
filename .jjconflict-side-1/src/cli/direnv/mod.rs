use std::sync::Arc;

use eyre::Result;

use crate::config::Config;

mod activate;
mod envrc;
mod exec;

/// Output direnv function to use mise inside direnv
///
/// See https://mise.jdx.dev/direnv.html for more information
///
/// Because this generates the idiomatic files based on currently installed plugins,
/// you should run this command after installing new plugins. Otherwise
/// direnv may not know to update environment variables when idiomatic file versions change.
#[derive(Debug, usage_rs::Args)]
#[usage(hide = true, verbatim_doc_comment)]
pub(crate) struct Direnv {
    #[usage(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Activate(activate::DirenvActivate),
    Envrc(envrc::Envrc),
    Exec(exec::DirenvExec),
}

impl Commands {
    pub(crate) async fn run(self, config: &Arc<Config>) -> Result<()> {
        match self {
            Self::Activate(cmd) => cmd.run().await,
            Self::Envrc(cmd) => cmd.run(config).await,
            Self::Exec(cmd) => cmd.run(config).await,
        }
    }
}

impl Direnv {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let cmd = self
            .command
            .unwrap_or(Commands::Activate(activate::DirenvActivate {}));
        cmd.run(&config).await
    }
}
