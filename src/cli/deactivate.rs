use eyre::Result;

use crate::env;
use crate::shell::{EXAMPLE_SHELL, build_deactivation_script, require_shell};

/// Disable mise for current shell session
///
/// This can be used to temporarily disable mise in a shell session.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Deactivate {}

impl Deactivate {
    pub fn run(self) -> Result<()> {
        if !env::is_activated() {
            // Deactivating when not activated is safe - just show a warning
            warn!(
                "mise is not activated in this shell session. Already deactivated or never activated."
            );
            return Ok(());
        }

        let shell = require_shell(
            None,
            &format!("Re-run `mise activate {EXAMPLE_SHELL}` in your shell rc file."),
        )?;

        let mut output = build_deactivation_script(&*shell);
        output.push_str(&shell.unset_env("__MISE_ORIG_PATH"));
        miseprint!("{output}")?;

        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise deactivate</bold>
"#
);
