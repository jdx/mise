use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::config::Settings;
use crate::duration::parse_into_timestamp;
use crate::hooks::Hooks;
use crate::toolset::{InstallOptions, ResolveOptions, ToolRequest, ToolSource, Toolset};
use crate::{config, env, exit, hooks};
use eyre::Result;
use itertools::Itertools;
use jiff::Timestamp;

/// Install a tool version
///
/// Installs a tool version to `~/.local/share/mise/installs/<PLUGIN>/<VERSION>`
/// Installing alone will not activate the tools so they won't be in PATH.
/// To install and/or activate in one command, use `mise use` which will create a `mise.toml` file
/// in the current directory to activate this tool when inside the directory.
/// Alternatively, run `mise exec <TOOL>@<VERSION> -- <COMMAND>` to execute a tool without creating config files.
///
/// Tools will be installed in parallel. To disable, set `--jobs=1` or `MISE_JOBS=1`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "i", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Install {
    /// Tool(s) to install
    /// e.g.: node@20
    #[clap(value_name = "TOOL@VERSION")]
    tool: Option<Vec<ToolArg>>,

    /// Force reinstall even if already installed
    #[clap(long, short, requires = "tool")]
    force: bool,

    /// Number of jobs to run in parallel
    /// [default: 4]
    #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    jobs: Option<usize>,

    /// Show what would be installed without actually installing
    #[clap(long, short = 'n', verbatim_doc_comment)]
    dry_run: bool,

    /// Show installation output
    ///
    /// This argument will print plugin output such as download, configuration, and compilation output.
    #[clap(long, short, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Only install versions released before this date
    ///
    /// Supports absolute dates like "2024-06-01" and relative durations like "90d" or "1y".
    #[clap(long, verbatim_doc_comment)]
    before: Option<String>,

    /// Like --dry-run but exits with code 1 if there are tools to install
    ///
    /// This is useful for scripts to check if tools need to be installed.
    #[clap(long, verbatim_doc_comment)]
    dry_run_code: bool,

    /// Directly pipe stdin/stdout/stderr from plugin to user
    /// Sets --jobs=1
    #[clap(long, overrides_with = "jobs")]
    raw: bool,
}

impl Install {
    fn is_dry_run(&self) -> bool {
        self.dry_run || self.dry_run_code
    }

    #[async_backtrace::framed]
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        match &self.tool {
            Some(runtime) => {
                let original_tool_args = env::TOOL_ARGS.read().unwrap().clone();
                env::TOOL_ARGS.write().unwrap().clone_from(runtime);
                self.install_runtimes(config, runtime, original_tool_args)
                    .await?
            }
            None => self.install_missing_runtimes(config).await?,
        };
        Ok(())
    }

    #[async_backtrace::framed]
    async fn install_runtimes(
        &self,
        mut config: Arc<Config>,
        runtimes: &[ToolArg],
        original_tool_args: Vec<ToolArg>,
    ) -> Result<()> {
        let trs = config.get_tool_request_set().await?;

        // Expand wildcards (e.g., "pipx:*") to actual ToolArgs from config
        let mut has_unmatched_wildcard = false;
        let expanded_runtimes: Vec<ToolArg> = runtimes
            .iter()
            .flat_map(|ta| {
                if let Some(backend_prefix) = ta.ba.short.strip_suffix(":*") {
                    // Find all tools in config with this backend prefix
                    let matching: Vec<_> = trs
                        .tools
                        .keys()
                        .filter(|ba| {
                            ba.short.starts_with(&format!("{backend_prefix}:"))
                                && ba.tool_name != "*"
                        })
                        .filter_map(|ba| ToolArg::from_str(&ba.short).ok())
                        .collect();
                    if matching.is_empty() {
                        warn!("no tools found in config matching {}", ta.ba.short);
                        has_unmatched_wildcard = true;
                    }
                    return matching;
                }
                vec![ta.clone()]
            })
            .collect();

        // If only wildcards were provided and none matched, exit early
        if expanded_runtimes.is_empty() && has_unmatched_wildcard {
            return Ok(());
        }

        let tools: HashSet<String> = expanded_runtimes
            .iter()
            .map(|ta| ta.ba.short.clone())
            .collect();
        // Collect inactive tool names before trs borrow is consumed
        let inactive_tools: Vec<String> = expanded_runtimes
            .iter()
            .filter(|ta| {
                trs.sources
                    .get(ta.ba.as_ref())
                    .is_none_or(|s| s.is_argument())
            })
            .map(|ta| ta.ba.short.clone())
            .collect();
        let mut ts: Toolset = trs.filter_by_tool(tools).into();
        let tool_versions = self.get_requested_tool_versions(&ts, &expanded_runtimes)?;
        let mut versions = if tool_versions.is_empty() {
            warn!("no runtimes to install");
            warn!("specify a version with `mise install <PLUGIN>@<VERSION>`");
            vec![]
        } else {
            ts.install_all_versions(&mut config, tool_versions, &self.install_opts()?)
                .await?
        };
        // In dry-run mode, check if any tools would be installed before filtering
        if self.is_dry_run() {
            if self.dry_run_code {
                let has_work = versions.iter().any(|tv| {
                    if let Ok(backend) = tv.backend() {
                        !backend.is_version_installed(&config, tv, true)
                    } else {
                        true
                    }
                });
                if has_work {
                    exit::exit(1);
                }
            }
            return Ok(());
        }

        // because we may be installing a tool that is not in config, we need to restore the original tool args and reset everything
        env::TOOL_ARGS
            .write()
            .unwrap()
            .clone_from(&original_tool_args);
        let config = Config::reset().await?;
        let ts = config.get_toolset().await?;
        let current_versions = ts.list_current_versions();
        // ensure that only current versions are sent to lockfile rebuild
        versions.retain(|tv| current_versions.iter().any(|(_, cv)| tv == cv));

        config::rebuild_shims_and_runtime_symlinks(&config, ts, &versions).await?;

        // Warn about tools that were installed but not in any config file
        if !inactive_tools.is_empty() {
            let tool_list = inactive_tools.join(", ");
            let use_cmds: Vec<String> = inactive_tools
                .iter()
                .map(|t| format!("  mise use {t}"))
                .collect();
            warn!(
                "{tool_list} installed but not activated — {} not in any config file.\nTo install and activate, run:\n{}",
                if inactive_tools.len() == 1 {
                    "it is"
                } else {
                    "they are"
                },
                use_cmds.join("\n"),
            );
        }

        Ok(())
    }

    fn install_opts(&self) -> Result<InstallOptions> {
        Ok(InstallOptions {
            force: self.force,
            jobs: self.jobs,
            raw: self.raw,
            missing_args_only: false,
            resolve_options: ResolveOptions {
                use_locked_version: true,
                latest_versions: true,
                before_date: self.get_before_date()?,
            },
            dry_run: self.is_dry_run(),
            locked: Settings::get().locked,
            ..Default::default()
        })
    }

    /// Get the before_date from CLI flag or settings
    fn get_before_date(&self) -> Result<Option<Timestamp>> {
        if let Some(before) = &self.before {
            return Ok(Some(parse_into_timestamp(before)?));
        }
        if let Some(before) = &Settings::get().install_before {
            return Ok(Some(parse_into_timestamp(before)?));
        }
        Ok(None)
    }

    fn get_requested_tool_versions(
        &self,
        ts: &Toolset,
        runtimes: &[ToolArg],
    ) -> Result<Vec<ToolRequest>> {
        let mut requests = vec![];
        for ta in ToolArg::double_tool_condition(runtimes)? {
            match ta.tvr {
                // user provided an explicit version
                Some(tv) => requests.push(tv),
                None => {
                    match ts.versions.get(ta.ba.as_ref()) {
                        // the tool is in config so fetch the params from config
                        // this may match multiple versions of one tool (e.g.: python)
                        Some(tvl) => {
                            for tvr in &tvl.requests {
                                requests.push(tvr.clone());
                            }
                        }
                        // in this case the user specified a tool which is not in config
                        // so we default to @latest with no options
                        None => {
                            let tvr = ToolRequest::Version {
                                backend: ta.ba.clone(),
                                version: "latest".into(),
                                options: ta.ba.opts(),
                                source: ToolSource::Argument,
                            };
                            requests.push(tvr);
                        }
                    }
                }
            }
        }
        Ok(requests)
    }

    async fn install_missing_runtimes(&self, mut config: Arc<Config>) -> eyre::Result<()> {
        let trs = measure!("get_tool_request_set", {
            config.get_tool_request_set().await?
        });

        // Install plugins from [plugins] config section first
        // This must happen before checking for missing tools so env-only plugins get installed
        Toolset::ensure_config_plugins_installed(&config, self.is_dry_run()).await?;

        // Check for tools that don't exist in the registry
        // These were tracked during build() before being filtered out
        for ba in &trs.unknown_tools {
            // This will error with a proper message like "tool not found in mise tool registry"
            ba.backend()?;
        }
        let missing = measure!("fetching missing runtimes", {
            trs.missing_tools(&config)
                .await
                .into_iter()
                .cloned()
                .collect_vec()
        });
        let has_missing = !missing.is_empty();
        let versions = if missing.is_empty() {
            measure!("run_postinstall_hook", {
                info!("all tools are installed");
                hooks::run_one_hook(
                    &config,
                    config.get_toolset().await?,
                    Hooks::Postinstall,
                    None,
                )
                .await;
                vec![]
            })
        } else {
            let mut ts = Toolset::from(trs.clone());
            measure!("install_all_versions", {
                ts.install_all_versions(&mut config, missing, &self.install_opts()?)
                    .await?
            })
        };
        if self.is_dry_run() {
            if self.dry_run_code && has_missing {
                exit::exit(1);
            }
            return Ok(());
        }
        measure!("rebuild_shims_and_runtime_symlinks", {
            let ts = config.get_toolset().await?;
            config::rebuild_shims_and_runtime_symlinks(&config, ts, &versions).await?;
        });
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise install node@20.0.0</bold>  # install specific node version
    $ <bold>mise install node@20</bold>      # install fuzzy node version
    $ <bold>mise install node</bold>         # install version specified in mise.toml
    $ <bold>mise install</bold>              # installs everything specified in mise.toml
"#
);
