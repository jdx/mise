use std::sync::Arc;

use eyre::Result;
use jiff::Timestamp;
use serde::Serialize;

use crate::backend::{Backend, VersionInfo};
use crate::cli::args::ToolArg;
use crate::config::Settings;
use crate::duration::parse_into_timestamp;
use crate::install_before::resolve_before_date_for_backend;
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
    /// Pre-release flag, sourced from upstream metadata or backend opt-in
    /// detection. Always emitted so JSON consumers can rely on its presence.
    prerelease: bool,
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

    /// Only show versions released before this age or date
    ///
    /// Supports absolute dates like "2024-06-01" and relative durations like "90d" or "1y".
    #[clap(
        long,
        alias = "before",
        value_name = "MINIMUM_RELEASE_AGE",
        verbatim_doc_comment
    )]
    pub minimum_release_age: Option<String>,

    /// Output in JSON format (includes version metadata like created_at timestamps when available)
    #[clap(short = 'J', long, verbatim_doc_comment)]
    pub json: bool,

    /// Disable checking the mise-versions host
    #[clap(long, verbatim_doc_comment)]
    pub no_versions_host: bool,

    /// Include pre-release versions in the output for backends that report
    /// upstream prerelease metadata or opt in to regex-based prerelease
    /// detection. Equivalent to setting `MISE_PRERELEASES=1` or the
    /// `prereleases` setting for the duration of this command.
    #[clap(long, verbatim_doc_comment)]
    pub prerelease: bool,

    /// Fail if release metadata fetches fail
    ///
    /// Requires --json and --no-versions-host.
    ///
    /// This prevents metadata consumers from accepting empty fallback results
    /// when a backend's metadata-producing upstream request fails.
    #[clap(long, verbatim_doc_comment, requires_all = ["json", "no_versions_host"])]
    pub strict_metadata: bool,
}

impl LsRemote {
    pub async fn run(self) -> Result<()> {
        if self.prerelease {
            Settings::override_with(|s| s.prereleases = Some(true));
        }
        if self.no_versions_host {
            Settings::override_with(|s| s.use_versions_host = Some(false));
        }
        backend::set_strict_metadata(self.strict_metadata);
        let config = Config::get().await?;
        let before_date = self
            .minimum_release_age
            .as_deref()
            .map(parse_into_timestamp)
            .transpose()?;
        if let Some(plugin) = self.get_plugin(&config).await? {
            self.run_single(&config, plugin, before_date).await
        } else {
            self.run_all(&config, before_date).await
        }
    }

    async fn run_single(
        self,
        config: &Arc<Config>,
        plugin: Arc<dyn Backend>,
        before_date: Option<Timestamp>,
    ) -> Result<()> {
        let before_date =
            resolve_before_date_for_backend(config, plugin.as_ref(), before_date).await?;
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

        let versions = filter_versions_by_date(
            plugin.list_remote_versions_with_info(config).await?,
            before_date,
        )
        .into_iter()
        .filter(|v| matches_prefix(&v.version))
        .collect::<Vec<_>>();

        if self.json {
            miseprintln!("{}", serde_json::to_string(&versions)?);
        } else {
            for v in versions {
                miseprintln!("{}", v.version);
            }
        }
        Ok(())
    }

    async fn run_all(self, config: &Arc<Config>, before_date: Option<Timestamp>) -> Result<()> {
        let mut versions = vec![];
        for b in backend::list() {
            let tool = b.id().to_string();
            let before_date =
                resolve_before_date_for_backend(config, b.as_ref(), before_date).await?;
            for v in filter_versions_by_date(
                b.list_remote_versions_with_info(config).await?,
                before_date,
            ) {
                versions.push(VersionOutputAll {
                    tool: tool.clone(),
                    version: v.version,
                    created_at: v.created_at,
                    prerelease: v.prerelease,
                });
            }
        }
        versions.sort_by(|a, b| a.tool.cmp(&b.tool));

        if self.json {
            miseprintln!("{}", serde_json::to_string(&versions)?);
        } else {
            for v in versions {
                miseprintln!("{}@{}", v.tool, v.version);
            }
        }
        Ok(())
    }

    async fn get_plugin(&self, config: &Arc<Config>) -> Result<Option<Arc<dyn Backend>>> {
        match &self.plugin {
            Some(tool_arg) => {
                let mut backend = tool_arg.ba.backend()?;
                let mpr = MultiProgressReport::get();
                if let Some(plugin) = backend.plugin() {
                    plugin.ensure_installed(config, &mpr, false, false).await?;
                    backend = tool_arg.ba.backend()?;
                }
                Ok(Some(backend))
            }
            None => Ok(None),
        }
    }
}

fn filter_versions_by_date(
    versions: Vec<VersionInfo>,
    before_date: Option<Timestamp>,
) -> Vec<VersionInfo> {
    match before_date {
        Some(before) => VersionInfo::filter_by_date(versions, before),
        None => versions,
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

    $ <bold>mise ls-remote node --minimum-release-age 2024-01-01</bold>
    20.0.0

    $ <bold>mise ls-remote github:cli/cli --json</bold>
    [{"version":"2.62.0","created_at":"2024-11-14T15:40:35Z","prerelease":false},{"version":"2.61.0","created_at":"2024-10-23T19:22:15Z","prerelease":false}]
"#
);
