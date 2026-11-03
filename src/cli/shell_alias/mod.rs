use eyre::Result;

mod get;
mod ls;
mod set;
mod unset;

/// Manage shell aliases
///
/// Shell aliases are defined under `[shell_alias]` in mise.toml. In a shell
/// where mise is activated, mise sets them when you enter the directory and
/// removes them when you leave. With no subcommand, lists them (same as
/// `mise shell-alias ls`). See https://mise.jdx.dev/shell-aliases.html
#[derive(Debug, usage_rs::Args)]
#[usage(name = "shell-alias", verbatim_doc_comment)]
pub(crate) struct ShellAlias {
    #[usage(subcommand)]
    command: Option<Commands>,

    /// Do not print the table header
    #[usage(long)]
    pub no_header: bool,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    Get(get::ShellAliasGet),
    Ls(ls::ShellAliasLs),
    Set(set::ShellAliasSet),
    Unset(unset::ShellAliasUnset),
}

impl Commands {
    pub(crate) async fn run(self) -> Result<()> {
        match self {
            Self::Get(cmd) => cmd.run().await,
            Self::Ls(cmd) => cmd.run().await,
            Self::Set(cmd) => cmd.run().await,
            Self::Unset(cmd) => cmd.run().await,
        }
    }
}

impl ShellAlias {
    pub(crate) async fn run(self) -> Result<()> {
        let cmd = self.command.unwrap_or(Commands::Ls(ls::ShellAliasLs {
            no_header: self.no_header,
        }));

        cmd.run().await
    }
}
