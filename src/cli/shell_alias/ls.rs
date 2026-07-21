use eyre::Result;
use tabled::Tabled;

use crate::config::Config;
use crate::ui::table;

/// List shell aliases
///
/// Shows the shell aliases that are set in the current directory.
/// These are defined in `mise.toml` under the `[shell_alias]` section.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct ShellAliasLs {
    /// Don't show table header
    #[clap(long)]
    pub no_header: bool,
}

impl ShellAliasLs {
    pub async fn run(self) -> Result<()> {
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
        table::default_style(&mut table, self.no_header);
        miseprintln!("{table}");
        Ok(())
    }
}

#[derive(Tabled)]
struct Row {
    alias: String,
    command: String,
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise shell-alias ls</bold>
    alias    command
    ll       ls -la
    gs       git status
"#
);
