use eyre::Result;
use tabled::Tabled;

use crate::config::Config;
use crate::ui::table;

/// List shell aliases
///
/// Lists the shell aliases configured for the current directory: the
/// `[shell_alias]` entries of every config file mise loads here.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "list",
    example(
        r###"mise shell-alias ls
alias    command
ll       ls -la
gs       git status"###,
        help = "List the aliases set in this directory"
    ),
    verbatim_doc_comment
)]
pub(super) struct ShellAliasLs {
    /// Do not print the table header
    #[usage(long)]
    pub no_header: bool,
}

impl ShellAliasLs {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let rows = config
            .shell_aliases
            .iter()
            .map(|(name, (command, _path))| Row {
                alias: name.clone(),
                command: command.clone(),
            })
            .collect::<Vec<_>>();
        let mut table = tabled::Table::new(rows);
        table::print(&mut table, self.no_header)?;
        Ok(())
    }
}

#[derive(Tabled)]
struct Row {
    alias: String,
    command: String,
}
