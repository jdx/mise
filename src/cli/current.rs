use color_eyre::eyre::Result;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

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
        let plugin = match self.plugin {
            Some(plugin_name) => {
                let plugin = config.ts.find_plugin(&plugin_name);
                if plugin.is_none() {
                    warn!("Plugin {} is not installed", plugin_name);
                    return Ok(());
                }
                Some(plugin.unwrap())
            }
            None => None,
        };

        for rtv in config.ts.list_current_versions() {
            if !rtv.is_installed() {
                let source = config.ts.get_source_for_plugin(&rtv.plugin.name).unwrap();
                warn!(
                    "{}@{} is specified in {}, but not installed",
                    rtv.plugin.name, rtv.version, source
                );
                continue;
            }
            if let Some(plugin) = &plugin {
                if plugin != &rtv.plugin {
                    continue;
                }
                rtxprintln!(out, "{}", rtv.version);
            } else {
                rtxprintln!(out, "{}@{}", rtv.plugin.name, rtv.version);
            }
        }
        Ok(())
    }
}

const AFTER_LONG_HELP: &str = r#"
Examples:

  $ rtx current
  shfmt@3.6.0
  shellcheck@0.9.0
  nodejs@18.13.0

  $ rtx current nodejs
  18.13.0
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
        assert_snapshot!(grep(stdout, "shfmt"), @"shfmt@3.5.2");
    }

    #[test]
    fn test_current_with_runtimes() {
        assert_cli!("install");
        let stdout = assert_cli!("current", "shfmt");
        assert_snapshot!(stdout, @r###"
        3.5.2
        "###);
    }
}
