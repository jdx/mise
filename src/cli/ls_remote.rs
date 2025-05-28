use std::sync::Arc;

use eyre::Result;

use crate::backend::Backend;
use crate::cli::args::ToolArg;
use crate::toolset::{ToolRequest, tool_request};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{backend, config::Config};

/// List runtime versions available for install.
///
/// Note that the results may be cached, run `mise cache clean` to clear the cache and get fresh results.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP, aliases = ["list-all", "list-remote"]
)]
pub struct LsRemote {
    /// Tool to get versions for
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
        let config = Config::get().await?;
        if let Some(plugin) = self.get_plugin(&config).await? {
            self.run_single(&config, plugin).await
        } else {
            self.run_all(&config).await
        }
    }

    async fn run_single(self, config: &Arc<Config>, plugin: Arc<dyn Backend>) -> Result<()> {
        let prefix = match &self.plugin {
            Some(tool_arg) => match &tool_arg.tvr {
                Some(ToolRequest::Version { version: v, .. }) => Some(v.clone()),
                Some(ToolRequest::Sub {
                    sub, orig_version, ..
                }) => Some(tool_request::version_sub(orig_version, sub)),
                _ => self.prefix.clone(),
            },
            _ => self.prefix.clone(),
        };

        let versions = plugin.list_remote_versions(config).await?;
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

    async fn run_all(self, config: &Arc<Config>) -> Result<()> {
        let mut versions = vec![];
        for b in backend::list() {
            let v = b.list_remote_versions(config).await?;
            versions.extend(v.into_iter().map(|v| (b.id().to_string(), v)));
        }
        versions.sort_by_cached_key(|(id, _)| id.to_string());

        for (tool, v) in versions {
            miseprintln!("{tool}@{v}");
        }
        Ok(())
    }

    async fn get_plugin(&self, config: &Arc<Config>) -> Result<Option<Arc<dyn Backend>>> {
        match &self.plugin {
            Some(tool_arg) => {
                let backend = tool_arg.ba.backend()?;
                let mpr = MultiProgressReport::get();
                if let Some(plugin) = backend.plugin() {
                    plugin.ensure_installed(config, &mpr, false).await?;
                }
                Ok(Some(backend))
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
