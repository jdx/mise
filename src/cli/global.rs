use color_eyre::eyre::Result;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::{config_file, Config};
use crate::output::Output;
use crate::plugins::PluginName;
use crate::{dirs, env};

/// sets global .tool-versions to include a specified runtime
///
/// this file is `$HOME/.tool-versions` by default
/// use `rtx local` to set a runtime version locally in the current directory
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "g", after_long_help = AFTER_LONG_HELP)]
pub struct Global {
    /// runtimes
    ///
    /// e.g.: nodejs@20
    #[clap(value_parser = RuntimeArgParser)]
    runtime: Option<Vec<RuntimeArg>>,

    /// save fuzzy match to .tool-versions
    /// e.g.: `rtx global --fuzzy nodejs@20` will save `nodejs 20` to .tool-versions,
    /// by default, it would save the exact version, e.g.: `nodejs 20.0.0`
    #[clap(long)]
    fuzzy: bool,

    /// remove the plugin(s) from ~/.tool-versions
    #[clap(long, value_name = "PLUGIN")]
    remove: Option<Vec<PluginName>>,
}

impl Command for Global {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let cf_path = dirs::HOME.join(env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());

        let mut cf = match cf_path.exists() {
            true => config_file::parse(&cf_path)?,
            false => config_file::init(&cf_path),
        };

        if let Some(plugins) = &self.remove {
            for plugin in plugins {
                cf.remove_plugin(plugin);
            }
        }
        if let Some(runtimes) = &self.runtime {
            cf.add_runtimes(&config, runtimes, self.fuzzy)?;
        }

        if self.runtime.is_some() || self.remove.is_some() {
            cf.save()?;
        }

        rtxprint!(out, "{}", cf.dump());

        Ok(())
    }
}

const AFTER_LONG_HELP: &str = r#"
Examples:
  # set the current version of nodejs to 20.x
  # will use a precise version (e.g.: 20.0.0) in .tool-versions file
  $ rtx global nodejs@20     

  # set the current version of nodejs to 20.x
  # will use a fuzzy version (e.g.: 20) in .tool-versions file
  $ rtx global --fuzzy nodejs@20
"#;

#[cfg(test)]
mod test {
    use std::fs;

    use insta::assert_snapshot;

    use crate::output::Output;
    use crate::{assert_cli, dirs};

    #[test]
    fn test_global() {
        let cf_path = dirs::HOME.join(".tool-versions");
        let orig = fs::read_to_string(&cf_path).ok();
        let _ = fs::remove_file(&cf_path);

        assert_cli!("install", "shfmt@2");
        let Output { stdout, .. } = assert_cli!("global", "shfmt@2");
        assert_snapshot!(stdout.content);
        let Output { stdout, .. } = assert_cli!("global", "--fuzzy", "shfmt@2");
        assert_snapshot!(stdout.content);
        let Output { stdout, .. } = assert_cli!("global", "--remove", "nodejs");
        assert_snapshot!(stdout.content);

        if let Some(orig) = orig {
            fs::write(cf_path, orig).unwrap();
        }
    }
}
