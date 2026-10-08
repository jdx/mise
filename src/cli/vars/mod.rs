use eyre::{Result, bail};

use crate::config::env_directive::prompt as answers;

mod ls;
mod prompt;
mod unset;

/// Ask for, set, and list the per-machine answers to `[vars]` that declare a `prompt`
///
/// A `[vars]` entry with a `prompt` is asked for once per machine and the
/// answer is remembered in `$MISE_STATE_DIR/vars.toml`, outside every config
/// file. `mise vars NAME=VALUE` sets an answer without asking; with no
/// arguments, lists the saved answers (same as `mise vars ls`). Only
/// `mise vars prompt` and `mise bootstrap --prompt-vars` ask. See
/// https://mise.jdx.dev/configuration/vars.html#prompt
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise vars git_name="Ada Lovelace""###,
        help = "Set an answer without asking"
    ),
    example(r###"mise vars"###, help = "List the saved answers")
)]
pub(crate) struct Vars {
    #[usage(subcommand)]
    command: Option<Commands>,

    /// Answers to save, as NAME=VALUE
    #[usage(value_name = "NAME=VALUE")]
    assignments: Vec<String>,

    /// Do not print the table header
    #[usage(long)]
    no_header: bool,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Ls(ls::VarsLs),
    Prompt(prompt::VarsPrompt),
    Unset(unset::VarsUnset),
}

impl Vars {
    pub(crate) async fn run(self) -> Result<()> {
        match self.command {
            Some(Commands::Ls(cmd)) => cmd.run(),
            Some(Commands::Prompt(cmd)) => cmd.run().await,
            Some(Commands::Unset(cmd)) => cmd.run(),
            None if self.assignments.is_empty() => ls::list(self.no_header),
            None => {
                for assignment in &self.assignments {
                    let Some((name, value)) = assignment.split_once('=') else {
                        bail!("expected NAME=VALUE, got '{assignment}'");
                    };
                    if name.is_empty() {
                        bail!("expected NAME=VALUE, got '{assignment}'");
                    }
                    answers::set(name, value)?;
                }
                Ok(())
            }
        }
    }
}
