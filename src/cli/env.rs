use color_eyre::eyre::Result;

use crate::cli::args::tool::{ToolArg, ToolArgParser};
use crate::config::Config;

use crate::shell::{get_shell, ShellType};
use crate::toolset::{InstallOptions, Toolset, ToolsetBuilder};

/// Exports env vars to activate rtx a single time
///
/// Use this if you don't want to permanently install rtx. It's not necessary to
/// use this if you have `rtx activate` in your shell rc file.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "e", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Env {
    /// Shell type to generate environment variables for
    #[clap(long, short, overrides_with = "json")]
    shell: Option<ShellType>,

    /// Tool(s) to use
    #[clap(value_name = "TOOL@VERSION", value_parser = ToolArgParser)]
    tool: Vec<ToolArg>,

    /// Number of jobs to run in parallel
    /// [default: 4]
    #[clap(long, short, env = "RTX_JOBS", verbatim_doc_comment)]
    jobs: Option<usize>,

    /// Output in JSON format
    #[clap(long, short = 'J', overrides_with = "shell")]
    json: bool,

    /// Directly pipe stdin/stdout/stderr from plugin to user
    /// Sets --jobs=1
    #[clap(long, overrides_with = "jobs")]
    raw: bool,
}

impl Env {
    pub fn run(self, mut config: Config) -> Result<()> {
        if self.raw {
            config.settings.raw = true;
        }
        let mut ts = ToolsetBuilder::new().with_args(&self.tool).build(&config)?;
        let opts = InstallOptions {
            force: false,
            jobs: self.jobs,
            raw: self.raw,
        };
        ts.install_arg_versions(&config, &opts)?;

        if self.json {
            self.output_json(config, ts)
        } else {
            self.output_shell(config, ts)
        }
    }

    fn output_json(&self, config: Config, ts: Toolset) -> Result<()> {
        let env = ts.env_with_path(&config);
        rtxprintln!("{}", serde_json::to_string_pretty(&env)?);
        Ok(())
    }

    fn output_shell(&self, config: Config, ts: Toolset) -> Result<()> {
        let default_shell = get_shell(Some(ShellType::Bash)).unwrap();
        let shell = get_shell(self.shell).unwrap_or(default_shell);
        for (k, v) in ts.env_with_path(&config) {
            let k = k.to_string();
            let v = v.to_string();
            rtxprint!("{}", shell.set_env(&k, &v));
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>eval "$(rtx env -s bash)"</bold>
  $ <bold>eval "$(rtx env -s zsh)"</bold>
  $ <bold>rtx env -s fish | source</bold>
  $ <bold>execx($(rtx env -s xonsh))</bold>
"#
);

#[cfg(test)]
mod tests {
    use std::env;

    use pretty_assertions::assert_str_eq;

    use crate::cli::tests::grep;
    use crate::dirs;
    use crate::{assert_cli, assert_cli_snapshot};

    #[test]
    fn test_env() {
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
        let stdout = assert_cli!("env", "tiny@2", "tiny@1", "tiny@3", "-s", "bash");
        assert_str_eq!(grep(stdout, "JDXCODE"), "export JDXCODE_TINY=2.1.0");
    }

    #[test]
    fn test_env_default_shell() {
        env::set_var("SHELL", "");
        let stdout = assert_cli!("env");
        assert!(stdout.contains("export PATH="));
    }

    #[test]
    fn test_env_json() {
        assert_cli_snapshot!("env", "-J");
    }
}
