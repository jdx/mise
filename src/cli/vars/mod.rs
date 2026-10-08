use eyre::Result;

mod ls;
mod prompt;
mod unset;

/// Ask for and list the per-machine answers to `[vars]` that declare a `prompt`
///
/// A `[vars]` entry with a `prompt` is asked for once per machine and the
/// answer is remembered in `$MISE_STATE_DIR/vars.toml`, outside every config
/// file. With no subcommand, lists the saved answers (same as `mise vars ls`).
/// `mise vars prompt` asks for them or, as `NAME=VALUE`, sets them directly;
/// only it and `mise bootstrap --prompt-vars` ever ask. See
/// https://mise.jdx.dev/configuration/vars.html#prompt
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(r###"mise vars"###, help = "List the saved answers")
)]
pub(crate) struct Vars {
    #[usage(subcommand)]
    command: Option<Commands>,

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
            None => ls::list(self.no_header),
        }
    }
}
