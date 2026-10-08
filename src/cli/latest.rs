use color_eyre::eyre::{Result, bail};
use jiff::Timestamp;

use crate::args::ToolArg;
use crate::config::Config;
use crate::install_before::resolve_cli_minimum_release_age;
use crate::toolset::{ToolRequest, resolve_sub_base};
use crate::ui::multi_progress_report::MultiProgressReport;

/// Print the latest version that matches a version request
///
/// `mise latest node` prints the latest release, and `mise latest node@22` the
/// latest 22.x release. Each backend decides what "latest" means, for example
/// whether prereleases count and how channels and refs resolve. Nothing is
/// installed and no config changes.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example("mise latest node", help = "Print the latest node release"),
    example("mise latest node@22", help = "Print the latest node 22.x release"),
    example(
        "mise latest node@22 --installed",
        help = "Print the latest installed node 22.x version"
    ),
    example(
        "mise latest node --minimum-release-age 30d",
        help = "Skip releases from the last 30 days"
    )
)]
pub(crate) struct Latest {
    /// Tool, optionally with a version prefix, such as `node` or `node@22`
    #[usage(value_name = "TOOL@VERSION")]
    tool: ToolArg,

    /// The version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    /// used for asdf compatibility
    #[usage(hide = true)]
    asdf_version: Option<String>,

    /// Print the latest installed version instead of the latest available one
    #[usage(short, long)]
    installed: bool,

    /// Only consider versions released before a date or at least a duration ago
    ///
    /// Takes a date such as `2024-06-01` or a duration such as `90d` or `1y`.
    /// Overrides the `minimum_release_age` setting and tool option.
    #[usage(
        long,
        alias = "before",
        value_name = "AGE",
        verbatim_doc_comment,
        conflicts = "installed"
    )]
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
            // `prefix:20` is a plain prefix: the backend's version matching resolves it.
            Some(ToolRequest::Prefix { prefix, .. }) => Some(prefix.clone()),
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
            // `prefix:` asks for a prefix match, never an alias, as `ToolVersion::resolve` treats it.
            Some(ToolRequest::Prefix { .. }) => prefix,
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
