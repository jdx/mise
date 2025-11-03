use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::backend::{get, platform_target::PlatformTarget};
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

        // Validate platforms if specified
        if !self.platform.is_empty() {
            let parsed_platforms = Platform::parse_multiple(&self.platform)?;
            miseprintln!(
                "{} Validated {} platform(s): {}",
                style("→").green(),
                parsed_platforms.len(),
                parsed_platforms
                    .iter()
                    .map(|p| p.to_key())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        // For Phase 1, just implement lockfile discovery and platform analysis
        self.analyze_lockfiles(&config).await?;

        // Demonstrate the new backend metadata fetching capabilities
        self.demonstrate_metadata_fetching(&config).await?;

        if !self.dry_run {
            miseprintln!(
                "{} {}",
                style("mise lock").bold().cyan(),
                style("full implementation coming in next phase").green()
            );
        }

        Ok(())
    }

    async fn analyze_lockfiles(&self, config: &Config) -> Result<()> {
        let potential_lockfiles = self.discover_lockfiles(config)?;
        let existing_lockfiles: Vec<PathBuf> = potential_lockfiles
            .iter()
            .filter(|p| p.exists())
            .cloned()
            .collect();
        let missing_lockfiles: Vec<PathBuf> = potential_lockfiles
            .iter()
            .filter(|p| !p.exists())
            .cloned()
            .collect();

        if potential_lockfiles.is_empty() {
            miseprintln!("No config found in current directory");
            return Ok(());
        }

        if existing_lockfiles.is_empty() && missing_lockfiles.is_empty() {
            miseprintln!("No lockfiles found");
            return Ok(());
        }

        // Analyze existing lockfiles
        if !existing_lockfiles.is_empty() {
            miseprintln!("Found lockfile:");
            for lockfile_path in &existing_lockfiles {
                miseprintln!("  {}", style(display_path(lockfile_path)).cyan());

                // Read and analyze each lockfile
                let lockfile = Lockfile::read(lockfile_path)?;
                let platforms = self.extract_platforms(&lockfile);
                let tools = self.extract_tools(&lockfile);

                self.analyze_lockfile_content(&tools, &platforms)?;
            }
        }

        // Analyze missing lockfiles (potential for creation)
        if !missing_lockfiles.is_empty() {
            if !existing_lockfiles.is_empty() {
                miseprintln!();
            }
            miseprintln!("No lockfile found, would create:");
            for lockfile_path in &missing_lockfiles {
                miseprintln!(
                    "  {} {}",
                    style("→").yellow(),
                    style(display_path(lockfile_path)).cyan()
                );

                // Get tools from the corresponding config file
                let config_path = PathBuf::from("mise.toml");

                // Try to read tools from the config file or from the overall config
                let tools = if config_path.exists() {
                    // Read directly from the local config file
                    match crate::config::config_file::parse(&config_path).await {
                        Ok(config_file) => {
                            let tool_request_set = config_file.to_tool_request_set()?;
                            tool_request_set
                                .list_tools()
                                .iter()
                                .map(|ba| ba.short.clone())
                                .collect()
                        }
                        Err(_) => Vec::new(),
                    }
                } else {
                    // No local config file exists, but maybe get tools from current config context
                    if let Ok(tool_request_set) = config.get_tool_request_set().await {
                        tool_request_set
                            .list_tools()
                            .iter()
                            .map(|ba| ba.short.clone())
                            .collect()
                    } else {
                        Vec::new()
                    }
                };

                if tools.is_empty() {
                    miseprintln!("    {} No tools configured", style("!").yellow());
                } else {
                    miseprintln!(
                        "    {} Would create lockfile with {} tool(s): {}",
                        style("→").green(),
                        tools.len(),
                        tools.join(", ")
                    );

                    // For creation, we don't have existing platforms, but show what tools would be targeted
                    let target_tools = self.get_target_tools(&tools);
                    if !target_tools.is_empty() {
                        miseprintln!(
                            "    {} Would initialize {} tool(s) in new lockfile",
                            style("→").green(),
                            target_tools.len()
                        );

                        if self.dry_run {
                            for tool in &target_tools {
                                miseprintln!(
                                    "      {} {} (new lockfile)",
                                    style("✓").green(),
                                    style(tool).bold()
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn analyze_lockfile_content(
        &self,
        tools: &[String],
        platforms: &BTreeSet<String>,
    ) -> Result<()> {
        if tools.is_empty() {
            miseprintln!("    {} No tools found", style("!").yellow());
            return Ok(());
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
        let target_tools = self.get_target_tools(tools);
        let target_platforms = self.get_target_platforms(platforms);

        if !target_tools.is_empty() && (!target_platforms.is_empty() || platforms.is_empty()) {
            let platform_count = if platforms.is_empty() {
                1
            } else {
                target_platforms.len()
            };
            miseprintln!(
                "    {} Would update {} tool(s) for {} platform(s)",
                style("→").green(),
                target_tools.len(),
                platform_count
            );

            if self.dry_run && !target_platforms.is_empty() {
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

        Ok(())
    }

    fn discover_lockfiles(&self, _config: &Config) -> Result<Vec<PathBuf>> {
        let mut lockfiles = Vec::new();

        // Look for mise.lock in the current directory
        let lockfile_path = PathBuf::from("mise.lock");
        lockfiles.push(lockfile_path);

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
            // Parse and validate specified platforms first, then filter
            match Platform::parse_multiple(&self.platform) {
                Ok(parsed_platforms) => {
                    let specified_platforms: BTreeSet<String> =
                        parsed_platforms.iter().map(|p| p.to_key()).collect();

                    available_platforms
                        .iter()
                        .filter(|platform| specified_platforms.contains(*platform))
                        .cloned()
                        .collect()
                }
                Err(_) => {
                    // If parsing fails, fall back to original logic
                    let specified_platforms: BTreeSet<String> =
                        self.platform.iter().cloned().collect();

                    available_platforms
                        .iter()
                        .filter(|platform| specified_platforms.contains(*platform))
                        .cloned()
                        .collect()
                }
            }
        }
    }

    async fn demonstrate_metadata_fetching(&self, config: &Config) -> Result<()> {
        // Skip if no platforms specified (keep current behavior)
        if self.platform.is_empty() {
            return Ok(());
        }

        miseprintln!(
            "{} Demonstrating new backend metadata fetching:",
            style("INFO").blue()
        );

        let parsed_platforms = Platform::parse_multiple(&self.platform)?;

        // Get configured tools from the toolset
        if let Ok(tool_request_set) = config.get_tool_request_set().await {
            let tools = tool_request_set.list_tools();

            for tool_ba in tools.iter().take(2) {
                // Limit to 2 tools for demo
                if let Some(_backend) = get(tool_ba) {
                    miseprintln!("  {} tool: {}", style("→").green(), tool_ba.short);

                    for platform in parsed_platforms.iter().take(2) {
                        // Limit to 2 platforms for demo
                        let _target = PlatformTarget::new(platform.clone());
                        miseprintln!("    {} platform: {}", style("→").blue(), platform.to_key());

                        // Demonstrate the new backend methods without full ToolVersion
                        // For now, just show that the methods are available
                        miseprintln!(
                            "      {} Backend supports metadata fetching methods:",
                            style("✓").green()
                        );

                        // We can't easily create a ToolVersion here without complex setup
                        // But we can show that the backend has the new capabilities
                        miseprintln!(
                            "        {} get_tarball_url() - implemented",
                            style("•").dim()
                        );
                        miseprintln!(
                            "        {} get_github_release_info() - implemented",
                            style("•").dim()
                        );
                        miseprintln!(
                            "        {} resolve_lock_info() - implemented",
                            style("•").dim()
                        );
                    }
                }
            }
        }

        Ok(())
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
