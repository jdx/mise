use std::io::Cursor;

use clap_complete::generate;
use color_eyre::eyre::Result;

use crate::cli::self_update::SelfUpdate;
use crate::cli::Cli;
use crate::config::Config;
use crate::output::Output;

/// Generate shell completions
#[derive(Debug, clap::Args)]
#[clap(aliases = ["complete", "completions"], verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Completion {
    /// Shell type to generate completions for
    #[clap()]
    shell: Option<clap_complete::Shell>,

    /// Shell type to generate completions for
    #[clap(long = "shell", short = 's', hide = true)]
    shell_type: Option<clap_complete::Shell>,
}

impl Completion {
    pub fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let shell = match self.shell.or(self.shell_type) {
            Some(shell) => shell,
            None => panic!("no shell provided"),
        };

        let mut c = Cursor::new(Vec::new());
        let mut cmd = Cli::command().subcommand(SelfUpdate::command());
        generate(shell, &mut cmd, "rtx", &mut c);
        rtxprintln!(out, "{}", String::from_utf8(c.into_inner()).unwrap());

        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx completion bash > /etc/bash_completion.d/rtx</bold>
  $ <bold>rtx completion zsh  > /usr/local/share/zsh/site-functions/_rtx</bold>
  $ <bold>rtx completion fish > ~/.config/fish/completions/rtx.fish</bold>
"#
);
