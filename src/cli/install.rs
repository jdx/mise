use std::collections::HashSet;

use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::config::MissingRuntimeBehavior::AutoInstall;
use crate::errors::Error::PluginNotInstalled;
use crate::output::Output;
use crate::plugins::PluginName;
use crate::toolset::ToolsetBuilder;

/// Install a runtime
///
/// This will install a runtime to `~/.local/share/rtx/installs/<PLUGIN>/<VERSION>`
/// It won't be used simply by being installed, however.
/// For that, you must set up a `.tool-version` file manually or with `rtx local/global`.
/// Or you can call a runtime explicitly with `rtx exec <PLUGIN>@<VERSION> -- <COMMAND>`.
///
/// Runtimes will be installed in parallel. To disable, set `--jobs=1` or `RTX_JOBS=1`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "i", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Install {
    /// Runtime(s) to install
    ///
    /// e.g.: nodejs@20
    #[clap(value_parser = RuntimeArgParser)]
    runtime: Option<Vec<RuntimeArg>>,

    /// Only install runtime(s) for <PLUGIN>
    #[clap(long, short, conflicts_with = "runtime")]
    plugin: Option<Vec<PluginName>>,

    /// Force reinstall even if already installed
    #[clap(long, short, requires = "runtime")]
    force: bool,

    /// Install all missing runtimes as well as all plugins for the current directory
    ///
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
            Some(runtime) => self.install_runtimes(&config, runtime)?,
            None => self.install_missing_runtimes(&config)?,
        }

        Ok(())
    }
}

impl Install {
    fn install_runtimes(&self, config: &Config, runtimes: &[RuntimeArg]) -> Result<()> {
        let runtimes = RuntimeArg::double_runtime_condition(runtimes);
        let mut ts = ToolsetBuilder::new().with_args(&runtimes).build(config);
        let plugins_to_install = runtimes.iter().map(|r| &r.plugin).collect::<HashSet<_>>();
        for plugin in ts.versions.clone().keys() {
            if !plugins_to_install.contains(plugin) {
                ts.versions.remove(plugin);
            }
        }
        if ts.versions.is_empty() {
            warn!("no runtimes to install");
            warn!("specify a version with `rtx install <PLUGIN>@<VERSION>`");
            return Ok(());
        }
        for (plugin, versions) in &ts.versions {
            if plugins_to_install.contains(plugin) && self.force {
                for v in &versions.versions {
                    if let Some(rtv) = &v.rtv {
                        if rtv.is_installed() {
                            info!("uninstalling {}", rtv);
                            rtv.uninstall()?;
                        }
                    }
                }
            }
        }
        ts.install_missing(config)?;

        Ok(())
    }

    fn install_missing_runtimes(&self, config: &Config) -> Result<()> {
        let mut ts = ToolsetBuilder::new().build(config);
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
        ts.install_missing(config)?;

        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx install nodejs@18.0.0  # install specific nodejs version
      $ rtx install nodejs@18      # install fuzzy nodejs version
      $ rtx install nodejs         # install version specified in .tool-versions
      $ rtx install                # installs all runtimes specified in .tool-versions for installed plugins
      $ rtx install --all          # installs all runtimes and all plugins
    "#, style("Examples:").bold().underlined()}
});

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
