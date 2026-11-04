use std::sync::Arc;

use eyre::Result;

use crate::config::Config;

mod activate;
mod envrc;
mod exec;

/// Print `use_mise`, a direnv function that loads mise inside direnv
///
/// Running mise inside direnv is unsupported. To move a project from direnv to mise,
/// see https://mise.jdx.dev/direnv.html.
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
            // Only the user-facing command warns. The `use_mise` hook it
            // prints runs `mise direnv exec` on every direnv reload, so a
            // warning there would print on every directory change.
            Self::Activate(cmd) => {
                deprecated_at!(
                    "2026.10.4",
                    "2027.10.4",
                    "cli.direnv",
                    "`mise direnv` and the `use mise` direnv integration are deprecated. Use `mise activate`, and move .envrc settings into mise.toml `[env]` (see https://mise.jdx.dev/direnv.html)."
                );
                cmd.run().await
            }
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
