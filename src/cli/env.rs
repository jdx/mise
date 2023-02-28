use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::args::runtime::{RuntimeArg, RuntimeArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::shell::{get_shell, ShellType};
use crate::toolset::ToolsetBuilder;

/// Exports env vars to activate rtx a single time
///
/// Use this if you don't want to permanently install rtx. It's not necessary to
/// use this if you have `rtx activate` in your shell rc file.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "e", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Env {
    /// Shell type to generate environment variables for
    #[clap(long, short)]
    shell: Option<ShellType>,

    /// Runtime version to use
    #[clap(value_parser = RuntimeArgParser)]
    runtime: Vec<RuntimeArg>,
}

impl Command for Env {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new()
            .with_install_missing()
            .with_args(&self.runtime)
            .build(&config);

        let default_shell = get_shell(Some(ShellType::Bash)).unwrap();
        let shell = get_shell(self.shell).unwrap_or(default_shell);
        for (k, v) in ts.env() {
            let k = k.to_string();
            let v = v.to_string();
            rtxprint!(out, "{}", shell.set_env(&k, &v));
        }
        let path = ts.path_env(&config.settings);
        rtxprintln!(out, "{}", shell.set_env("PATH", &path));

        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ eval "$(rtx env -s bash)"
      $ eval "$(rtx env -s zsh)"
      $ rtx env -s fish | source
      $ execx($(rtx env -s xonsh))
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use std::env;

    use pretty_assertions::assert_str_eq;

    use crate::assert_cli;
    use crate::cli::tests::grep;
    use crate::dirs;

    #[test]
    fn test_env() {
        let stdout = assert_cli!("env", "-s", "bash");
        assert!(stdout.contains(
            dirs::ROOT
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
            dirs::ROOT
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
            dirs::ROOT
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
}
