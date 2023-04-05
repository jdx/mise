use std::collections::HashSet;
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result};
use console::style;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::config::MissingRuntimeBehavior::AutoInstall;
use crate::errors::Error::PluginNotInstalled;
use crate::output::Output;
use crate::plugins::{ExternalPlugin, Plugin, PluginName, Plugins};
use crate::runtime_symlinks::rebuild_symlinks;
use crate::shims::reshim;
use crate::toolset::{ToolVersionOptions, ToolVersionRequest, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;

/// Install a runtime
///
/// This will install a runtime to `~/.local/share/rtx/installs/<PLUGIN>/<VERSION>`
/// It won't be used simply by being installed, however.
/// For that, you must set up a `.tool-version` file manually or with `rtx local/global`.
/// Or you can call a runtime explicitly with `rtx exec <PLUGIN>@<VERSION> -- <COMMAND>`.
///
/// Runtimes will be installed in parallel. To disable, set `--jobs=1` or `RTX_JOBS=1`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "i", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Install {
    /// Runtime(s) to install
    /// e.g.: nodejs@18
    #[clap(value_parser = RuntimeArgParser)]
    runtime: Option<Vec<RuntimeArg>>,

    /// Only install runtime(s) for <PLUGIN>
    #[clap(long, short, conflicts_with = "runtime")]
    plugin: Option<Vec<PluginName>>,

    /// Force reinstall even if already installed
    #[clap(long, short, requires = "runtime")]
    force: bool,

    /// Install all missing runtimes as well as all plugins for the current directory
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
        config.autoupdate();

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
        let mut tool_version_requests = vec![];
        let ts = ToolsetBuilder::new()
            .with_latest_versions()
            .build(&mut config)?;
        for runtime in RuntimeArg::double_runtime_condition(runtimes) {
            let default_opts = ToolVersionOptions::new();
            match runtime.tvr {
                Some(tv) => tool_version_requests.push((runtime.plugin, tv, default_opts.clone())),
                None => {
                    if runtime.tvr.is_none() {
                        match ts.versions.get(&runtime.plugin) {
                            Some(tvl) => {
                                for (tvr, opts) in &tvl.requests {
                                    tool_version_requests.push((
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
                                tool_version_requests.push((
                                    runtime.plugin,
                                    tvr,
                                    default_opts.clone(),
                                ));
                            }
                        }
                    }
                }
            }
        }
        ThreadPoolBuilder::new()
            .num_threads(config.settings.jobs)
            .build()?
            .install(|| -> Result<()> {
                let mut tool_versions = vec![];
                for (plugin_name, tvr, opts) in tool_version_requests {
                    let plugin = match config.plugins.get(&plugin_name).cloned() {
                        Some(plugin) => plugin,
                        None => Arc::new(Plugins::External(ExternalPlugin::new(
                            &config.settings,
                            &plugin_name,
                        ))),
                    };
                    match plugin.as_ref() {
                        Plugins::External(plugin) => {
                            if !plugin.is_installed() {
                                let mut pr = mpr.add();
                                if let Err(err) = plugin.install(&config, &mut pr, false) {
                                    pr.error();
                                    return Err(err)?;
                                }
                            }
                        }
                    }
                    let tv = tvr.resolve(&config, &plugin, opts, ts.latest_versions)?;
                    tool_versions.push((plugin, tv));
                }
                if tool_versions.is_empty() {
                    warn!("no runtimes to install");
                    warn!("specify a version with `rtx install <PLUGIN>@<VERSION>`");
                    return Ok(());
                }
                let mut to_uninstall = vec![];
                for (plugin, tv) in &tool_versions {
                    if plugin.is_version_installed(tv) {
                        if self.force {
                            to_uninstall.push((plugin, tv.clone()));
                        } else {
                            warn!("{} already installed", style(tv).cyan().for_stderr());
                        }
                    }
                }
                if !to_uninstall.is_empty() {
                    to_uninstall
                        .into_par_iter()
                        .map(|(plugin, tv)| {
                            let mut pr = mpr.add();
                            plugin.decorate_progress_bar(&mut pr, Some(&tv));
                            match plugin.uninstall_version(&config, &tv, &pr, false) {
                                Ok(_) => {
                                    pr.finish();
                                    Ok(())
                                }
                                Err(err) => {
                                    pr.error();
                                    Err(err.wrap_err(format!("failed to uninstall {}", tv)))
                                }
                            }
                        })
                        .collect::<Result<Vec<_>>>()?;
                }
                tool_versions
                    .into_par_iter()
                    .map(|(plugin, tv)| {
                        if plugin.is_version_installed(&tv) {
                            return Ok(());
                        }
                        let mut pr = mpr.add();
                        plugin.decorate_progress_bar(&mut pr, Some(&tv));
                        match plugin.install_version(&config, &tv, &mut pr, self.force) {
                            Ok(_) => Ok(()),
                            Err(err) => {
                                pr.error();
                                Err(err.wrap_err(format!("failed to install {}", tv)))
                            }
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                reshim(&mut config, &ts).map_err(|err| eyre!("failed to reshim: {}", err))?;
                rebuild_symlinks(&config)?;
                Ok(())
            })
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
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx install nodejs@18.0.0</bold>  # install specific nodejs version
  $ <bold>rtx install nodejs@18</bold>      # install fuzzy nodejs version
  $ <bold>rtx install nodejs</bold>         # install version specified in .tool-versions or .rtx.toml
  $ <bold>rtx install</bold>                # installs all runtimes specified in .tool-versions or .rtx.toml
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
