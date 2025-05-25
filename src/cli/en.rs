use crate::cli::exec::Exec;
use crate::config::Settings;
use std::path::PathBuf;

use crate::env;

/// [experimental] starts a new shell with the mise environment built from the current configuration
///
/// This is an alternative to `mise activate` that allows you to explicitly start a mise session.
/// It will have the tools and environment variables in the configs loaded.
/// Note that changing directories will not update the mise environment.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct En {
    /// Directory to start the shell in
    #[clap(default_value = ".", verbatim_doc_comment, value_hint = clap::ValueHint::DirPath)]
    pub dir: PathBuf,

    /// Shell to start
    ///
    /// Defaults to $SHELL
    #[clap(verbatim_doc_comment, long, short = 's')]
    pub shell: Option<String>,
}

impl En {
    pub async fn run(self) -> eyre::Result<()> {
        let settings = Settings::get();
        settings.ensure_experimental("en")?;

        env::set_current_dir(&self.dir)?;
        let shell = self.shell.unwrap_or((*env::SHELL).clone());
        let command = shell_words::split(&shell).map_err(|e| eyre::eyre!(e))?;

        Exec {
            tool: vec![],
            raw: false,
            jobs: None,
            c: None,
            command: Some(command),
        }
        .run()
        .await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise en .</bold>
    $ <bold>node -v</bold>
    v20.0.0

    Skip loading bashrc:
    $ <bold>mise en -s "bash --norc"</bold>

    Skip loading zshrc:
    $ <bold>mise en -s "zsh -f"</bold>
"#
);
