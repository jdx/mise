use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, ContextCompat, Result};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::{config_file, Config};
use crate::env::{RTX_DEFAULT_CONFIG_FILENAME, RTX_DEFAULT_TOOL_VERSIONS_FILENAME};
use crate::output::Output;
use crate::plugins::PluginName;
use crate::{dirs, env, file};

/// Sets/gets tool version in local .tool-versions or .rtx.toml
///
/// Use this to set a tool's version when within a directory
/// Use `rtx global` to set a runtime version globally
/// This uses `.tool-version` by default unless there is a `.rtx.toml` file or if `RTX_USE_TOML`
/// is set. A future v2 release of rtx will default to using `.rtx.toml`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "l", after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Local {
    /// Runtimes to add to .tool-versions/.rtx.toml
    /// e.g.: nodejs@18
    /// if this is a single runtime with no version,
    /// the current value of .tool-versions/.rtx.toml will be displayed
    #[clap(value_parser = RuntimeArgParser, verbatim_doc_comment)]
    runtime: Option<Vec<RuntimeArg>>,

    /// Recurse up to find a .tool-versions file rather than using the current directory only
    /// by default this command will only set the runtime in the current directory ("$PWD/.tool-versions")
    #[clap(short, long, verbatim_doc_comment)]
    parent: bool,

    /// Save exact version to `.tool-versions`
    /// e.g.: `rtx local --pin nodejs@18` will save `nodejs 18.0.0` to .tool-versions
    #[clap(long, verbatim_doc_comment, overrides_with = "fuzzy")]
    pin: bool,

    /// Save fuzzy version to `.tool-versions`
    /// e.g.: `rtx local --fuzzy nodejs@18` will save `nodejs 18` to .tool-versions
    /// This is the default behavior unless RTX_ASDF_COMPAT=1
    #[clap(long, overrides_with = "pin")]
    fuzzy: bool,

    /// Remove the plugin(s) from .tool-versions
    #[clap(long, value_name = "PLUGIN", aliases = ["rm", "unset"])]
    remove: Option<Vec<PluginName>>,

    /// Get the path of the config file
    #[clap(long)]
    path: bool,
}

impl Command for Local {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let path = if self.parent {
            get_parent_path()?
        } else {
            get_path()
        };
        local(
            config,
            out,
            &path,
            self.runtime,
            self.remove,
            self.pin,
            self.fuzzy,
            self.path,
        )
    }
}

fn get_path() -> PathBuf {
    let rtx_toml = dirs::CURRENT.join(RTX_DEFAULT_CONFIG_FILENAME.as_str());
    if *env::RTX_USE_TOML || rtx_toml.exists() {
        rtx_toml
    } else {
        dirs::CURRENT.join(RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str())
    }
}

fn get_parent_path() -> Result<PathBuf> {
    let mut filenames = vec![RTX_DEFAULT_CONFIG_FILENAME.as_str()];
    if !*env::RTX_USE_TOML {
        filenames.push(RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());
    }
    file::find_up(&dirs::CURRENT, &filenames)
        .with_context(|| eyre!("no {} file found", filenames.join(" or "),))
}

#[allow(clippy::too_many_arguments)]
pub fn local(
    mut config: Config,
    out: &mut Output,
    path: &Path,
    runtime: Option<Vec<RuntimeArg>>,
    remove: Option<Vec<PluginName>>,
    pin: bool,
    fuzzy: bool,
    show_path: bool,
) -> Result<()> {
    let mut cf = match path.exists() {
        true => config_file::parse(path)?,
        false => config_file::init(path),
    };
    if show_path {
        rtxprintln!(out, "{}", path.display());
        return Ok(());
    }

    if let Some(plugins) = &remove {
        for plugin in plugins {
            cf.remove_plugin(plugin);
        }
    }

    if let Some(runtimes) = &runtime {
        let runtimes = RuntimeArg::double_runtime_condition(&runtimes.clone());
        if cf.display_runtime(out, &runtimes)? {
            return Ok(());
        }
        let pin = pin || (config.settings.asdf_compat && !fuzzy);
        cf.add_runtimes(&mut config, &runtimes, pin)?;
    }

    if runtime.is_some() || remove.is_some() {
        cf.save()?;
    }

    rtxprint!(out, "{}", cf.dump());

    Ok(())
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      # set the current version of nodejs to 18.x for the current directory
      # will use a precise version (e.g.: 18.0.0) in .tool-versions file
      $ rtx local nodejs@18

      # set nodejs to 18.x for the current project (recurses up to find .tool-versions)
      $ rtx local -p nodejs@18

      # set the current version of nodejs to 18.x for the current directory
      # will use a fuzzy version (e.g.: 18) in .tool-versions file
      $ rtx local --fuzzy nodejs@18

      # removes nodejs from .tool-versions
      $ rtx local --remove=nodejs

      # show the current version of nodejs in .tool-versions
      $ rtx local nodejs
      18.0.0
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use std::{fs, panic};

    use pretty_assertions::assert_str_eq;

    use crate::cli::tests::grep;
    use crate::test::reset_config;
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
            assert_str_eq!(grep(stdout, "tiny"), "tiny 1.0.1");
            let stdout = assert_cli!("local", "--pin", "tiny", "2");
            assert_str_eq!(grep(stdout, "tiny"), "tiny 2.1.0");
        });
    }
    #[test]
    fn test_local_path() {
        run_test(|| {
            let stdout = assert_cli!("local", "dummy@path:.");
            assert_str_eq!(grep(stdout, "dummy"), "dummy path:.");
        });
    }
    #[test]
    fn test_local_ref() {
        run_test(|| {
            let stdout = assert_cli!("local", "dummy@ref:master");
            assert_str_eq!(grep(stdout, "dummy"), "dummy ref:master");
        });
    }
    #[test]
    fn test_local_prefix() {
        run_test(|| {
            let stdout = assert_cli!("local", "dummy@prefix:1");
            assert_str_eq!(grep(stdout, "dummy"), "dummy prefix:1");
        });
    }
    #[test]
    fn test_local_multiple_versions() {
        run_test(|| {
            assert_cli_snapshot!("local", "--path");
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
            assert_str_eq!(grep(stdout, "dummy"), "dummy m");
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
            assert_str_eq!(grep(stdout, "dummy"), "dummy m");
            let stdout = assert_cli!("current", "dummy");
            assert_str_eq!(grep(stdout, "dummy"), "~/data/installs/dummy/1.1.0");
        });
    }
    #[test]
    fn test_local_alias_prefix() {
        run_test(|| {
            assert_cli!("alias", "set", "dummy", "m", "prefix:1");
            let stdout = assert_cli!("local", "dummy@m");
            assert_str_eq!(grep(stdout, "dummy"), "dummy m");
            assert_cli_snapshot!("current", "dummy");
        });
    }
    #[test]
    fn test_local_alias_system() {
        run_test(|| {
            assert_cli!("alias", "set", "dummy", "m", "system");
            let stdout = assert_cli!("local", "dummy@m");
            assert_str_eq!(grep(stdout, "dummy"), "dummy m");
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

        assert!(result.is_ok());
        reset_config();
    }
}
