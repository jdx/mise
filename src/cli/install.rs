use std::sync::Arc;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::hooks::Hooks;
use crate::toolset::{InstallOptions, ResolveOptions, ToolRequest, ToolSource, Toolset};
use crate::{config, env, hooks};
use eyre::Result;
use itertools::Itertools;

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

    /// Directly pipe stdin/stdout/stderr from plugin to user
    /// Sets --jobs=1
    #[clap(long, overrides_with = "jobs")]
    raw: bool,

    /// Show installation output
    ///
    /// This argument will print plugin output such as download, configuration, and compilation output.
    #[clap(long, short, action = clap::ArgAction::Count)]
    verbose: u8,
}

impl Install {
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
        let tools = runtimes.iter().map(|ta| ta.ba.short.clone()).collect();
        let mut ts = config
            .get_tool_request_set()
            .await?
            .filter_by_tool(tools)
            .into();
        let tool_versions = self.get_requested_tool_versions(&ts, runtimes)?;
        let mut versions = if tool_versions.is_empty() {
            warn!("no runtimes to install");
            warn!("specify a version with `mise install <PLUGIN>@<VERSION>`");
            vec![]
        } else {
            ts.install_all_versions(&mut config, tool_versions, &self.install_opts())
                .await?
        };
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
        Ok(())
    }

    fn install_opts(&self) -> InstallOptions {
        InstallOptions {
            force: self.force,
            jobs: self.jobs,
            raw: self.raw,
            missing_args_only: false,
            resolve_options: ResolveOptions {
                use_locked_version: true,
                latest_versions: true,
            },
            ..Default::default()
        }
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
                    if ta.tvr.is_none() {
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
        }
        Ok(requests)
    }

    async fn install_missing_runtimes(&self, mut config: Arc<Config>) -> eyre::Result<()> {
        let trs = measure!("get_tool_request_set", {
            config.get_tool_request_set().await?
        });
        let versions = measure!("fetching missing runtims", {
            trs.missing_tools(&config)
                .await
                .into_iter()
                .cloned()
                .collect_vec()
        });
        let versions = if versions.is_empty() {
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
                ts.install_all_versions(&mut config, versions, &self.install_opts())
                    .await?
            })
        };
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
