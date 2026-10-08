use std::collections::HashSet;

use crate::args::ToolArg;
use crate::config::Config;
use crate::toolset::outdated_info::OutdatedInfo;
use crate::toolset::{ConfigScope, ResolveOptions, ToolsetBuilder};
use crate::ui::table;
use eyre::Result;
use indexmap::IndexMap;
use tabled::settings::Remove;
use tabled::settings::location::ByColumnName;

/// Show outdated tool versions
///
/// Lists tools whose installed version is older than the newest version their
/// config request allows. For `node = "20"`, that is the newest 20.x release;
/// pass `--bump` to compare against the newest release overall instead. Run
/// `mise upgrade` to install the newer versions.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP,
    example(r###"mise outdated
name    requested  current  latest  source
node    20         20.0.0   20.1.0  ~/src/app/mise.toml
python  3.11       3.11.0   3.11.1  ~/src/app/mise.toml"###,
        help = "Show tools with a newer version inside their configured range"),
    example(r###"mise outdated --bump
name  requested  current  bump  latest  source
node  20         20.0.0   24    24.1.0  ~/src/app/mise.toml"###,
        help = "Compare against the newest release overall and show the request for it"),
    example(r###"mise outdated --json
{
  "node": {
    "name": "node",
    "requested": "20",
    "current": "20.0.0",
    "bump": null,
    "latest": "20.1.0",
    "source": {
      "type": "mise.toml",
      "path": "/home/me/src/app/mise.toml"
    }
  }
}"###,
        help = "Print the same information as JSON"),
    example(r###"mise outdated --local"###,
        help = "Skip tools that only the global config requests"))]
pub(crate) struct Outdated {
    /// Tools to check, such as `node@20 python@3.10`
    ///
    /// Checks every tool in the global and project configs when omitted.
    #[usage(value_name = "TOOL@VERSION")]
    pub tool: Vec<ToolArg>,

    /// Compare against the newest release overall, not only the configured range
    ///
    /// With `node = "20"` in your config, `mise outdated` reports the newest 20.x
    /// release. With this flag the `latest` column shows the newest release overall,
    /// such as 24.1.0, and a `bump` column shows the request `mise upgrade --bump`
    /// would write, such as `24`.
    #[usage(long, short = 'b')]
    pub bump: bool,

    /// Output in JSON format
    #[usage(short = 'J', long, verbatim_doc_comment)]
    pub json: bool,

    /// Deprecated shorthand for --bump
    #[usage(short = 'l', hide = true)]
    pub legacy_bump: bool,

    /// Also check installed tools that the current config does not request
    ///
    /// By default, `mise outdated` checks only tools that come from the current config.
    #[usage(long, conflicts = "local")]
    pub inactive: bool,

    /// Only check tools defined in project config files
    ///
    /// Skips tools defined in the global config (~/.config/mise/config.toml) and
    /// tools set through `MISE_<TOOL>_VERSION` environment variables.
    #[usage(long)]
    pub local: bool,

    /// Placeholder for future monorepo outdated checks; `mise outdated --monorepo` is not implemented yet.
    #[usage(long, hide = true, verbatim_doc_comment)]
    pub monorepo: bool,

    /// Do not print the table header
    #[usage(long)]
    pub no_header: bool,
}

impl Outdated {
    pub(crate) async fn run(mut self) -> Result<()> {
        if self.legacy_bump {
            deprecated_at!(
                "2026.8.5",
                "2027.8.5",
                "cli.outdated.bump-l",
                "`mise outdated -l` is deprecated. Use `mise outdated -b` or `mise outdated --bump` instead. After removal, `-l` will become shorthand for `--local`."
            );
            self.bump = true;
        }
        if self.monorepo {
            eyre::bail!("--monorepo is not supported by mise outdated yet");
        }
        let config = Config::get().await?;
        let scope = if self.local {
            ConfigScope::LocalOnly
        } else {
            ConfigScope::All
        };
        let mut ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .with_scope(scope)
            .build(&config)
            .await?;
        let tool_set = self
            .tool
            .iter()
            .map(|t| t.ba.clone())
            .collect::<HashSet<_>>();
        ts.versions
            .retain(|_, tvl| tool_set.is_empty() || tool_set.contains(&tvl.backend));
        let outdated = ts
            .list_outdated_versions(
                &config,
                self.bump,
                &ResolveOptions {
                    inactive: self.inactive,
                    ..Default::default()
                },
            )
            .await;
        let bump_available = if !self.json && !self.bump && outdated.is_empty() {
            ts.list_outdated_versions(
                &config,
                true,
                &ResolveOptions {
                    inactive: self.inactive,
                    ..Default::default()
                },
            )
            .await
            .iter()
            .any(|o| o.bump.is_some())
        } else {
            false
        };
        self.display(outdated, bump_available)?;
        Ok(())
    }

    fn display(&self, outdated: Vec<OutdatedInfo>, bump_available: bool) -> Result<()> {
        match self.json {
            true => self.display_json(outdated)?,
            false => self.display_table(outdated, bump_available)?,
        }
        Ok(())
    }

    fn display_table(&self, outdated: Vec<OutdatedInfo>, bump_available: bool) -> Result<()> {
        if outdated.is_empty() {
            info!("All tools are up to date");
            if bump_available {
                info!(
                    "Newer versions are available outside the configured version ranges. Use `mise outdated --bump` to view them."
                );
            }
            return Ok(());
        }
        let mut table = tabled::Table::new(outdated);
        if !self.bump {
            table.with(Remove::column(ByColumnName::new("bump")));
        }
        table::print(&mut table, self.no_header)?;
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
    r###"<bold><underline>Deprecation:</underline></bold>

The `-l` shorthand for `--bump` is deprecated and will be removed in mise 2027.8.5.
After removal, `-l` will become shorthand for `--local`. Use `-b` or `--bump` instead."###
);
