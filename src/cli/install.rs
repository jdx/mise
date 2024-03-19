use eyre::Result;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::forge;
use crate::toolset::{
    InstallOptions, ToolVersion, ToolVersionOptions, ToolVersionRequest, Toolset, ToolsetBuilder,
};
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
        }

        Ok(())
    }
    fn install_runtimes(&self, config: &Config, runtimes: &[ToolArg]) -> Result<()> {
        let mpr = MultiProgressReport::get();
        let mut ts = ToolsetBuilder::new().build(config)?;
        let tool_versions = self.get_requested_tool_versions(&ts, runtimes, &mpr)?;
        if tool_versions.is_empty() {
            warn!("no runtimes to install");
            warn!("specify a version with `mise install <PLUGIN>@<VERSION>`");
            return Ok(());
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
        mpr: &MultiProgressReport,
    ) -> Result<Vec<ToolVersion>> {
        let mut requests = vec![];
        for ta in ToolArg::double_tool_condition(runtimes)? {
            let default_opts = ToolVersionOptions::new();
            match ta.tvr {
                Some(tv) => requests.push((ta.forge, tv, default_opts.clone())),
                None => {
                    if ta.tvr.is_none() {
                        match ts.versions.get(&ta.forge) {
                            Some(tvl) => {
                                for (tvr, opts) in &tvl.requests {
                                    requests.push((ta.forge.clone(), tvr.clone(), opts.clone()));
                                }
                            }
                            None => {
                                let tvr =
                                    ToolVersionRequest::Version(ta.forge.clone(), "latest".into());
                                requests.push((ta.forge, tvr, default_opts.clone()));
                            }
                        }
                    }
                }
            }
        }
        let mut tool_versions = vec![];
        for (fa, tvr, opts) in requests {
            let plugin = forge::get(&fa);
            plugin.ensure_installed(mpr, false)?;
            let tv = tvr.resolve(plugin.as_ref(), opts, true)?;
            tool_versions.push(tv);
        }
        Ok(tool_versions)
    }

    fn install_missing_runtimes(&self, config: &Config) -> Result<()> {
        let mut ts = ToolsetBuilder::new().build(config)?;
        let versions = ts.list_missing_versions();
        if versions.is_empty() {
            info!("all runtimes are installed");
            return Ok(());
        }
        let mpr = MultiProgressReport::get();
        ts.install_versions(config, versions, &mpr, &self.install_opts())?;
        Ok(())
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
    use crate::dirs;

    #[test]
    fn test_install_force() {
        assert_cli!("install", "-f", "tiny");
    }

    #[test]
    fn test_install_asdf_style() {
        assert_cli!("install", "tiny", "2");
    }

    #[test]
    fn test_install_with_alias() {
        assert_cli!("install", "-f", "tiny@my/alias");
        assert_cli_snapshot!("where", "tiny@my/alias");
    }

    #[test]
    fn test_install_ref() {
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
        // this doesn't do anything since dummy isn't specified
        assert_cli_snapshot!("install", "dummy");
    }
}
