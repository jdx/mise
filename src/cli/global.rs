use std::path::PathBuf;

use color_eyre::eyre::Result;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::cli::local::local;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::PluginName;
use crate::{dirs, env};

/// Sets/gets the global runtime version(s)
///
/// Displays the contents of ~/.tool-versions after writing.
/// The file is `$HOME/.tool-versions` by default. It can be changed with `$RTX_CONFIG_FILE`.
/// If `$RTX_CONFIG_FILE` is set to anything that ends in `.toml`, it will be parsed as `.rtx.toml`.
/// Otherwise, it will be parsed as a `.tool-versions` file.
/// A future v2 release of rtx will default to using `~/.config/rtx/config.toml` instead.
///
/// Use `rtx local` to set a runtime version locally in the current directory.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "g", after_long_help = AFTER_LONG_HELP)]
pub struct Global {
    /// Runtime(s) to add to .tool-versions
    /// e.g.: nodejs@18
    /// If this is a single runtime with no version, the current value of the global
    /// .tool-versions will be displayed
    #[clap(value_parser = RuntimeArgParser, verbatim_doc_comment)]
    runtime: Option<Vec<RuntimeArg>>,

    /// Save exact version to `~/.tool-versions`
    /// e.g.: `rtx local --pin nodejs@18` will save `nodejs 18.0.0` to ~/.tool-versions
    #[clap(long, verbatim_doc_comment, overrides_with = "fuzzy")]
    pin: bool,

    /// Save fuzzy version to `~/.tool-versions`
    /// e.g.: `rtx local --fuzzy nodejs@18` will save `nodejs 18` to ~/.tool-versions
    /// this is the default behavior unless RTX_ASDF_COMPAT=1
    #[clap(long, verbatim_doc_comment, overrides_with = "pin")]
    fuzzy: bool,

    /// Remove the plugin(s) from ~/.tool-versions
    #[clap(long, value_name = "PLUGIN", aliases = ["rm", "unset"])]
    remove: Option<Vec<PluginName>>,

    /// Get the path of the global config file
    #[clap(long)]
    path: bool,
}

impl Command for Global {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        config.autoupdate();
        local(
            config,
            out,
            &global_file(),
            self.runtime,
            self.remove,
            self.pin,
            self.fuzzy,
            self.path,
        )
    }
}

fn global_file() -> PathBuf {
    env::RTX_CONFIG_FILE.clone().unwrap_or_else(|| {
        if *env::RTX_USE_TOML {
            dirs::CONFIG.join("config.toml")
        } else {
            dirs::HOME.join(env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str())
        }
    })
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  # set the current version of nodejs to 18.x
  # will use a fuzzy version (e.g.: 18) in .tool-versions file
  $ <bold>rtx global --fuzzy nodejs@18</bold>

  # set the current version of nodejs to 18.x
  # will use a precise version (e.g.: 18.0.0) in .tool-versions file
  $ <bold>rtx global --pin nodejs@18</bold>

  # show the current version of nodejs in ~/.tool-versions
  $ <bold>rtx global nodejs</bold>
  18.0.0
"#
);

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_str_eq;

    use crate::{assert_cli, assert_cli_err, assert_cli_snapshot, dirs};

    #[test]
    fn test_global() {
        let cf_path = dirs::HOME.join(".test-tool-versions");
        let orig = fs::read_to_string(&cf_path).ok();
        let _ = fs::remove_file(&cf_path);

        assert_cli!("install", "tiny@2");
        assert_cli_snapshot!("global", "--pin", "tiny@2");
        assert_cli_snapshot!("global", "tiny@2");
        assert_cli_snapshot!("global", "--remove", "tiny");
        assert_cli_snapshot!("global", "--pin", "tiny", "2");

        // will output the current version(s)
        assert_cli_snapshot!("global", "tiny");

        // this plugin isn't installed
        let err = assert_cli_err!("global", "invalid-plugin");
        assert_str_eq!(
            err.to_string(),
            "no version set for invalid-plugin in ~/.test-tool-versions"
        );

        // can only request a version one plugin at a time
        let err = assert_cli_err!("global", "tiny", "dummy");
        assert_str_eq!(err.to_string(), "invalid input, specify a version for each runtime. Or just specify one runtime to print the current version");

        // this is just invalid
        let err = assert_cli_err!("global", "tiny", "dummy@latest");
        assert_str_eq!(err.to_string(), "invalid input, specify a version for each runtime. Or just specify one runtime to print the current version");

        assert_cli_snapshot!("global", "--path");

        if let Some(orig) = orig {
            fs::write(cf_path, orig).unwrap();
        }
    }
}
