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
    /// e.g.: `rtx local --pin nodejs@20` will save `nodejs 20.0.0` to .tool-versions
    #[clap(long, verbatim_doc_comment, overrides_with = "fuzzy")]
    pin: bool,

    /// save fuzzy version to `.tool-versions`
    ///
    /// e.g.: `rtx local --fuzzy nodejs@20` will save `nodejs 20` to .tool-versions
    /// this is the default behavior unless RTX_ASDF_COMPAT=1
    #[clap(long, overrides_with = "pin")]
    fuzzy: bool,

    /// remove the plugin(s) from .tool-versions
    #[clap(long, value_name = "PLUGIN", aliases = ["rm", "unset"])]
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
            let pin = self.pin || (config.settings.asdf_compat && !self.fuzzy);
            cf.add_runtimes(&config, &runtimes, pin)?;
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
    use std::{fs, panic};

    use insta::assert_snapshot;
    use pretty_assertions::assert_str_eq;

    use crate::cli::tests::grep;
    use crate::{assert_cli, assert_cli_err, assert_cli_snapshot, dirs};

    #[test]
    fn test_local_remove() {
        run_test(|| {
            assert_cli!("install", "tiny@2");
            assert_cli_snapshot!("local", "--pin", "tiny@2");
            assert_cli_snapshot!("local", "tiny@2");
            assert_cli_snapshot!("local", "--remove", "tiny");
            let stdout = assert_cli!("ls", "--current");
            assert_str_eq!(
                grep(stdout, "tiny"),
                "-> tiny 2.1.0                (set by ~/.test-tool-versions)"
            );
        });
    }
    #[test]
    fn test_local_pin() {
        run_test(|| {
            let stdout = assert_cli!("local", "--pin", "tiny@1");
            assert_str_eq!(grep(stdout, "tiny"), "tiny       1.0.1");
            let stdout = assert_cli!("local", "--pin", "tiny", "2");
            assert_str_eq!(grep(stdout, "tiny"), "tiny       2.1.0");
        });
    }
    #[test]
    fn test_local_path() {
        run_test(|| {
            let stdout = assert_cli!("local", "dummy@path:.");
            assert_str_eq!(grep(stdout, "dummy"), "dummy      path:.");
        });
    }
    #[test]
    fn test_local_ref() {
        run_test(|| {
            let stdout = assert_cli!("local", "dummy@ref:master");
            assert_str_eq!(grep(stdout, "dummy"), "dummy      ref:master");
        });
    }
    #[test]
    fn test_local_prefix() {
        run_test(|| {
            let stdout = assert_cli!("local", "dummy@prefix:1");
            assert_str_eq!(grep(stdout, "dummy"), "dummy      prefix:1");
        });
    }
    #[test]
    fn test_local_multiple_versions() {
        run_test(|| {
            assert_cli_snapshot!("local", "tiny@2", "tiny@1", "tiny@3");
            assert_cli_snapshot!("bin-paths");
        });
    }
    #[test]
    fn test_local_output_current_version() {
        run_test(|| {
            assert_cli!("local", "tiny", "2");
            let stdout = assert_cli!("local", "tiny");
            assert_str_eq!(stdout, "2\n");
        });
    }
    #[test]
    fn test_local_invalid_multiple_plugins() {
        run_test(|| {
            let err = assert_cli_err!("local", "tiny", "dummy");
            assert_str_eq!(err.to_string(), "invalid input, specify a version for each runtime. Or just specify one runtime to print the current version");
        });
    }
    #[test]
    fn test_local_invalid() {
        run_test(|| {
            let err = assert_cli_err!("local", "tiny", "dummy@latest");
            assert_str_eq!(err.to_string(), "invalid input, specify a version for each runtime. Or just specify one runtime to print the current version");
        });
    }
    #[test]
    fn test_local_alias_ref() {
        run_test(|| {
            assert_cli!("alias", "set", "dummy", "m", "ref:master");
            let stdout = assert_cli!("local", "dummy@m");
            assert_str_eq!(grep(stdout, "dummy"), "dummy      m");
            assert_cli_snapshot!("current", "dummy");
        });
    }
    #[test]
    fn test_local_alias_path() {
        run_test(|| {
            assert_cli!("install", "dummy@1.1.0");
            let local_dummy_path = dirs::INSTALLS.join("dummy").join("1.1.0");
            let path_arg = String::from("path:") + &local_dummy_path.to_string_lossy();
            assert_cli!("alias", "set", "dummy", "m", &path_arg);
            let stdout = assert_cli!("local", "dummy@m");
            assert_str_eq!(grep(stdout, "dummy"), "dummy      m");
            let stdout = assert_cli!("current", "dummy");
            assert_str_eq!(grep(stdout, "dummy"), "~/data/installs/dummy/1.1.0");
        });
    }
    #[test]
    fn test_local_alias_prefix() {
        run_test(|| {
            assert_cli!("alias", "set", "dummy", "m", "prefix:1");
            let stdout = assert_cli!("local", "dummy@m");
            assert_str_eq!(grep(stdout, "dummy"), "dummy      m");
            assert_cli_snapshot!("current", "dummy");
        });
    }
    #[test]
    fn test_local_alias_system() {
        run_test(|| {
            assert_cli!("alias", "set", "dummy", "m", "system");
            let stdout = assert_cli!("local", "dummy@m");
            assert_str_eq!(grep(stdout, "dummy"), "dummy      m");
            assert_cli_snapshot!("current", "dummy");
        });
    }

    fn run_test<T>(test: T)
    where
        T: FnOnce() + panic::UnwindSafe,
    {
        let cf_path = dirs::CURRENT.join(".test-tool-versions");
        let orig = fs::read_to_string(&cf_path).unwrap();

        let result = panic::catch_unwind(test);

        fs::write(cf_path, orig).unwrap();

        assert!(result.is_ok())
    }
}
