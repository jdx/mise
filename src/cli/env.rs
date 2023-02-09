use color_eyre::eyre::Result;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::shell::{get_shell, ShellType};

/// exports env vars to activate rtx in a single shell session
///
/// It's not necessary to use this if you have `rtx activate` in your shell rc file.
/// Use this if you don't want to permanently install rtx.
/// This can be used similarly to `asdf shell`.
/// Unfortunately, it requires `eval` to work since it's not written in Bash though.
/// It's also useful just to see what environment variables rtx sets.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "e", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Env {
    /// Shell type to generate environment variables for
    #[clap(long, short)]
    shell: Option<ShellType>,

    /// runtime version to use
    #[clap(value_parser = RuntimeArgParser)]
    runtime: Vec<RuntimeArg>,
}

impl Command for Env {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let config = config.with_runtime_args(&self.runtime)?;
        config.ensure_installed()?;

        let shell = get_shell(self.shell);
        for (k, v) in config.env()? {
            let k = k.to_string();
            let v = v.to_string();
            rtxprint!(out, "{}", shell.set_env(&k, &v));
        }
        rtxprintln!(
            out,
            "{}",
            shell.set_env("PATH", config.path_env()?.as_ref())
        );

        Ok(())
    }
}

const AFTER_LONG_HELP: &str = r#"
Examples:
  $ eval "$(rtx env -s bash)"
  $ eval "$(rtx env -s zsh)"
  $ rtx env -s fish | source
  $ execx($(rtx env -s xonsh))
"#;

#[cfg(test)]
mod test {
    use crate::assert_cli;
    use crate::cli::test::grep;
    use crate::dirs;
    use pretty_assertions::assert_str_eq;

    #[test]
    fn test_env() {
        assert_cli!("plugin", "add", "shfmt");
        assert_cli!("install");
        let stdout = assert_cli!("env", "-s", "bash");
        assert!(stdout.contains(
            dirs::ROOT
                .join("installs/shfmt/3.5.1/bin")
                .to_string_lossy()
                .as_ref()
        ));
    }

    #[test]
    fn test_env_with_runtime_arg() {
        assert_cli!("plugin", "add", "shfmt");
        assert_cli!("install", "shfmt@3.5");
        let stdout = assert_cli!("env", "shfmt@3.5", "-s", "bash");

        assert!(stdout.contains(
            dirs::ROOT
                .join("installs/shfmt/3.5.1/bin")
                .to_string_lossy()
                .as_ref()
        ));
    }

    #[test]
    fn test_env_alias() {
        assert_cli!("plugin", "add", "shfmt");
        assert_cli!("install", "shfmt@my/alias");
        let stdout = assert_cli!("env", "shfmt@my/alias", "-s", "bash");
        assert!(stdout.contains(
            dirs::ROOT
                .join("installs/shfmt/3.0.2")
                .to_string_lossy()
                .as_ref()
        ));
    }

    #[test]
    fn test_env_tiny() {
        let stdout = assert_cli!("env", "tiny@1", "-s", "bash");
        assert_str_eq!(grep(stdout, "JDXCODE"), "export JDXCODE_TINY=1.0.1");
    }
}
