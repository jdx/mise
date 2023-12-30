use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, ContextCompat, Result};
use console::style;
use itertools::Itertools;

use crate::cli::args::tool::{ToolArg, ToolArgParser};
use crate::config::{config_file, Config, Settings};
use crate::env::{RTX_DEFAULT_CONFIG_FILENAME, RTX_DEFAULT_TOOL_VERSIONS_FILENAME};
use crate::file::display_path;

use crate::plugins::PluginName;
use crate::{env, file};

/// Sets/gets tool version in local .tool-versions or .rtx.toml
///
/// Use this to set a tool's version when within a directory
/// Use `rtx global` to set a tool version globally
/// This uses `.tool-version` by default unless there is a `.rtx.toml` file or if `RTX_USE_TOML`
/// is set. A future v2 release of rtx will default to using `.rtx.toml`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, hide = true, alias = "l", after_long_help = AFTER_LONG_HELP)]
pub struct Local {
    /// Tool(s) to add to .tool-versions/.rtx.toml
    /// e.g.: node@20
    /// if this is a single tool with no version,
    /// the current value of .tool-versions/.rtx.toml will be displayed
    #[clap(value_name = "TOOL@VERSION", value_parser = ToolArgParser, verbatim_doc_comment)]
    tool: Vec<ToolArg>,

    /// Recurse up to find a .tool-versions file rather than using the current directory only
    /// by default this command will only set the tool in the current directory ("$PWD/.tool-versions")
    #[clap(short, long, verbatim_doc_comment)]
    parent: bool,

    /// Save exact version to `.tool-versions`
    /// e.g.: `rtx local --pin node@20` will save `node 20.0.0` to .tool-versions
    #[clap(long, verbatim_doc_comment, overrides_with = "fuzzy")]
    pin: bool,

    /// Save fuzzy version to `.tool-versions`
    /// e.g.: `rtx local --fuzzy node@20` will save `node 20` to .tool-versions
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

impl Local {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let path = if self.parent {
            get_parent_path()?
        } else {
            get_path()?
        };
        local(
            &config,
            &path,
            self.tool,
            self.remove,
            self.pin,
            self.fuzzy,
            self.path,
        )
    }
}

fn get_path() -> Result<PathBuf> {
    let rtx_toml = env::current_dir()?.join(RTX_DEFAULT_CONFIG_FILENAME.as_str());
    if *env::RTX_USE_TOML || rtx_toml.exists() {
        Ok(rtx_toml)
    } else {
        Ok(env::current_dir()?.join(RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str()))
    }
}

pub fn get_parent_path() -> Result<PathBuf> {
    let mut filenames = vec![RTX_DEFAULT_CONFIG_FILENAME.as_str()];
    if !*env::RTX_USE_TOML {
        filenames.push(RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());
    }
    file::find_up(&env::current_dir()?, &filenames)
        .wrap_err_with(|| eyre!("no {} file found", filenames.join(" or "),))
}

#[allow(clippy::too_many_arguments)]
pub fn local(
    config: &Config,
    path: &Path,
    runtime: Vec<ToolArg>,
    remove: Option<Vec<PluginName>>,
    pin: bool,
    fuzzy: bool,
    show_path: bool,
) -> Result<()> {
    let settings = Settings::try_get()?;
    let mut cf = config_file::parse_or_init(path)?;
    if show_path {
        rtxprintln!("{}", path.display());
        return Ok(());
    }

    if let Some(plugins) = &remove {
        for plugin in plugins {
            cf.remove_plugin(plugin);
        }
        let tools = plugins
            .iter()
            .map(|r| style(r).blue().for_stderr().to_string())
            .join(" ");
        rtxprintln!("{} {} {tools}", style("rtx").dim(), display_path(path));
    }

    if !runtime.is_empty() {
        let runtimes = ToolArg::double_tool_condition(&runtime);
        if cf.display_runtime(&runtimes)? {
            return Ok(());
        }
        let pin = pin || (settings.asdf_compat && !fuzzy);
        cf.add_runtimes(config, &runtimes, pin)?;
        let tools = runtimes.iter().map(|t| t.style()).join(" ");
        rtxprintln!("{} {} {tools}", style("rtx").dim(), display_path(path));
    }

    if !runtime.is_empty() || remove.is_some() {
        cf.save()?;
    } else {
        rtxprint!("{}", cf.dump());
    }

    Ok(())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  # set the current version of node to 20.x for the current directory
  # will use a precise version (e.g.: 20.0.0) in .tool-versions file
  $ <bold>rtx local node@20</bold>

  # set node to 20.x for the current project (recurses up to find .tool-versions)
  $ <bold>rtx local -p node@20</bold>

  # set the current version of node to 20.x for the current directory
  # will use a fuzzy version (e.g.: 20) in .tool-versions file
  $ <bold>rtx local --fuzzy node@20</bold>

  # removes node from .tool-versions
  $ <bold>rtx local --remove=node</bold>

  # show the current version of node in .tool-versions
  $ <bold>rtx local node</bold>
  20.0.0
"#
);

#[cfg(test)]
mod tests {

    use std::panic;

    use pretty_assertions::assert_str_eq;

    use crate::cli::tests::grep;
    use crate::test::reset_config;
    use crate::{dirs, env, file};

    #[test]
    fn test_local_remove() {
        run_test(|| {
            assert_cli!("install", "tiny@2");
            assert_cli_snapshot!("local", "--pin", "tiny@2");
            assert_cli_snapshot!("local", "tiny@2");
            assert_cli_snapshot!("local", "--remove", "tiny");
            let stdout = assert_cli!("ls", "--current");
            assert_snapshot!(
                grep(stdout, "tiny"),
                @"tiny   2.1.0       ~/.test-tool-versions 2"
            );
        });
    }

    #[test]
    fn test_local_pin() {
        run_test(|| {
            assert_cli!("local", "--pin", "tiny@1");
            let stdout = assert_cli!("local");
            assert_str_eq!(grep(stdout, "tiny"), "tiny 1.0.1");
            assert_cli!("local", "--pin", "tiny", "2");
            let stdout = assert_cli!("local");
            assert_str_eq!(grep(stdout, "tiny"), "tiny 2.1.0");
        });
    }

    #[test]
    fn test_local_path() {
        run_test(|| {
            assert_cli!("local", "dummy@path:.");
            let stdout = assert_cli!("local");
            assert_str_eq!(grep(stdout, "dummy"), "dummy path:.");
        });
    }

    #[test]
    fn test_local_ref() {
        run_test(|| {
            assert_cli!("local", "dummy@ref:master");
            let stdout = assert_cli!("local");
            assert_str_eq!(grep(stdout, "dummy"), "dummy ref:master");
        });
    }

    #[test]
    fn test_local_prefix() {
        run_test(|| {
            assert_cli!("local", "dummy@prefix:1");
            let stdout = assert_cli!("local");
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
            assert_str_eq!(stdout, "2");
        });
    }

    #[test]
    fn test_local_invalid_multiple_plugins() {
        run_test(|| {
            let err = assert_cli_err!("local", "tiny", "dummy");
            assert_str_eq!(err.to_string(), "invalid input, specify a version for each tool. Or just specify one tool to print the current version");
        });
    }

    #[test]
    fn test_local_invalid() {
        run_test(|| {
            let err = assert_cli_err!("local", "tiny", "dummy@latest");
            assert_str_eq!(err.to_string(), "invalid input, specify a version for each tool. Or just specify one tool to print the current version");
        });
    }

    #[test]
    fn test_local_alias_ref() {
        run_test(|| {
            assert_cli!("alias", "set", "dummy", "m", "ref:master");
            assert_cli!("local", "dummy@m");
            let stdout = assert_cli!("local");
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
            assert_cli!("local", "dummy@m");
            let stdout = assert_cli!("local");
            assert_str_eq!(grep(stdout, "dummy"), "dummy m");
            let stdout = assert_cli!("current", "dummy");
            assert_str_eq!(grep(stdout, "dummy"), "path:~/data/installs/dummy/1.1.0");
        });
    }

    #[test]
    fn test_local_alias_prefix() {
        run_test(|| {
            assert_cli!("alias", "set", "dummy", "m", "prefix:1");
            assert_cli!("local", "dummy@m");
            let stdout = assert_cli!("local");
            assert_str_eq!(grep(stdout, "dummy"), "dummy m");
            assert_cli_snapshot!("current", "dummy");
        });
    }

    #[test]
    fn test_local_alias_system() {
        run_test(|| {
            assert_cli!("alias", "set", "dummy", "m", "system");
            assert_cli!("local", "dummy@m");
            let stdout = assert_cli!("local");
            assert_str_eq!(grep(stdout, "dummy"), "dummy m");
            assert_cli_snapshot!("current", "dummy");
        });
    }

    fn run_test<T>(test: T)
    where
        T: FnOnce() + panic::UnwindSafe,
    {
        let _ = file::remove_file(env::current_dir().unwrap().join(".test.rtx.toml"));
        let cf_path = env::current_dir().unwrap().join(".test-tool-versions");
        let orig = file::read_to_string(&cf_path).unwrap();

        let result = panic::catch_unwind(test);

        file::write(cf_path, orig).unwrap();

        assert!(result.is_ok());
        reset_config();
    }
}
