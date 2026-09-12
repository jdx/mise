use eyre::Result;
use itertools::Itertools;
use tabled::Tabled;

use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::ui::table;

/// List tool version aliases
///
/// Aliases can be defined in user config or provided by plugins via `bin/list-aliases`.
///
/// In user config, aliases are defined like the following in `~/.config/mise/config.toml`:
///
///     [tool_alias.node.versions]
///     project = "20"
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "list",
    example(
        r###"mise tool-alias ls
node  lts-jod      22"###
    ),
    verbatim_doc_comment
)]
pub(super) struct ToolAliasLs {
    /// Show aliases for <TOOL>
    #[usage()]
    pub tool: Option<BackendArg>,

    /// Don't show table header
    #[usage(long)]
    pub no_header: bool,
}

impl ToolAliasLs {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let rows = config
            .all_aliases
            .iter()
            .filter(|(short, _)| {
                self.tool.is_none() || self.tool.as_ref().is_some_and(|f| &f.short == *short)
            })
            .sorted_by(|(a, _), (b, _)| a.cmp(b))
            .flat_map(|(short, aliases)| {
                aliases
                    .versions
                    .iter()
                    .filter(|(from, _to)| short != "node" || !from.starts_with("lts/"))
                    .map(|(from, to)| Row {
                        tool: short.clone(),
                        alias: from.clone(),
                        version: to.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut table = tabled::Table::new(rows);
        table::print(&mut table, self.no_header)?;
        Ok(())
    }
}

#[derive(Tabled)]
struct Row {
    tool: String,
    alias: String,
    version: String,
}
