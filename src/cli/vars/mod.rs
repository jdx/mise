use eyre::Result;

mod ls;
mod prompt;
mod unset;

/// Ask for and manage the per-machine answers to `[vars]` that declare a `prompt`
///
/// A `[vars]` entry with a `prompt` is asked for once per machine and the
/// answer is remembered in `$MISE_STATE_DIR/vars.toml`, outside every config
/// file. Only `mise vars prompt` and `mise bootstrap --prompt-vars` ask. See
/// https://mise.jdx.dev/configuration/vars.html#prompt
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct Vars {
    #[usage(subcommand)]
    command: Commands,
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
            Commands::Ls(cmd) => cmd.run(),
            Commands::Prompt(cmd) => cmd.run().await,
            Commands::Unset(cmd) => cmd.run(),
        }
    }
}
