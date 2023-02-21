use color_eyre::eyre::{eyre, ContextCompat, Result};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::{config_file, Config};
use crate::output::Output;
use crate::plugins::PluginName;
use crate::{dirs, env, file};

/// Sets .tool-versions to include a specific runtime
///
/// then displays the contents of .tool-versions
/// use this to set the runtime version when within a directory
/// use `rtx global` to set a runtime version globally
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "l", after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Local {
    /// runtimes to add to .tool-versions
    ///
    /// e.g.: nodejs@20
    /// if this is a single runtime with no version,
    /// the current value of .tool-versions will be displayed
    #[clap(value_parser = RuntimeArgParser, verbatim_doc_comment)]
    runtime: Option<Vec<RuntimeArg>>,

    /// recurse up to find a .tool-versions file rather than using the current directory only
    /// by default this command will only set the runtime in the current directory ("$PWD/.tool-versions")
    #[clap(short, long, verbatim_doc_comment)]
    parent: bool,

    /// save exact version to `.tool-versions`
    ///
    /// e.g.: `rtx local --pin nodejs@20` will save `nodejs 20.0.0` to .tool-versions,
    #[clap(long, verbatim_doc_comment)]
    pin: bool,

    #[clap(long, hide = true)]
    fuzzy: bool,

    /// remove the plugin(s) from .tool-versions
    #[clap(long, value_name = "PLUGIN")]
    remove: Option<Vec<PluginName>>,
}

impl Command for Local {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let cf_path = match self.parent {
            true => file::find_up(
                &dirs::CURRENT,
                &[env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str()],
            )
            .with_context(|| {
                eyre!(
                    "no {} file found",
                    env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str()
                )
            })?,
            false => dirs::CURRENT.join(env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str()),
        };

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
            let runtimes = RuntimeArg::double_runtime_condition(runtimes);
            if cf.display_runtime(out, &runtimes)? {
                return Ok(());
            }
            cf.add_runtimes(&config, &runtimes, self.pin)?;
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
      # set the current version of nodejs to 20.x for the current directory
      # will use a precise version (e.g.: 20.0.0) in .tool-versions file
      $ rtx local nodejs@20

      # set nodejs to 20.x for the current project (recurses up to find .tool-versions)
      $ rtx local -p nodejs@20

      # set the current version of nodejs to 20.x for the current directory
      # will use a fuzzy version (e.g.: 20) in .tool-versions file
      $ rtx local --fuzzy nodejs@20

      # removes nodejs from .tool-versions
      $ rtx local --remove=nodejs

      # show the current version of nodejs in .tool-versions
      $ rtx local nodejs
      20.0.0
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use std::fs;

    use insta::assert_snapshot;
    use pretty_assertions::assert_str_eq;

    use crate::cli::tests::grep;
    use crate::{assert_cli, assert_cli_err, dirs};

    #[test]
    fn test_local() {
        let cf_path = dirs::CURRENT.join(".tool-versions");
        let orig = fs::read_to_string(&cf_path).unwrap();

        assert_cli!("plugin", "add", "nodejs");
        assert_cli!("install", "shfmt@2");
        let stdout = assert_cli!("local", "--pin", "shfmt@2");
        assert_snapshot!(stdout);
        let stdout = assert_cli!("local", "shfmt@2");
        assert_snapshot!(stdout);
        let stdout = assert_cli!("local", "--remove", "nodejs");
        assert_snapshot!(stdout);
        let stdout = assert_cli!("ls", "--current");
        assert_str_eq!(
            grep(stdout, "nodejs"),
            "   nodejs 18.0.0 (missing)   (set by ~/cwd/.node-version)"
        );
        let stdout = assert_cli!("local", "--pin", "tiny@1");
        assert_str_eq!(grep(stdout, "tiny"), "tiny 1.0.1");
        let stdout = assert_cli!("local", "--pin", "tiny", "2");
        assert_str_eq!(grep(stdout, "tiny"), "tiny 2.1.0");

        // path
        let stdout = assert_cli!("local", "dummy@path:.");
        assert_str_eq!(grep(stdout, "dummy"), "dummy path:.");

        // ref
        let stdout = assert_cli!("local", "dummy@ref:master");
        assert_str_eq!(grep(stdout, "dummy"), "dummy ref:master");

        // prefix
        let stdout = assert_cli!("local", "dummy@prefix:1");
        assert_str_eq!(grep(stdout, "dummy"), "dummy prefix:1");

        // will output the current version(s)
        let stdout = assert_cli!("local", "tiny");
        assert_str_eq!(stdout, "2.1.0\n");

        // can only request a version one plugin at a time
        let err = assert_cli_err!("local", "tiny", "dummy");
        assert_str_eq!(err.to_string(), "invalid input, specify a version for each runtime. Or just specify one runtime to print the current version");

        // this is just invalid
        let err = assert_cli_err!("local", "tiny", "dummy@latest");
        assert_str_eq!(err.to_string(), "invalid input, specify a version for each runtime. Or just specify one runtime to print the current version");

        fs::write(cf_path, orig).unwrap();
    }
}
