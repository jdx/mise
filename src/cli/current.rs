use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::Plugin;
use crate::toolset::{Toolset, ToolsetBuilder};

/// Shows current active and installed runtime versions
///
/// This is similar to `rtx ls --current`, but this only shows the runtime
/// and/or version. It's designed to fit into scripts more easily.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Current {
    /// Plugin to show versions of
    /// e.g.: ruby, nodejs
    #[clap()]
    plugin: Option<String>,
}

impl Command for Current {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new().build(&mut config);
        match &self.plugin {
            Some(plugin_name) => match config.plugins.get(plugin_name) {
                Some(plugin) => self.one(&config, ts, out, plugin),
                None => {
                    warn!("Plugin {} is not installed", plugin_name);
                    Ok(())
                }
            },
            None => self.all(&config, ts, out),
        }
    }
}

impl Current {
    fn one(&self, config: &Config, ts: Toolset, out: &mut Output, plugin: &Plugin) -> Result<()> {
        if !plugin.is_installed() {
            warn!("Plugin {} is not installed", plugin.name);
            return Ok(());
        }
        match ts.list_versions_by_plugin(config).get(&plugin.name) {
            Some(versions) => {
                rtxprintln!(
                    out,
                    "{}",
                    versions
                        .iter()
                        .map(|v| v.version.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
            None => {
                warn!("Plugin {} does not have a version set", plugin.name);
            }
        };
        Ok(())
    }

    fn all(&self, config: &Config, ts: Toolset, out: &mut Output) -> Result<()> {
        for (plugin, versions) in ts.list_versions_by_plugin(config) {
            if versions.is_empty() {
                continue;
            }
            for rtv in &versions {
                if !rtv.is_installed() {
                    let source = ts.versions.get(&rtv.plugin.name).unwrap().source.clone();
                    warn!(
                        "{}@{} is specified in {}, but not installed",
                        &rtv.plugin.name, &rtv.version, &source
                    );
                }
            }
            rtxprintln!(
                out,
                "{} {}",
                &plugin,
                versions
                    .iter()
                    .map(|v| v.version.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      # outputs `.tool-versions` compatible format
      $ rtx current
      python 3.11.0 3.10.0
      shfmt 3.6.0
      shellcheck 0.9.0
      nodejs 18.13.0

      $ rtx current nodejs
      18.13.0

      # can output multiple versions
      $ rtx current python
      3.11.0 3.10.0
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use std::env;

    use crate::{assert_cli, assert_cli_snapshot};

    #[test]
    fn test_current() {
        assert_cli_snapshot!("current");
    }

    #[test]
    fn test_current_with_runtimes() {
        assert_cli_snapshot!("current", "tiny");
    }

    #[test]
    fn test_current_missing() {
        assert_cli!("uninstall", "dummy@1.0.1");

        env::set_var("RTX_DUMMY_VERSION", "1.1.0");
        assert_cli_snapshot!("current");

        env::remove_var("RTX_DUMMY_VERSION");
    }
}
