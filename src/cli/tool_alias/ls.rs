use eyre::Result;
use itertools::Itertools;
use tabled::Tabled;

use crate::args::BackendArg;
use crate::config::Config;
use crate::ui::table;

/// List tool version aliases
///
/// Lists version aliases from `[tool_alias.<tool>.versions]` in any loaded
/// config file and the aliases that tool backends provide, such as node's
/// `lts-*` names or an asdf plugin's `bin/list-aliases`. For example:
///
///     [tool_alias.node.versions]
///     project = "20"
///
/// Backend aliases (`[tool_alias] tool = "backend"`) are not listed; run
/// `mise tool <TOOL>` to see the backend a tool uses.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "list",
    example(
        r###"mise tool-alias ls node
tool  alias      version
node  lts        24
node  lts-jod    22
node  project    20"###,
        help = "List node's version aliases"
    ),
    verbatim_doc_comment
)]
pub(super) struct ToolAliasLs {
    /// Only show aliases for this tool
    #[usage()]
    pub tool: Option<BackendArg>,

    /// Do not print the table header
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
