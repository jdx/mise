use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::{config_file, Config};
use crate::output::Output;
use crate::plugins::PluginName;
use crate::{dirs, env};

/// Shows/sets the global runtime version(s)
///
/// Displays the contents of ~/.tool-versions after writing.
/// The file is `$HOME/.tool-versions` by default.
/// Use `rtx local` to set a runtime version locally in the current directory.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "g", after_long_help = AFTER_LONG_HELP.as_str())]
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
}

impl Command for Global {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
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
            let runtimes = RuntimeArg::double_runtime_condition(&runtimes.clone());
            if cf.display_runtime(out, &runtimes)? {
                return Ok(());
            }
            let pin = self.pin || (config.settings.asdf_compat && !self.fuzzy);
            cf.add_runtimes(&mut config, &runtimes, pin)?;
        }

        if self.runtime.is_some() || self.remove.is_some() {
            cf.save()?;
        }

        rtxprint!(out, "{}", cf.dump());

        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      # set the current version of nodejs to 18.x
      # will use a fuzzy version (e.g.: 18) in .tool-versions file
      $ rtx global --fuzzy nodejs@18

      # set the current version of nodejs to 18.x
      # will use a precise version (e.g.: 18.0.0) in .tool-versions file
      $ rtx global --pin nodejs@18

      # show the current version of nodejs in ~/.tool-versions
      $ rtx global nodejs
      18.0.0
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use std::fs;

    use insta::assert_snapshot;
    use pretty_assertions::assert_str_eq;

    use crate::{assert_cli, assert_cli_err, dirs};

    #[test]
    fn test_global() {
        let cf_path = dirs::HOME.join(".test-tool-versions");
        let orig = fs::read_to_string(&cf_path).ok();
        let _ = fs::remove_file(&cf_path);

        assert_cli!("install", "tiny@2");
        let stdout = assert_cli!("global", "--pin", "tiny@2");
        assert_snapshot!(stdout);
        let stdout = assert_cli!("global", "tiny@2");
        assert_snapshot!(stdout);
        let stdout = assert_cli!("global", "--remove", "tiny");
        assert_snapshot!(stdout);
        let stdout = assert_cli!("global", "--pin", "tiny", "2");
        assert_snapshot!(stdout);

        // will output the current version(s)
        let stdout = assert_cli!("global", "tiny");
        assert_str_eq!(stdout, "2.1.0\n");

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

        if let Some(orig) = orig {
            fs::write(cf_path, orig).unwrap();
        }
    }
}
