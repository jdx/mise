use color_eyre::eyre::{Result, bail};
use jiff::Timestamp;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::install_before::resolve_cli_minimum_release_age;
use crate::toolset::{ToolRequest, resolve_sub_base};
use crate::ui::multi_progress_report::MultiProgressReport;

/// Get the latest available version of a tool
///
/// Supports prefixes such as `node@20` to get the latest version of node 20.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise latest node@20
mise latest node"###,
        help = r###"Resolve a Node 20 release, or the backend's latest stable release"###
    ),
    example(
        r###"mise latest node@20 --installed"###,
        help = r###"Restrict resolution to installed versions"###
    ),
    example(
        r###"mise latest node --minimum-release-age 30d"###,
        help = r###"Exclude releases newer than the requested age"###
    )
)]
pub(crate) struct Latest {
    /// Tool to get the latest version of
    #[usage(value_name = "TOOL@VERSION")]
    tool: ToolArg,

    /// The version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    /// used for asdf compatibility
    #[usage(hide = true)]
    asdf_version: Option<String>,

    /// Show latest installed instead of available version
    #[usage(short, long)]
    installed: bool,

    /// Only consider versions released before this date or older than this duration
    ///
    /// Supports absolute dates like "2024-06-01" and relative durations like "90d" or "1y".
    /// Overrides per-tool `minimum_release_age` options and the global `minimum_release_age` setting.
    #[usage(long, alias = "before", verbatim_doc_comment, conflicts = "installed")]
    minimum_release_age: Option<String>,
}

impl Latest {
    pub(crate) async fn run(self) -> Result<()> {
        let before_date = self.get_before_date()?;
        let config = Config::get().await?;
        let Self {
            tool,
            asdf_version,
            installed,
            minimum_release_age: _,
        } = self;
        let prefix = match &tool.tvr {
            None => asdf_version,
            Some(ToolRequest::Version { version, .. }) => Some(version.clone()),
            // `sub-N:<base>` resolves its base against the backend, so it is handled
            // below once the backend (and its plugin) is ready.
            Some(ToolRequest::Sub { .. }) => None,
            _ => bail!("invalid version: {}", tool.style()),
        };

        let ba = prefix
            .as_deref()
            .and_then(|prefix| tool.ba.with_registry_version(prefix));
        let ba = ba.as_ref().unwrap_or(&tool.ba);
        let mut backend = ba.backend()?;
        let mpr = MultiProgressReport::get();
        if let Some(plugin) = backend.plugin() {
            plugin.ensure_installed(&config, &mpr, false, false).await?;
            backend = ba.backend()?;
        }
        let prefix = match &tool.tvr {
            Some(ToolRequest::Sub {
                sub, orig_version, ..
            }) => Some(
                resolve_sub_base(&config, &backend, sub, orig_version, before_date, false).await?,
            ),
            _ => match prefix {
                Some(v) => Some(config.resolve_alias(&backend, &v).await?),
                None => None,
            },
        };

        if let Some(ba) = prefix
            .as_deref()
            .and_then(|prefix| ba.with_registry_version(prefix))
        {
            backend = ba.backend()?;
        }
        let latest_version = if installed {
            backend.latest_installed_version(prefix)?
        } else {
            backend.latest_version(&config, prefix, before_date).await?
        };
        if let Some(version) = latest_version {
            miseprintln!("{}", version);
        }
        Ok(())
    }

    /// Get the minimum_release_age cutoff from the CLI --minimum-release-age flag only.
    /// Per-tool and global setting fallbacks are handled by backend latest resolution.
    fn get_before_date(&self) -> Result<Option<Timestamp>> {
        resolve_cli_minimum_release_age(self.minimum_release_age.as_deref())
    }
}
