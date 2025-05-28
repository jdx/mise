use eyre::Result;
use itertools::Itertools;
use tabled::Tabled;

use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::ui::table;

/// List aliases
/// Shows the aliases that can be specified.
/// These can come from user config or from plugins in `bin/list-aliases`.
///
/// For user config, aliases are defined like the following in `~/.config/mise/config.toml`:
///
///     [alias.node.versions]
///     lts = "22.0.0"
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct AliasLs {
    /// Show aliases for <TOOL>
    #[clap()]
    pub tool: Option<BackendArg>,

    /// Don't show table header
    #[clap(long)]
    pub no_header: bool,
}

impl AliasLs {
    pub async fn run(self) -> Result<()> {
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
        table::default_style(&mut table, self.no_header);
        miseprintln!("{table}");
        Ok(())
    }
}

#[derive(Tabled)]
struct Row {
    tool: String,
    alias: String,
    version: String,
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise aliases</bold>
    node  lts-jod      22
"#
);
