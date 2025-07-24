use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::file::display_path;
use crate::lockfile::Lockfile;
use console::style;
use eyre::Result;

/// Update lockfile checksums and URLs for all specified platforms
///
/// Updates checksums and download URLs for all platforms already specified in the lockfile.
/// This allows you to refresh lockfile data for platforms other than the one you're currently on.
/// By default, updates all tools in all lockfiles. Use TOOL arguments to target specific tools.
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
        let config = Config::get().await?;

        // Phase 2: Add actual implementation
        if self.dry_run {
            self.analyze_lockfiles(&config).await?;
        } else {
            self.update_lockfiles(&config).await?;
        }

        Ok(())
    }

    async fn update_lockfiles(&self, config: &Config) -> Result<()> {
        use crate::lockfile::update_lockfiles_for_platforms;

        let tool_filters: Vec<String> = self.tool.iter().map(|t| t.ba.short.clone()).collect();

        miseprintln!("{} Updating lockfiles...", style("→").green());

        update_lockfiles_for_platforms(config, &tool_filters, &self.platform).await?;

        miseprintln!("{} Lockfiles updated successfully", style("✓").green());
        Ok(())
    }

    async fn analyze_lockfiles(&self, config: &Config) -> Result<()> {
        let lockfiles = self.discover_lockfiles(config)?;

        if lockfiles.is_empty() {
            miseprintln!("No lockfiles found");
            return Ok(());
        }

        miseprintln!("Found {} lockfile(s):", lockfiles.len());

        for lockfile_path in &lockfiles {
            miseprintln!("  {}", style(display_path(lockfile_path)).cyan());

            // Read and analyze each lockfile
            let lockfile = Lockfile::read(lockfile_path)?;
            let platforms = self.extract_platforms(&lockfile);
            let tools = self.extract_tools(&lockfile);

            if tools.is_empty() {
                miseprintln!("    {} No tools found", style("!").yellow());
                continue;
            }

            miseprintln!("    Tools: {}", tools.join(", "));

            if platforms.is_empty() {
                miseprintln!("    {} No platform data found", style("!").yellow());
            } else {
                miseprintln!(
                    "    Platforms: {}",
                    platforms.iter().cloned().collect::<Vec<_>>().join(", ")
                );
            }

            // Show what would be updated based on filters
            let target_tools = self.get_target_tools(&tools);
            let target_platforms = self.get_target_platforms(&platforms);

            if !target_tools.is_empty() && !target_platforms.is_empty() {
                miseprintln!(
                    "    {} Would update {} tool(s) for {} platform(s)",
                    style("→").green(),
                    target_tools.len(),
                    target_platforms.len()
                );

                if self.dry_run {
                    for tool in &target_tools {
                        for platform in &target_platforms {
                            miseprintln!(
                                "      {} {} for {}",
                                style("✓").green(),
                                style(tool).bold(),
                                style(platform).blue()
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn discover_lockfiles(&self, config: &Config) -> Result<Vec<PathBuf>> {
        let mut lockfiles = Vec::new();

        // Find all mise.toml files and check for corresponding .lock files
        for (config_path, config_file) in &config.config_files {
            if config_file.source().is_mise_toml() {
                let lockfile_path = config_path.with_extension("lock");
                if lockfile_path.exists() {
                    lockfiles.push(lockfile_path);
                }
            }
        }

        Ok(lockfiles)
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

    fn extract_tools(&self, lockfile: &Lockfile) -> Vec<String> {
        lockfile.tools().keys().cloned().collect()
    }

    fn get_target_tools(&self, available_tools: &[String]) -> Vec<String> {
        if self.tool.is_empty() {
            // If no tools specified, target all tools
            available_tools.to_vec()
        } else {
            // Filter to only specified tools that exist in lockfile
            let specified_tools: BTreeSet<String> =
                self.tool.iter().map(|t| t.ba.short.clone()).collect();

            available_tools
                .iter()
                .filter(|tool| specified_tools.contains(*tool))
                .cloned()
                .collect()
        }
    }

    fn get_target_platforms(&self, available_platforms: &BTreeSet<String>) -> Vec<String> {
        if self.platform.is_empty() {
            // If no platforms specified, target all platforms
            available_platforms.iter().cloned().collect()
        } else {
            // Filter to only specified platforms that exist in lockfile
            let specified_platforms: BTreeSet<String> = self.platform.iter().cloned().collect();

            available_platforms
                .iter()
                .filter(|platform| specified_platforms.contains(*platform))
                .cloned()
                .collect()
        }
    }
}

// Note: We'll need to make Lockfile::read public in src/lockfile.rs

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  
  $ <bold>mise lock</bold>                           Update all tools in all lockfiles for all platforms
  $ <bold>mise lock node python</bold>              Update only node and python 
  $ <bold>mise lock --platform linux-x64</bold>     Update only linux-x64 platform
  $ <bold>mise lock --dry-run</bold>                Show what would be updated
  $ <bold>mise lock --force</bold>                  Re-download and update even if data exists
"#
);
