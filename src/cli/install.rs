use eyre::Result;
use itertools::Itertools;
use std::collections::HashSet;

use crate::cli::args::{BackendArg, ToolArg};
use crate::config::Config;
use crate::toolset::{InstallOptions, ToolRequest, ToolVersion, ToolVersionOptions, Toolset};
use crate::ui::multi_progress_report::MultiProgressReport;

/// Install a tool version
///
/// Installs a tool version to `~/.local/share/mise/installs/<PLUGIN>/<VERSION>`
/// Installing alone will not activate the tools so they won't be in PATH.
/// To install and/or activate in one command, use `mise use` which will create a `.mise.toml` file
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
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        match &self.tool {
            Some(runtime) => self.install_runtimes(&config, runtime)?,
            None => self.install_missing_runtimes(&config)?,
        };

        Ok(())
    }
    fn install_runtimes(&self, config: &Config, runtimes: &[ToolArg]) -> Result<Vec<ToolVersion>> {
        let mpr = MultiProgressReport::get();
        let tools: HashSet<BackendArg> = runtimes.iter().map(|ta| ta.backend.clone()).collect();
        let mut ts = config.get_tool_request_set()?.filter_by_tool(&tools).into();
        let tool_versions = self.get_requested_tool_versions(&ts, runtimes)?;
        if tool_versions.is_empty() {
            warn!("no runtimes to install");
            warn!("specify a version with `mise install <PLUGIN>@<VERSION>`");
            return Ok(vec![]);
        }
        ts.install_versions(config, tool_versions, &mpr, &self.install_opts())
    }

    fn install_opts(&self) -> InstallOptions {
        InstallOptions {
            force: self.force,
            jobs: self.jobs,
            raw: self.raw,
            latest_versions: true,
        }
    }

    fn get_requested_tool_versions(
        &self,
        ts: &Toolset,
        runtimes: &[ToolArg],
    ) -> Result<Vec<ToolRequest>> {
        let mut requests = vec![];
        for ta in ToolArg::double_tool_condition(runtimes)? {
            let default_opts = ToolVersionOptions::new();
            match ta.tvr {
                // user provided an explicit version
                // TODO: this should install using options from config if the version matches
                Some(tv) => requests.push(tv),
                None => {
                    if ta.tvr.is_none() {
                        match ts.versions.get(&ta.backend) {
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
                                    backend: ta.backend.clone(),
                                    version: "latest".into(),
                                    options: default_opts.clone(),
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

    fn install_missing_runtimes(&self, config: &Config) -> eyre::Result<Vec<ToolVersion>> {
        let trs = config.get_tool_request_set()?;
        let versions = trs.missing_tools().into_iter().cloned().collect_vec();
        if versions.is_empty() {
            info!("all runtimes are installed");
            return Ok(vec![]);
        }
        let mpr = MultiProgressReport::get();
        let mut ts = Toolset::from(trs.clone());
        ts.install_versions(config, versions, &mpr, &self.install_opts())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise install node@20.0.0</bold>  # install specific node version
    $ <bold>mise install node@20</bold>      # install fuzzy node version
    $ <bold>mise install node</bold>         # install version specified in .tool-versions or .mise.toml
    $ <bold>mise install</bold>              # installs everything specified in .tool-versions or .mise.toml
"#
);

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::dirs;
    use crate::test::reset;

    #[test]
    fn test_install_force() {
        reset();
        assert_cli!("install", "-f", "tiny");
    }

    #[test]
    fn test_install_asdf_style() {
        reset();
        assert_cli!("install", "tiny", "2");
    }

    #[test]
    fn test_install_with_alias() {
        reset();
        assert_cli!("install", "-f", "tiny@my/alias");
        assert_cli_snapshot!("where", "tiny@my/alias");
    }

    #[test]
    fn test_install_ref() {
        reset();
        assert_cli!("install", "-f", "dummy@ref:master");
        assert_cli!("global", "dummy@ref:master");
        let output = assert_cli!("where", "dummy");
        assert_str_eq!(
            output.trim(),
            dirs::INSTALLS.join("dummy/ref-master").to_string_lossy()
        );
        assert_cli!("global", "--unset", "dummy");
    }

    #[test]
    fn test_install_nothing() {
        reset();
        // this doesn't do anything since dummy isn't specified
        assert_cli_snapshot!("install", "dummy");
    }
}
