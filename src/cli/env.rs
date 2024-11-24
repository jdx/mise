use eyre::Result;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::shell::{get_shell, ShellType};
use crate::toolset::{InstallOptions, Toolset, ToolsetBuilder};

/// Exports env vars to activate mise a single time
///
/// Use this if you don't want to permanently install mise. It's not necessary to
/// use this if you have `mise activate` in your shell rc file.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "e", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Env {
    /// Tool(s) to use
    #[clap(value_name = "TOOL@VERSION")]
    tool: Vec<ToolArg>,

    /// Output in JSON format
    #[clap(long, short = 'J', overrides_with = "shell")]
    json: bool,

    /// Output in dotenv format
    #[clap(long, short = 'D', overrides_with = "shell")]
    dotenv: bool,

    /// Shell type to generate environment variables for
    #[clap(long, short, overrides_with = "json")]
    shell: Option<ShellType>,
}

impl Env {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let mut ts = ToolsetBuilder::new().with_args(&self.tool).build(&config)?;
        ts.install_missing_versions(&InstallOptions::default())?;
        ts.notify_if_versions_missing();

        if self.json {
            self.output_json(&config, ts)
        } else if self.shell.is_some() {
            self.output_shell(&config, ts)
        } else {
            self.output_dotenv(&config, ts)
        }
    }

    fn output_json(&self, config: &Config, ts: Toolset) -> Result<()> {
        let env = ts.env_with_path(config)?;
        miseprintln!("{}", serde_json::to_string_pretty(&env)?);
        Ok(())
    }

    fn output_shell(&self, config: &Config, ts: Toolset) -> Result<()> {
        let default_shell = get_shell(Some(ShellType::Bash)).unwrap();
        let shell = get_shell(self.shell).unwrap_or(default_shell);
        for (k, v) in ts.env_with_path(config)? {
            let k = k.to_string();
            let v = v.to_string();
            miseprint!("{}", shell.set_env(&k, &v))?;
        }
        Ok(())
    }

    fn output_dotenv(&self, config: &Config, ts: Toolset) -> Result<()> {
        for (k, v) in ts.env(config)? {
            let k = k.to_string();
            let v = v.to_string();
            miseprint!("{}={}\n", k, v)?;
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>eval "$(mise env -s bash)"</bold>
    $ <bold>eval "$(mise env -s zsh)"</bold>
    $ <bold>mise env -s fish | source</bold>
    $ <bold>execx($(mise env -s xonsh))</bold>
"#
);

#[cfg(test)]
mod tests {
    use std::env;

    use pretty_assertions::assert_str_eq;
    use test_log::test;

    use crate::cli::tests::grep;
    use crate::dirs;
    use crate::test::reset;

    #[test]
    fn test_env() {
        reset();
        let stdout = assert_cli!("env", "-s", "bash");
        assert!(stdout.contains(
            dirs::DATA
                .join("installs/tiny/3.1.0/bin")
                .to_string_lossy()
                .as_ref()
        ));
    }

    #[test]
    fn test_env_with_runtime_arg() {
        reset();
        assert_cli!("install", "tiny@3.0");
        let stdout = assert_cli!("env", "tiny@3.0", "-s", "bash");

        assert!(stdout.contains(
            dirs::DATA
                .join("installs/tiny/3.0.1/bin")
                .to_string_lossy()
                .as_ref()
        ));
    }

    #[test]
    fn test_env_alias() {
        reset();
        assert_cli!("plugin", "add", "tiny");
        assert_cli!("install", "tiny@my/alias");
        let stdout = assert_cli!("env", "tiny@my/alias", "-s", "bash");
        assert!(stdout.contains(
            dirs::DATA
                .join("installs/tiny/3.0.1")
                .to_string_lossy()
                .as_ref()
        ));
    }

    #[test]
    fn test_env_tiny() {
        reset();
        let stdout = assert_cli!("env", "tiny@2", "tiny@1", "tiny@3", "-s", "bash");
        assert_str_eq!(grep(stdout, "JDXCODE"), "export JDXCODE_TINY=2.1.0");
    }

    #[test]
    fn test_env_default_shell() {
        reset();
        env::set_var("SHELL", "");
        let stdout = assert_cli!("env");
        assert!(stdout.contains("export PATH="));
    }

    #[test]
    fn test_env_json() {
        reset();
        assert_cli_snapshot!("env", "-J");
    }
}
