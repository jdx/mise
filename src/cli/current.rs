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
    ///
    /// e.g.: ruby, nodejs
    #[clap()]
    plugin: Option<String>,
}

impl Command for Current {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new().build(&config);
        match &self.plugin {
            Some(plugin_name) => match config.plugins.get(plugin_name) {
                Some(plugin) => self.one(ts, out, plugin),
                None => {
                    warn!("Plugin {} is not installed", plugin_name);
                    Ok(())
                }
            },
            None => self.all(ts, out),
        }
    }
}

impl Current {
    fn one(&self, ts: Toolset, out: &mut Output, plugin: &Plugin) -> Result<()> {
        if !plugin.is_installed() {
            warn!("Plugin {} is not installed", plugin.name);
            return Ok(());
        }
        match ts.list_versions_by_plugin().get(&plugin.name) {
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

    fn all(&self, ts: Toolset, out: &mut Output) -> Result<()> {
        for (plugin, versions) in ts.list_versions_by_plugin() {
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

    use insta::assert_snapshot;
    use pretty_assertions::assert_str_eq;

    use crate::assert_cli;
    use crate::cli::tests::grep;

    #[test]
    fn test_current() {
        assert_cli!("install");
        let stdout = assert_cli!("current");
        assert_snapshot!(grep(stdout, "shfmt"), @"shfmt 3.5.1");
    }

    #[test]
    fn test_current_with_runtimes() {
        assert_cli!("install");
        let stdout = assert_cli!("current", "shfmt");
        assert_snapshot!(stdout, @r###"
        3.5.1
        "###);
    }

    #[test]
    fn test_current_missing() {
        assert_cli!("uninstall", "dummy@1.0.1");

        env::set_var("RTX_DUMMY_VERSION", "1.1.0");
        let stdout = assert_cli!("current");
        assert_str_eq!(grep(stdout, "dummy"), "dummy 1.1.0");

        env::remove_var("RTX_DUMMY_VERSION");
    }
}
