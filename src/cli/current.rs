use console::style;
use eyre::Result;

use crate::cli::args::ForgeArg;
use crate::config::Config;
use crate::forge;
use crate::forge::Forge;
use crate::toolset::{Toolset, ToolsetBuilder};

/// Shows current active and installed runtime versions
///
/// This is similar to `mise ls --current`, but this only shows the runtime
/// and/or version. It's designed to fit into scripts more easily.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Current {
    /// Plugin to show versions of
    /// e.g.: ruby, node, cargo:eza, npm:prettier, etc.
    #[clap()]
    plugin: Option<ForgeArg>,
}

impl Current {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let ts = ToolsetBuilder::new().build(&config)?;
        match &self.plugin {
            Some(fa) => {
                let plugin = forge::get(fa);
                if !plugin.is_installed() {
                    bail!("Plugin {fa} is not installed");
                }
                self.one(ts, plugin.as_ref())
            }
            None => self.all(ts),
        }
    }

    fn one(&self, ts: Toolset, tool: &dyn Forge) -> Result<()> {
        if !tool.is_installed() {
            warn!("Plugin {} is not installed", tool.id());
            return Ok(());
        }
        match ts
            .list_versions_by_plugin()
            .into_iter()
            .find(|(p, _)| p.id() == tool.id())
        {
            Some((_, versions)) => {
                miseprintln!(
                    "{}",
                    versions
                        .iter()
                        .map(|v| v.version.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
            None => {
                warn!(
                    "Plugin {} does not have a version set",
                    style(tool.id()).blue().for_stderr()
                );
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
                    let source = ts.versions.get(&tv.forge).unwrap().source.clone();
                    warn!(
                        "{}@{} is specified in {}, but not installed",
                        &tv.forge, &tv.version, &source
                    );
                }
            }
            miseprintln!(
                "{} {}",
                &plugin.id(),
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
    $ <bold>mise current</bold>
    python 3.11.0 3.10.0
    shfmt 3.6.0
    shellcheck 0.9.0
    node 20.0.0
  
    $ <bold>mise current node</bold>
    20.0.0
  
    # can output multiple versions
    $ <bold>mise current python</bold>
    3.11.0 3.10.0
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset;
    use std::env;
    use test_log::test;

    #[test]
    fn test_current() {
        reset();
        assert_cli_snapshot!("current", @r###"
        tiny 3.1.0
        dummy ref:master
        "###);
    }

    #[test]
    fn test_current_with_runtimes() {
        reset();
        assert_cli_snapshot!("current", "tiny", @"3.1.0");
    }

    #[test]
    fn test_current_missing() {
        reset();
        assert_cli!("uninstall", "--all", "dummy");

        env::set_var("MISE_DUMMY_VERSION", "1.1.0");
        assert_cli_snapshot!("current", @r###"
        dummy 1.1.0
        tiny 3.1.0
        mise dummy@1.1.0 is specified in MISE_DUMMY_VERSION=1.1.0, but not installed
        "###);

        env::remove_var("MISE_DUMMY_VERSION");
    }
}
