use std::sync::Arc;

use eyre::Result;
use itertools::Itertools;
use rayon::prelude::*;
use tokio::runtime::Handle;

use crate::cli::args::ToolArg;
use crate::forge;
use crate::forge::Forge;
use crate::toolset::ToolRequest;
use crate::ui::multi_progress_report::MultiProgressReport;

/// List runtime versions available for install
///
/// note that the results are cached for 24 hours
/// run `mise cache clean` to clear the cache and get fresh results
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP, aliases = ["list-all", "list-remote"])]
pub struct LsRemote {
    /// Plugin to get versions for
    #[clap(value_name = "TOOL@VERSION", required_unless_present = "all")]
    pub plugin: Option<ToolArg>,

    /// Show all installed plugins and versions
    #[clap(long, verbatim_doc_comment, conflicts_with_all = ["plugin", "prefix"])]
    pub all: bool,

    /// The version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    #[clap(verbatim_doc_comment)]
    pub prefix: Option<String>,
}

impl LsRemote {
    pub async fn run(self) -> Result<()> {
        if let Some(plugin) = self.get_plugin()? {
            self.run_single(plugin).await
        } else {
            self.run_all().await
        }
    }

    async fn run_single(self, plugin: Arc<dyn Forge>) -> Result<()> {
        let prefix = match &self.plugin {
            Some(tool_arg) => match &tool_arg.tvr {
                Some(ToolRequest::Version { version: v, .. }) => Some(v.clone()),
                _ => self.prefix.clone(),
            },
            _ => self.prefix.clone(),
        };

        let versions = plugin.list_remote_versions().await?;
        let versions = match prefix {
            Some(prefix) => versions
                .into_iter()
                .filter(|v| v.starts_with(&prefix))
                .collect(),
            None => versions,
        };

        for version in versions {
            miseprintln!("{}", version);
        }

        Ok(())
    }

    async fn run_all(self) -> Result<()> {
        let handle = Handle::current();
        let versions = forge::list()
            .into_par_iter()
            .map(|p| {
                handle.block_on(async {
                    let versions = p.list_remote_versions().await?;
                    Ok((p, versions))
                })
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sorted_by_cached_key(|(p, _)| p.id().to_string())
            .collect::<Vec<_>>();
        for (plugin, versions) in versions {
            for v in versions {
                miseprintln!("{}@{v}", plugin);
            }
        }
        Ok(())
    }

    fn get_plugin(&self) -> Result<Option<Arc<dyn Forge>>> {
        match &self.plugin {
            Some(tool_arg) => {
                let plugin = forge::get(&tool_arg.forge);
                let mpr = MultiProgressReport::get();
                plugin.ensure_installed(&mpr, false)?;
                Ok(Some(plugin))
            }
            None => Ok(None),
        }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise ls-remote node</bold>
    18.0.0
    20.0.0

    $ <bold>mise ls-remote node@20</bold>
    20.0.0
    20.1.0

    $ <bold>mise ls-remote node 20</bold>
    20.0.0
    20.1.0
"#
);

#[cfg(test)]
mod tests {
    use test_log::test;

    #[test(tokio::test)]
    async fn test_list_remote() {
        assert_cli_snapshot!("list-remote", "dummy");
    }

    #[test(tokio::test)]
    async fn test_ls_remote_prefix() {
        assert_cli_snapshot!("list-remote", "dummy", "1");
        assert_cli_snapshot!("list-remote", "dummy@2");
    }
}
