use color_eyre::eyre::Result;

use crate::cli::args::tool::{ToolArg, ToolArgParser};
use crate::config::Config;

use crate::toolset::{
    InstallOptions, ToolVersion, ToolVersionOptions, ToolVersionRequest, Toolset, ToolsetBuilder,
};
use crate::ui::multi_progress_report::MultiProgressReport;

/// Install a tool version
///
/// This will install a tool version to `~/.local/share/rtx/installs/<PLUGIN>/<VERSION>`
/// It won't be used simply by being installed, however.
/// For that, you must set up a `.rtx.toml`/`.tool-version` file manually or with `rtx use`.
/// Or you can call a tool version explicitly with `rtx exec <TOOL>@<VERSION> -- <COMMAND>`.
///
/// Tools will be installed in parallel. To disable, set `--jobs=1` or `RTX_JOBS=1`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "i", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Install {
    /// Tool(s) to install
    /// e.g.: node@20
    #[clap(value_name = "TOOL@VERSION", value_parser = ToolArgParser)]
    tool: Option<Vec<ToolArg>>,

    /// Force reinstall even if already installed
    #[clap(long, short, requires = "tool")]
    force: bool,

    /// Number of jobs to run in parallel
    /// [default: 4]
    #[clap(long, short, env = "RTX_JOBS", verbatim_doc_comment)]
    jobs: Option<usize>,

    /// Directly pipe stdin/stdout/stderr from plugin to user
    /// Sets --jobs=1
    #[clap(long, overrides_with = "jobs")]
    raw: bool,

    /// Show installation output
    #[clap(long, short, action = clap::ArgAction::Count)]
    verbose: u8,
}

impl Install {
    pub fn run(self, config: Config) -> Result<()> {
        match &self.tool {
            Some(runtime) => self.install_runtimes(config, runtime)?,
            None => self.install_missing_runtimes(config)?,
        }

        Ok(())
    }
    fn install_runtimes(&self, config: Config, runtimes: &[ToolArg]) -> Result<()> {
        let mpr = MultiProgressReport::new(&config.settings);
        let mut ts = ToolsetBuilder::new()
            .with_latest_versions()
            .build(&config)?;
        let tool_versions = self.get_requested_tool_versions(&config, &ts, runtimes, &mpr)?;
        if tool_versions.is_empty() {
            warn!("no runtimes to install");
            warn!("specify a version with `rtx install <PLUGIN>@<VERSION>`");
            return Ok(());
        }
        ts.install_versions(&config, tool_versions, &mpr, &self.install_opts())
    }

    fn install_opts(&self) -> InstallOptions {
        InstallOptions {
            force: self.force,
            jobs: self.jobs,
            raw: self.raw,
        }
    }

    fn get_requested_tool_versions(
        &self,
        config: &Config,
        ts: &Toolset,
        runtimes: &[ToolArg],
        mpr: &MultiProgressReport,
    ) -> Result<Vec<ToolVersion>> {
        let mut requests = vec![];
        for runtime in ToolArg::double_tool_condition(runtimes) {
            let default_opts = ToolVersionOptions::new();
            match runtime.tvr {
                Some(tv) => requests.push((runtime.plugin, tv, default_opts.clone())),
                None => {
                    if runtime.tvr.is_none() {
                        match ts.versions.get(&runtime.plugin) {
                            Some(tvl) => {
                                for (tvr, opts) in &tvl.requests {
                                    requests.push((
                                        runtime.plugin.clone(),
                                        tvr.clone(),
                                        opts.clone(),
                                    ));
                                }
                            }
                            None => {
                                let tvr = ToolVersionRequest::Version(
                                    runtime.plugin.clone(),
                                    "latest".into(),
                                );
                                requests.push((runtime.plugin, tvr, default_opts.clone()));
                            }
                        }
                    }
                }
            }
        }
        let mut tool_versions = vec![];
        for (plugin_name, tvr, opts) in requests {
            let plugin = config.get_or_create_plugin(&plugin_name);
            plugin.ensure_installed(config, Some(mpr), false)?;
            let tv = tvr.resolve(config, plugin, opts, ts.latest_versions)?;
            tool_versions.push(tv);
        }
        Ok(tool_versions)
    }

    fn install_missing_runtimes(&self, config: Config) -> Result<()> {
        let mut ts = ToolsetBuilder::new()
            .with_latest_versions()
            .build(&config)?;
        let versions = ts
            .list_missing_versions(&config)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        if versions.is_empty() {
            info!("all runtimes are installed");
            return Ok(());
        }
        let mpr = MultiProgressReport::new(&config.settings);
        ts.install_versions(&config, versions, &mpr, &self.install_opts())?;
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx install node@20.0.0</bold>  # install specific node version
  $ <bold>rtx install node@20</bold>      # install fuzzy node version
  $ <bold>rtx install node</bold>         # install version specified in .tool-versions or .rtx.toml
  $ <bold>rtx install</bold>                # installs everything specified in .tool-versions or .rtx.toml
"#
);

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::{assert_cli, assert_cli_snapshot, dirs};

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
