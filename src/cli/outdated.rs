use std::collections::HashSet;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::toolset::ToolsetBuilder;
use crate::toolset::outdated_info::OutdatedInfo;
use crate::ui::table;
use eyre::Result;
use indexmap::IndexMap;
use tabled::settings::Remove;
use tabled::settings::location::ByColumnName;

/// Shows outdated tool versions
///
/// See `mise upgrade` to upgrade these versions.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Outdated {
    /// Tool(s) to show outdated versions for
    /// e.g.: node@20 python@3.10
    /// If not specified, all tools in global and local configs will be shown
    #[clap(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    pub tool: Vec<ToolArg>,

    /// Compares against the latest versions available, not what matches the current config
    ///
    /// For example, if you have `node = "20"` in your config by default `mise outdated` will only
    /// show other 20.x versions, not 21.x or 22.x versions.
    ///
    /// Using this flag, if there are 21.x or newer versions it will display those instead of 20.x.
    #[clap(long, short = 'l', verbatim_doc_comment)]
    pub bump: bool,

    /// Output in JSON format
    #[clap(short = 'J', long, verbatim_doc_comment)]
    pub json: bool,

    /// Don't show table header
    #[clap(long)]
    pub no_header: bool,
}

impl Outdated {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let mut ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .build(&config)
            .await?;
        let tool_set = self
            .tool
            .iter()
            .map(|t| t.ba.clone())
            .collect::<HashSet<_>>();
        ts.versions
            .retain(|_, tvl| tool_set.is_empty() || tool_set.contains(&tvl.backend));
        let outdated = ts.list_outdated_versions(&config, self.bump).await;
        self.display(outdated).await?;
        Ok(())
    }

    async fn display(&self, outdated: Vec<OutdatedInfo>) -> Result<()> {
        match self.json {
            true => self.display_json(outdated)?,
            false => self.display_table(outdated)?,
        }
        Ok(())
    }

    fn display_table(&self, outdated: Vec<OutdatedInfo>) -> Result<()> {
        if outdated.is_empty() {
            info!("All tools are up to date");
            if !self.bump {
                hint!(
                    "outdated_bump",
                    r#"By default, `mise outdated` only shows versions that match your config. Use `mise outdated --bump` to see all new versions."#,
                    ""
                );
            }
            return Ok(());
        }
        let mut table = tabled::Table::new(outdated);
        if !self.bump {
            table.with(Remove::column(ByColumnName::new("bump")));
        }
        table::default_style(&mut table, self.no_header);
        miseprintln!("{table}");
        Ok(())
    }

    fn display_json(&self, outdated: Vec<OutdatedInfo>) -> Result<()> {
        let mut map = IndexMap::new();
        for o in outdated {
            map.insert(o.name.to_string(), o);
        }
        miseprintln!("{}", serde_json::to_string_pretty(&map)?);
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise outdated</bold>
    Plugin  Requested  Current  Latest
    python  3.11       3.11.0   3.11.1
    node    20         20.0.0   20.1.0

    $ <bold>mise outdated node</bold>
    Plugin  Requested  Current  Latest
    node    20         20.0.0   20.1.0

    $ <bold>mise outdated --json</bold>
    {"python": {"requested": "3.11", "current": "3.11.0", "latest": "3.11.1"}, ...}
"#
);
