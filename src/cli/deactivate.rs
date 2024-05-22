use eyre::Result;

use crate::shell::get_shell;
use crate::ui::style;
use crate::{env, hook_env};

/// Disable mise for current shell session
///
/// This can be used to temporarily disable mise in a shell session.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Deactivate {}

impl Deactivate {
    pub async fn run(self) -> Result<()> {
        if !env::is_activated() {
            err_inactive()?;
        }

        let shell = get_shell(None).expect("no shell detected");

        miseprint!("{}", hook_env::clear_old_env(&*shell))?;
        let output = shell.deactivate();
        miseprint!("{output}")?;

        Ok(())
    }
}

fn err_inactive() -> Result<()> {
    Err(eyre!(formatdoc!(
        r#"
                mise is not activated in this shell session.
                Please run `{}` first in your shell rc file.
                "#,
        style::eyellow("mise activate")
    )))
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise deactivate bash</bold>
    $ <bold>mise deactivate zsh</bold>
    $ <bold>mise deactivate fish</bold>
    $ <bold>execx($(mise deactivate xonsh))</bold>
"#
);

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::env;
    use crate::test::reset;
    use test_log::test;

    #[test(tokio::test)]
    async fn test_deactivate() {
        reset().await;
        let _config = Config::try_get().unwrap(); // hack: prevents error parsing __MISE_DIFF
        let err = assert_cli_err!("deactivate");
        assert_snapshot!(err);
        env::set_var("__MISE_DIFF", "");
        env::set_var("MISE_SHELL", "zsh");
        assert_cli_snapshot!("deactivate");
        env::remove_var("__MISE_DIFF");
        env::remove_var("MISE_SHELL");
    }
}
