use eyre::{Result, bail};

use crate::config::Config;
use crate::config::env_directive::prompt as answers;

mod ls;
mod prompt;
mod unset;

/// List, set, and ask for config vars
///
/// Like `mise set`: with no arguments, lists every var and where it comes from;
/// `mise vars NAME=VALUE` saves a value on this machine and `mise vars NAME`
/// prints one. Saved values live in `$MISE_STATE_DIR/vars.toml`, outside every
/// config file, and sit above a var's `default` but below any value a config
/// file sets. `mise vars prompt` asks for the `[vars]` that declare a `prompt`;
/// only it and `mise bootstrap --prompt-vars` ever ask.
/// See https://mise.jdx.dev/configuration/vars.html#prompt
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise vars git_name="Ada Lovelace""###,
        help = "Save a value on this machine without asking"
    ),
    example(
        r###"mise vars git_name"###,
        help = "Print the saved value of git_name"
    ),
    example(r###"mise vars"###, help = "List every var and its source")
)]
pub(crate) struct Vars {
    #[usage(subcommand)]
    command: Option<Commands>,

    /// Values to save (`NAME=VALUE`) or to print (`NAME`)
    #[usage(value_name = "NAME[=VALUE]")]
    args: Vec<String>,

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
            Some(Commands::Ls(cmd)) => cmd.run().await,
            Some(Commands::Prompt(cmd)) => cmd.run().await,
            Some(Commands::Unset(cmd)) => cmd.run(),
            None if self.args.is_empty() => ls::list(self.no_header).await,
            None => {
                // Validate everything first so a typo doesn't save half the list.
                let mut sets = Vec::new();
                let mut gets = Vec::new();
                for arg in &self.args {
                    match arg.split_once('=') {
                        Some(("", _)) => bail!("expected NAME or NAME=VALUE, got '{arg}'"),
                        Some((name, value)) => sets.push((name, value)),
                        None => gets.push(arg.as_str()),
                    }
                }
                for (name, value) in sets {
                    answers::set(name, value)?;
                }
                if !gets.is_empty() {
                    // Print the value templates would see, wherever it comes from.
                    answers::tolerate_missing();
                    let config = Config::reset().await?;
                    for name in gets {
                        match config.vars.get(name) {
                            Some(value) => miseprintln!("{value}"),
                            None => bail!("no var named '{name}'"),
                        }
                    }
                }
                Ok(())
            }
        }
    }
}
