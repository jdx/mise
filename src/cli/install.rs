use std::collections::HashSet;
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result};
use console::style;
use itertools::Itertools;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::config::MissingRuntimeBehavior::AutoInstall;
use crate::errors::Error::PluginNotInstalled;
use crate::output::Output;
use crate::plugins::PluginName;
use crate::runtime_symlinks::rebuild_symlinks;
use crate::shims::reshim;
use crate::tool::Tool;
use crate::toolset::{
    ToolVersion, ToolVersionOptions, ToolVersionRequest, Toolset, ToolsetBuilder,
};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::ProgressReport;

/// Install a tool version
///
/// This will install a tool version to `~/.local/share/rtx/installs/<PLUGIN>/<VERSION>`
/// It won't be used simply by being installed, however.
/// For that, you must set up a `.tool-version` file manually or with `rtx local/global`.
/// Or you can call a tool version explicitly with `rtx exec <TOOL>@<VERSION> -- <COMMAND>`.
///
/// Runtimes will be installed in parallel. To disable, set `--jobs=1` or `RTX_JOBS=1`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "i", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Install {
    /// Tool version(s) to install
    /// e.g.: node@20
    #[clap(value_parser = RuntimeArgParser)]
    runtime: Option<Vec<RuntimeArg>>,

    /// Only install tool version(s) for <PLUGIN>
    #[clap(long, short, conflicts_with = "runtime")]
    plugin: Option<Vec<PluginName>>,

    /// Force reinstall even if already installed
    #[clap(long, short, requires = "runtime")]
    force: bool,

    /// Install all missing tool versions as well as all plugins for the current directory
    /// This is hidden because it's now the default behavior
    #[clap(long, short, conflicts_with_all = ["runtime", "plugin", "force"], hide = true)]
    all: bool,

    /// Show installation output
    #[clap(long, short, action = clap::ArgAction::Count)]
    verbose: u8,
}

impl Command for Install {
    fn run(self, mut config: Config, _out: &mut Output) -> Result<()> {
        config.settings.missing_runtime_behavior = AutoInstall;

        match &self.runtime {
            Some(runtime) => self.install_runtimes(config, runtime)?,
            None => self.install_missing_runtimes(config)?,
        }

        Ok(())
    }
}

impl Install {
    fn install_runtimes(&self, mut config: Config, runtimes: &[RuntimeArg]) -> Result<()> {
        let mpr = MultiProgressReport::new(config.settings.verbose);
        let ts = ToolsetBuilder::new()
            .with_latest_versions()
            .build(&mut config)?;
        ThreadPoolBuilder::new()
            .num_threads(config.settings.jobs)
            .build()?
            .install(|| -> Result<()> {
                let tool_versions =
                    self.get_requested_tool_versions(&mut config, &ts, runtimes, &mpr)?;
                if tool_versions.is_empty() {
                    warn!("no runtimes to install");
                    warn!("specify a version with `rtx install <PLUGIN>@<VERSION>`");
                    return Ok(());
                }
                self.uninstall_existing_versions(&config, &mpr, &tool_versions)?;
                self.install_requested_versions(&config, &mpr, tool_versions)?;
                reshim(&mut config, &ts).map_err(|err| eyre!("failed to reshim: {}", err))?;
                rebuild_symlinks(&config)?;
                Ok(())
            })
    }

    fn get_requested_tool_versions(
        &self,
        config: &mut Config,
        ts: &Toolset,
        runtimes: &[RuntimeArg],
        mpr: &MultiProgressReport,
    ) -> Result<Vec<(Arc<Tool>, ToolVersion)>> {
        let mut requests = vec![];
        for runtime in RuntimeArg::double_runtime_condition(runtimes) {
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
            let plugin = config.get_or_create_tool(&plugin_name);
            if !plugin.is_installed() {
                let mut pr = mpr.add();
                if let Err(err) = plugin.install(config, &mut pr, false) {
                    pr.error();
                    return Err(err)?;
                }
            }
            let tv = tvr.resolve(config, &plugin, opts, ts.latest_versions)?;
            tool_versions.push((plugin, tv));
        }
        Ok(tool_versions)
    }

    fn install_missing_runtimes(&self, mut config: Config) -> Result<()> {
        let mut ts = ToolsetBuilder::new()
            .with_latest_versions()
            .build(&mut config)?;
        if let Some(plugins) = &self.plugin {
            let plugins = plugins.iter().collect::<HashSet<&PluginName>>();
            for plugin in ts.versions.keys().cloned().collect::<Vec<_>>() {
                if !plugins.contains(&plugin) {
                    ts.versions.remove(&plugin);
                }
            }
            for plugin in plugins {
                if !ts.versions.contains_key(plugin) {
                    Err(PluginNotInstalled(plugin.to_string()))?;
                }
            }
        }
        if ts.list_missing_versions(&config).is_empty() {
            warn!("no runtimes to install");
        }
        let mpr = MultiProgressReport::new(config.settings.verbose);
        ts.install_missing(&mut config, mpr)?;

        Ok(())
    }

    fn uninstall_existing_versions(
        &self,
        config: &Config,
        mpr: &MultiProgressReport,
        tool_versions: &[(Arc<Tool>, ToolVersion)],
    ) -> Result<()> {
        let already_installed_tool_versions = tool_versions
            .iter()
            .filter(|(t, tv)| t.is_version_installed(tv))
            .map(|(t, tv)| (t, tv.clone()));
        if self.force {
            already_installed_tool_versions
                .par_bridge()
                .map(|(tool, tv)| self.uninstall_version(config, tool, &tv, mpr.add()))
                .collect::<Result<Vec<_>>>()?;
        } else {
            for (_, tv) in already_installed_tool_versions {
                warn!("{} already installed", style(tv).cyan().for_stderr());
            }
        }
        Ok(())
    }
    fn install_requested_versions(
        &self,
        config: &Config,
        mpr: &MultiProgressReport,
        tool_versions: Vec<(Arc<Tool>, ToolVersion)>,
    ) -> Result<()> {
        let grouped_tool_versions: Vec<(Arc<Tool>, Vec<ToolVersion>)> = tool_versions
            .into_iter()
            .filter(|(t, tv)| !t.is_version_installed(tv))
            .group_by(|(t, _)| t.clone())
            .into_iter()
            .map(|(t, tvs)| (t, tvs.map(|(_, tv)| tv).collect()))
            .collect();
        grouped_tool_versions
            .into_par_iter()
            .map(|(tool, versions)| {
                for tv in versions {
                    self.install_version(config, &tool, &tv, mpr.add())?;
                }
                Ok(())
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(())
    }
    fn uninstall_version(
        &self,
        config: &Config,
        tool: &Tool,
        tv: &ToolVersion,
        mut pr: ProgressReport,
    ) -> Result<()> {
        tool.decorate_progress_bar(&mut pr, Some(tv));
        match tool.uninstall_version(config, tv, &pr, false) {
            Ok(_) => {
                pr.finish();
                Ok(())
            }
            Err(err) => {
                pr.error();
                Err(err.wrap_err(format!("failed to uninstall {}", tv)))
            }
        }
    }
    fn install_version(
        &self,
        config: &Config,
        tool: &Tool,
        tv: &ToolVersion,
        mut pr: ProgressReport,
    ) -> Result<()> {
        tool.decorate_progress_bar(&mut pr, Some(tv));
        match tool.install_version(config, tv, &mut pr, self.force) {
            Ok(_) => Ok(()),
            Err(err) => {
                pr.error();
                Err(err.wrap_err(format!("failed to install {}", tv)))
            }
        }
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
