use color_eyre::eyre::{eyre, Result};
use console::style;

use crate::cli::args::tool::{ToolArg, ToolArgParser};
use crate::config::Config;
use crate::output::Output;
use crate::shell::get_shell;
use crate::toolset::{ToolSource, ToolsetBuilder};

/// Sets a tool version for the current shell session
///
/// Only works in a session where rtx is already activated.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "s", after_long_help = AFTER_LONG_HELP)]
pub struct Shell {
    /// Tool(s) to use
    #[clap(value_name = "TOOL@VERSION", value_parser = ToolArgParser)]
    tool: Vec<ToolArg>,

    /// Removes a previously set version
    #[clap(long, short)]
    unset: bool,
}

impl Shell {
    pub fn run(self, config: Config, out: &mut Output) -> Result<()> {
        if !config.is_activated() {
            err_inactive()?;
        }

        let mut ts = ToolsetBuilder::new().with_args(&self.tool).build(&config)?;
        ts.install_arg_versions(&config)?;

        let shell = get_shell(None).expect("no shell detected");

        for (p, tv) in ts.list_current_installed_versions(&config) {
            let source = &ts.versions.get(p.name()).unwrap().source;
            if matches!(source, ToolSource::Argument) {
                let k = format!("RTX_{}_VERSION", p.name().to_uppercase());
                let op = if self.unset {
                    shell.unset_env(&k)
                } else {
                    shell.set_env(&k, &tv.version)
                };
                out.stdout.writeln(op);
            }
        }

        Ok(())
    }
}

fn err_inactive() -> Result<()> {
    Err(eyre!(formatdoc!(
        r#"
                rtx is not activated in this shell session.
                Please run `{}` first in your shell rc file.
                "#,
        style("rtx activate").yellow()
    )))
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx shell node@20</bold>
  $ <bold>node -v</bold>
  v20.0.0
"#
);

#[cfg(test)]
mod tests {
    use std::env;

    use insta::assert_display_snapshot;

    use crate::{assert_cli_err, assert_cli_snapshot};

    #[test]
    fn test_shell() {
        let err = assert_cli_err!("shell", "tiny@1.0.1");
        assert_display_snapshot!(err);
        env::set_var("__RTX_DIFF", "");
        env::set_var("RTX_SHELL", "zsh");
        assert_cli_snapshot!("shell", "tiny@1.0.1");
        env::remove_var("__RTX_DIFF");
        env::remove_var("RTX_SHELL");
    }
}
