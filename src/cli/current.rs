use color_eyre::eyre::Result;
use std::sync::Arc;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::Plugin;

/// Shows currently active, and installed runtime versions
///
/// This is similar to `rtx list --current`, but this
/// only shows the runtime and/or version so it's
/// designed to fit into scripts more easily.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Current {
    /// plugin to show versions of
    ///
    /// e.g.: ruby, nodejs
    #[clap()]
    plugin: Option<String>,
}

impl Command for Current {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        match &self.plugin {
            Some(plugin_name) => match config.ts.find_plugin(plugin_name) {
                Some(plugin) => self.one(&config, out, plugin),
                None => {
                    warn!("Plugin {} is not installed", plugin_name);
                    Ok(())
                }
            },
            None => self.all(&config, out),
        }
    }
}

impl Current {
    fn one(&self, config: &Config, out: &mut Output, plugin: Arc<Plugin>) -> Result<()> {
        if !plugin.is_installed() {
            warn!("Plugin {} is not installed", plugin.name);
            return Ok(());
        }
        let versions = config.ts.list_current_versions_by_plugin();
        match versions.get(&plugin.name) {
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

    fn all(&self, config: &Config, out: &mut Output) -> Result<()> {
        for (plugin, versions) in config.ts.list_current_versions_by_plugin() {
            for rtv in &versions {
                if !rtv.is_installed() {
                    let source = config.ts.get_source_for_plugin(&rtv.plugin.name).unwrap();
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

const AFTER_LONG_HELP: &str = r#"
Examples:

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
"#;

#[cfg(test)]
mod test {
    use insta::assert_snapshot;

    use crate::assert_cli;
    use crate::cli::test::grep;

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
}
