use crate::cli::exec::Exec;
use std::path::PathBuf;

use crate::env;

/// Start a new shell with the mise environment built from the current configuration
///
/// This is an alternative to `mise activate` for starting a mise session explicitly.
/// The new shell has the tools and environment variables from the config loaded.
/// Unlike an activated shell, changing directories does not update the environment.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise en .
node -v
v20.0.0

Skip loading bashrc:
mise en -s "bash --norc"

Skip loading zshrc:
mise en -s "zsh -f""###
    )
)]
pub(crate) struct En {
    /// Directory to start the shell in
    #[usage(default = ".", verbatim_doc_comment, value_hint = usage_rs::ValueHint::DirPath)]
    pub dir: PathBuf,

    /// Shell to start
    ///
    /// Defaults to $SHELL
    #[usage(verbatim_doc_comment, long, short = 's')]
    pub shell: Option<String>,
}

impl En {
    pub(crate) async fn run(self) -> eyre::Result<()> {
        env::set_current_dir(&self.dir)?;
        let shell = self.shell.unwrap_or((*env::SHELL).clone());
        let command = shell_words::split(&shell).map_err(|e| eyre::eyre!(e))?;

        Exec {
            tool: vec![],
            raw: false,
            jobs: None,
            c: None,
            command: Some(command),
            no_deps: false,
            fresh_env: false,
            deny_all: false,
            deny_read: false,
            deny_write: false,
            deny_net: false,
            deny_env: false,
            allow_read: vec![],
            allow_write: vec![],
            allow_net: vec![],
            allow_env: vec![],
        }
        .run()
        .await
    }
}
