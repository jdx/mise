use color_eyre::eyre::{Result, bail};
use jiff::Timestamp;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::install_before::resolve_cli_minimum_release_age;
use crate::toolset::ToolRequest;
use crate::ui::multi_progress_report::MultiProgressReport;

/// Gets the latest available version for a plugin
///
/// Supports prefixes such as `node@20` to get the latest version of node 20.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Latest {
    /// Tool to get the latest version of
    #[clap(value_name = "TOOL@VERSION")]
    tool: ToolArg,

    /// The version prefix to use when querying the latest version
    /// same as the first argument after the "@"
    /// used for asdf compatibility
    #[clap(hide = true)]
    asdf_version: Option<String>,

    /// Show latest installed instead of available version
    #[clap(short, long)]
    installed: bool,

    /// Only consider versions released before this date or older than this duration
    ///
    /// Supports absolute dates like "2024-06-01" and relative durations like "90d" or "1y".
    /// Overrides per-tool `minimum_release_age` options and the global `minimum_release_age` setting.
    #[clap(
        long,
        alias = "before",
        verbatim_doc_comment,
        conflicts_with = "installed"
    )]
    minimum_release_age: Option<String>,
}

impl Latest {
    pub async fn run(self) -> Result<()> {
        let before_date = self.get_before_date()?;
        let config = Config::get().await?;
        let Self {
            tool,
            asdf_version,
            installed,
            minimum_release_age: _,
        } = self;
        let mut prefix = match &tool.tvr {
            None => asdf_version,
            Some(ToolRequest::Version { version, .. }) => Some(version.clone()),
            _ => bail!("invalid version: {}", tool.style()),
        };

        let mut backend = tool.ba.backend()?;
        let mpr = MultiProgressReport::get();
        if let Some(plugin) = backend.plugin() {
            plugin.ensure_installed(&config, &mpr, false, false).await?;
            backend = tool.ba.backend()?;
        }
        if let Some(v) = prefix {
            prefix = Some(config.resolve_alias(&backend, &v).await?);
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise latest node@20</bold>  # get the latest version of node 20
    20.0.0

    $ <bold>mise latest node</bold>     # get the latest stable version of node
    20.0.0

    $ <bold>mise latest node --minimum-release-age 2024-01-01</bold>  # latest stable node released before 2024-01-01
"#
);
