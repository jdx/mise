use eyre::Result;

use crate::config::Config;

use crate::plugins::{unalias_plugin, Plugin};
use crate::toolset::{Toolset, ToolsetBuilder};

/// Shows current active and installed runtime versions
///
/// This is similar to `rtx ls --current`, but this only shows the runtime
/// and/or version. It's designed to fit into scripts more easily.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Current {
    /// Plugin to show versions of
    /// e.g.: ruby, node
    #[clap()]
    plugin: Option<String>,
}

impl Current {
    pub fn run(self, config: &Config) -> Result<()> {
        let ts = ToolsetBuilder::new().build(config)?;
        match &self.plugin {
            Some(plugin_name) => {
                let plugin_name = unalias_plugin(plugin_name);
                let plugin = config.get_or_create_plugin(plugin_name);
                if !plugin.is_installed() {
                    bail!("Plugin {} is not installed", plugin_name);
                }
                self.one(ts, plugin.as_ref())
            }
            None => self.all(ts),
        }
    }

    fn one(&self, ts: Toolset, tool: &dyn Plugin) -> Result<()> {
        if !tool.is_installed() {
            warn!("Plugin {} is not installed", tool.name());
            return Ok(());
        }
        match ts
            .list_versions_by_plugin()
            .into_iter()
            .find(|(p, _)| p.name() == tool.name())
        {
            Some((_, versions)) => {
                rtxprintln!(
                    "{}",
                    versions
                        .iter()
                        .map(|v| v.version.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
            None => {
                warn!("Plugin {} does not have a version set", tool.name());
            }
        };
        Ok(())
    }

    fn all(&self, ts: Toolset) -> Result<()> {
        for (plugin, versions) in ts.list_versions_by_plugin() {
            if versions.is_empty() {
                continue;
            }
            for tv in versions {
                if !plugin.is_version_installed(tv) {
                    let source = ts.versions.get(&tv.plugin_name).unwrap().source.clone();
                    warn!(
                        "{}@{} is specified in {}, but not installed",
                        tv.plugin_name, &tv.version, &source
                    );
                }
            }
            rtxprintln!(
                "{} {}",
                &plugin.name(),
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  # outputs `.tool-versions` compatible format
  $ <bold>rtx current</bold>
  python 3.11.0 3.10.0
  shfmt 3.6.0
  shellcheck 0.9.0
  node 20.0.0

  $ <bold>rtx current node</bold>
  20.0.0

  # can output multiple versions
  $ <bold>rtx current python</bold>
  3.11.0 3.10.0
"#
);

#[cfg(test)]
mod tests {
    use std::env;

    #[test]
    fn test_current() {
        assert_cli_snapshot!("current", @r###"
        tiny 3.1.0
        dummy ref:master
        "###);
    }

    #[test]
    fn test_current_with_runtimes() {
        assert_cli_snapshot!("current", "tiny", @"3.1.0");
    }

    #[test]
    fn test_current_missing() {
        assert_cli!("uninstall", "dummy@1.0.1");

        env::set_var("RTX_DUMMY_VERSION", "1.1.0");
        assert_cli_snapshot!("current", @r###"
        dummy 1.1.0
        tiny 3.1.0
        rtx dummy@1.1.0 is specified in RTX_DUMMY_VERSION=1.1.0, but not installed
        "###);

        env::remove_var("RTX_DUMMY_VERSION");
    }
}
