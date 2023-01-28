use color_eyre::eyre::Result;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::shell::{get_shell, ShellType};

/// exports environment variables to activate rtx in a single shell session
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
            let k = k.to_string_lossy().to_string();
            let v = v.to_string_lossy().to_string();
            rtxprint!(out, "{}", shell.set_env(&k, &v));
        }
        rtxprintln!(
            out,
            "{}",
            shell.set_env("PATH", config.path_env()?.to_string_lossy().as_ref())
        );

        Ok(())
    }
}

const AFTER_LONG_HELP: &str = r#"
Examples:
  $ eval "$(rtx env -s bash)"
  $ eval "$(rtx env -s zsh)"
  $ rtx env -s fish | source
"#;

#[cfg(test)]
mod test {
    use crate::assert_cli;
    use crate::dirs;
    use crate::output::Output;

    #[test]
    fn test_env() {
        assert_cli!("plugin", "add", "shfmt");
        assert_cli!("install");
        let Output { stdout, .. } = assert_cli!("env", "-s", "bash");
        assert!(stdout.content.contains(
            dirs::ROOT
                .join("installs/shfmt/3.5.2/bin")
                .to_string_lossy()
                .as_ref()
        ));
    }

    #[test]
    fn test_env_with_runtime_arg() {
        assert_cli!("plugin", "add", "shfmt");
        assert_cli!("install", "shfmt@3.5");
        let Output { stdout, .. } = assert_cli!("env", "shfmt@3.5", "-s", "bash");

        assert!(stdout.content.contains(
            dirs::ROOT
                .join("installs/shfmt/3.5.2/bin")
                .to_string_lossy()
                .as_ref()
        ));
    }

    #[test]
    fn test_env_alias() {
        assert_cli!("plugin", "add", "shfmt");
        assert_cli!("install", "shfmt@my/alias");
        let Output { stdout, .. } = assert_cli!("env", "shfmt@my/alias", "-s", "bash");
        assert!(stdout.content.contains(
            dirs::ROOT
                .join("installs/shfmt/3.0.2")
                .to_string_lossy()
                .as_ref()
        ));
    }

    #[test]
    fn test_env_golang() {
        assert_cli!("plugin", "add", "golang");
        assert_cli!("install", "golang");
        let Output { stdout, .. } = assert_cli!("env", "golang", "-s", "bash");
        assert!(stdout.content.contains("GOROOT="));
    }
}
