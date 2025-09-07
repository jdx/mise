use std::collections::BTreeSet;

use crate::config::Config;
use crate::file::display_path;
use crate::lockfile::Lockfile;
use crate::platform::Platform;
use crate::{cli::args::ToolArg, config::Settings};
use console::style;
use eyre::Result;

/// Update lockfile checksums and URLs for all specified platforms
///
/// Updates checksums and download URLs for all platforms already specified in the lockfile.
/// If no lockfile exists, shows what would be created based on the current configuration.
/// This allows you to refresh lockfile data for platforms other than the one you're currently on.
/// Operates on the lockfile in the current config root. Use TOOL arguments to target specific tools.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Lock {
    /// Tool(s) to update in lockfile
    /// e.g.: node python
    /// If not specified, all tools in lockfile will be updated
    #[clap(value_name = "TOOL", verbatim_doc_comment)]
    pub tool: Vec<ToolArg>,

    /// Comma-separated list of platforms to target
    /// e.g.: linux-x64,macos-arm64,windows-x64
    /// If not specified, all platforms already in lockfile will be updated
    #[clap(long, short, value_delimiter = ',', verbatim_doc_comment)]
    pub platform: Vec<String>,

    /// Update all tools even if lockfile data already exists
    #[clap(long, short, verbatim_doc_comment)]
    pub force: bool,

    /// Show what would be updated without making changes
    #[clap(long, short = 'n', verbatim_doc_comment)]
    pub dry_run: bool,

    /// Number of jobs to run in parallel
    /// [default: 4]
    #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    pub jobs: Option<usize>,
}

impl Lock {
    pub async fn run(self) -> Result<()> {
        let settings = Settings::get();
        let config = Config::get().await?;
        settings.ensure_experimental("lock")?;

        // Get lockfile path (always mise.lock in current directory)
        let lockfile_path = std::path::Path::new("mise.lock");

        // Parse target platforms if specified
        let target_platforms = if !self.platform.is_empty() {
            Platform::parse_multiple(&self.platform)?
        } else if lockfile_path.exists() {
            // If lockfile exists and no platforms specified, extract from lockfile
            let existing_lockfile = Lockfile::read(lockfile_path)?;
            self.extract_platforms(&existing_lockfile)
                .into_iter()
                .filter_map(|key| Platform::parse(&key).ok())
                .collect()
        } else {
            // Default to current platform if no lockfile exists and no platforms specified
            vec![Platform::current()]
        };

        miseprintln!(
            "{} Targeting {} platform(s): {}",
            style("→").green(),
            target_platforms.len(),
            target_platforms
                .iter()
                .map(|p| p.to_key())
                .collect::<Vec<_>>()
                .join(", ")
        );

        // Get configured tools to process
        let toolset = config.get_toolset().await?;
        let all_tool_versions = toolset.list_current_versions();

        // Filter tools based on CLI arguments
        let target_tools: Vec<_> = if !self.tool.is_empty() {
            let specified_tools: BTreeSet<String> =
                self.tool.iter().map(|t| t.ba.short.clone()).collect();

            all_tool_versions
                .into_iter()
                .filter(|(_, tv)| specified_tools.contains(&tv.ba().short))
                .map(|(_, tv)| tv)
                .collect()
        } else {
            all_tool_versions.into_iter().map(|(_, tv)| tv).collect()
        };

        if target_tools.is_empty() {
            miseprintln!("{} No tools found to process", style("!").yellow());
            return Ok(());
        }

        if self.dry_run {
            miseprintln!(
                "{} Dry run - showing what would be processed:",
                style("INFO").blue()
            );
            for tool in &target_tools {
                miseprintln!("  {} {}", style("→").green(), tool.ba().short);
            }
            return Ok(());
        }

        // Generate lockfile using the high-level API
        let lockfile = Lockfile::generate_for_tools(
            lockfile_path,
            &target_tools,
            &target_platforms,
            self.force,
        )
        .await?;

        // Write the lockfile
        lockfile.write(lockfile_path)?;

        miseprintln!(
            "{} Lockfile updated at {}",
            style("✓").green(),
            style(display_path(lockfile_path)).cyan()
        );

        Ok(())
    }

    fn extract_platforms(&self, lockfile: &Lockfile) -> BTreeSet<String> {
        let mut platforms = BTreeSet::new();

        for tools in lockfile.tools().values() {
            for tool in tools {
                for platform_key in tool.platforms.keys() {
                    platforms.insert(platform_key.clone());
                }
            }
        }

        platforms
    }
}

// Note: We'll need to make Lockfile::read public in src/lockfile.rs

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  
  $ <bold>mise lock</bold>                           Update lockfile in current directory for all platforms
  $ <bold>mise lock node python</bold>              Update only node and python 
  $ <bold>mise lock --platform linux-x64</bold>     Update only linux-x64 platform
  $ <bold>mise lock --dry-run</bold>                Show what would be updated or created
  $ <bold>mise lock --force</bold>                  Re-download and update even if data exists
"#
);
