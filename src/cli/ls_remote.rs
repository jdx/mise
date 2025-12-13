use std::sync::Arc;

use eyre::Result;
use serde::Serialize;

use crate::backend::Backend;
use crate::cli::args::ToolArg;
use crate::toolset::{ToolRequest, tool_request};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{backend, config::Config};

/// Output struct for --all --json mode with consistent null handling
#[derive(Serialize)]
struct VersionOutputAll {
    tool: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<String>,
}

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

    /// The version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    #[clap(verbatim_doc_comment)]
    pub prefix: Option<String>,

    /// Show all installed plugins and versions
    #[clap(long, verbatim_doc_comment, conflicts_with_all = ["plugin", "prefix"])]
    pub all: bool,

    /// Output in JSON format (includes version metadata like created_at timestamps when available)
    #[clap(short = 'J', long, verbatim_doc_comment)]
    pub json: bool,
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
        let matches_prefix = |v: &str| prefix.as_ref().is_none_or(|p| v.starts_with(p));

        // Both JSON and non-JSON modes use cached list_remote_versions_with_info
        for v in plugin.list_remote_versions_with_info(config).await? {
            if matches_prefix(&v.version) {
                if self.json {
                    miseprintln!("{}", serde_json::to_string(&v)?);
                } else {
                    miseprintln!("{}", v.version);
                }
            }
        }
        Ok(())
    }

    async fn run_all(self, config: &Arc<Config>) -> Result<()> {
        // Both JSON and non-JSON modes use cached list_remote_versions_with_info
        let mut versions = vec![];
        for b in backend::list() {
            let tool = b.id().to_string();
            for v in b.list_remote_versions_with_info(config).await? {
                versions.push(VersionOutputAll {
                    tool: tool.clone(),
                    version: v.version,
                    created_at: v.created_at,
                });
            }
        }
        versions.sort_by(|a, b| a.tool.cmp(&b.tool));

        for v in versions {
            if self.json {
                miseprintln!("{}", serde_json::to_string(&v)?);
            } else {
                miseprintln!("{}@{}", v.tool, v.version);
            }
        }
        Ok(())
    }

    async fn get_plugin(&self, config: &Arc<Config>) -> Result<Option<Arc<dyn Backend>>> {
        match &self.plugin {
            Some(tool_arg) => {
                let backend = tool_arg.ba.backend()?;
                let mpr = MultiProgressReport::get();
                if let Some(plugin) = backend.plugin() {
                    plugin.ensure_installed(config, &mpr, false, false).await?;
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

    $ <bold>mise ls-remote github:cli/cli --json</bold>
    {"version":"2.62.0","created_at":"2024-11-14T15:40:35Z"}
    {"version":"2.61.0","created_at":"2024-10-23T19:22:15Z"}
"#
);
