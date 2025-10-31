use eyre::{Result, eyre};
use indoc::formatdoc;

use crate::env;
use crate::shell::{build_deactivation_script, get_shell};
use crate::ui::style;

/// Disable mise for current shell session
///
/// This can be used to temporarily disable mise in a shell session.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Deactivate {}

impl Deactivate {
    pub fn run(self) -> Result<()> {
        if !env::is_activated() {
            err_inactive()?;
        }

        let shell = get_shell(None).expect("no shell detected");

        let output = build_deactivation_script(&*shell);
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

    $ <bold>mise deactivate</bold>
"#
);
