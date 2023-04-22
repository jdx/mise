use std::path::PathBuf;

use color_eyre::eyre::Result;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::cli::local::local;
use crate::config::{Config, MissingRuntimeBehavior};
use crate::env::RTX_DEFAULT_CONFIG_FILENAME;
use crate::output::Output;
use crate::plugins::PluginName;
use crate::{dirs, env};

/// Change the active version of a tool locally or globally.
///
/// This will install the tool if it is not already installed.
/// By default, this will use an `.rtx.toml` file in the current directory.
/// Use the --global flag to use the global config file instead.
/// This replaces asdf's `local` and `global` commands, however those are still available in rtx.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "u", after_long_help = AFTER_LONG_HELP)]
pub struct Use {
    /// Tool(s) to add to config file
    /// e.g.: node@20
    /// If no version is specified, it will default to @latest
    #[clap(value_parser = RuntimeArgParser, verbatim_doc_comment, required_unless_present = "remove")]
    tool: Vec<RuntimeArg>,

    /// Save exact version to config file
    /// e.g.: `rtx use --pin node@20` will save `node 20.0.0` to ~/.tool-versions
    #[clap(long, verbatim_doc_comment, overrides_with = "fuzzy")]
    pin: bool,

    /// Save fuzzy version to config file
    /// e.g.: `rtx use --fuzzy node@20` will save `node 20` to ~/.tool-versions
    /// this is the default behavior unless RTX_ASDF_COMPAT=1
    #[clap(long, verbatim_doc_comment, overrides_with = "pin")]
    fuzzy: bool,

    /// Remove the tool(s) from config file
    #[clap(long, value_name = "TOOL", aliases = ["rm", "unset"])]
    remove: Option<Vec<PluginName>>,

    /// Use the global config file (~/.config/rtx/config.toml) instead of the local one
    #[clap(short, long, overrides_with = "path")]
    global: bool,

    /// Specify a path to a config file
    #[clap(short, long, overrides_with = "global", value_hint = clap::ValueHint::FilePath)]
    path: Option<PathBuf>,
}

impl Command for Use {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        config.settings.missing_runtime_behavior = MissingRuntimeBehavior::AutoInstall;
        let runtimes = self
            .tool
            .into_iter()
            .map(|r| match &r.tvr {
                Some(_) => r,
                None => RuntimeArg::parse(&format!("{}@latest", r.plugin)),
            })
            .collect();
        let path = match (self.global, self.path) {
            (true, _) => global_file(),
            (false, Some(p)) => p,
            (false, None) => dirs::CURRENT.join(&*RTX_DEFAULT_CONFIG_FILENAME),
        };
        local(
            config,
            out,
            &path,
            Some(runtimes),
            self.remove,
            self.pin,
            self.fuzzy,
            false,
        )
    }
}

fn global_file() -> PathBuf {
    env::RTX_CONFIG_FILE
        .clone()
        .unwrap_or_else(|| dirs::CONFIG.join("config.toml"))
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  # set the current version of node to 20.x in .rtx.toml of current directory
  # will write the fuzzy version (e.g.: 20)
  $ <bold>rtx use node@20</bold>

  # set the current version of node to 20.x in ~/.config/rtx/config.toml
  # will write the precise version (e.g.: 20.0.0)
  $ <bold>rtx use -g --pin node@20</bold>
"#
);

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use std::fs;

    use crate::{assert_cli, dirs};

    #[test]
    fn test_use_local() {
        let cf_path = dirs::CURRENT.join(".test.rtx.toml");
        let _ = fs::remove_file(&cf_path);

        assert_cli!("use", "tiny@2");
        assert_snapshot!(fs::read_to_string(&cf_path).unwrap());

        assert_cli!("use", "--pin", "tiny");
        assert_snapshot!(fs::read_to_string(&cf_path).unwrap());

        assert_cli!("use", "--fuzzy", "tiny@2");
        assert_snapshot!(fs::read_to_string(&cf_path).unwrap());

        assert_cli!(
            "use",
            "--rm",
            "tiny",
            "--path",
            &cf_path.to_string_lossy().to_string()
        );
        assert_snapshot!(fs::read_to_string(&cf_path).unwrap());

        let _ = fs::remove_file(&cf_path);
    }

    #[test]
    fn test_use_global() {
        let cf_path = dirs::CONFIG.join("config.toml");
        let orig = fs::read_to_string(&cf_path).unwrap();
        let _ = fs::remove_file(&cf_path);

        assert_cli!("use", "-g", "tiny@2");
        assert_snapshot!(fs::read_to_string(&cf_path).unwrap());

        fs::write(&cf_path, orig).unwrap();
    }
}
