use color_eyre::eyre::{eyre, Result};
use console::style;
use indoc::formatdoc;

use crate::cli::command::Command;
use crate::config::Config;
use crate::hook_env;
use crate::output::Output;
use crate::shell::get_shell;

/// Disable rtx for current shell session
///
/// This can be used to temporarily disable rtx in a shell session.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Deactivate {}

impl Command for Deactivate {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        if !config.is_activated() {
            err_inactive()?;
        }

        let shell = get_shell(None).expect("no shell detected");

        out.stdout.write(hook_env::clear_old_env(&*shell));
        let output = shell.deactivate();
        out.stdout.write(output);

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
  $ <bold>rtx deactivate bash</bold>
  $ <bold>rtx deactivate zsh</bold>
  $ <bold>rtx deactivate fish</bold>
  $ <bold>execx($(rtx deactivate xonsh))</bold>
"#
);

#[cfg(test)]
mod tests {
    use insta::assert_display_snapshot;

    use crate::{assert_cli_err, assert_cli_snapshot, env};

    #[test]
    fn test_deactivate() {
        let err = assert_cli_err!("deactivate");
        assert_display_snapshot!(err);
        env::set_var("__RTX_DIFF", "");
        env::set_var("RTX_SHELL", "zsh");
        assert_cli_snapshot!("deactivate");
        env::remove_var("__RTX_DIFF");
        env::remove_var("RTX_SHELL");
    }
}
