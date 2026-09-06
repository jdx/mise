use eyre::Result;

use crate::env;
use crate::shell::{EXAMPLE_SHELL, build_deactivation_script, require_shell};

/// Disable mise for current shell session
///
/// This can be used to temporarily disable mise in a shell session.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"eval "$(command mise deactivate)""###,
        help = r###"Bash or Zsh, calling the executable rather than the activation function"###
    ),
    example(r###"command mise deactivate | source"###, help = r###"Fish"###)
)]
pub(crate) struct Deactivate {}

impl Deactivate {
    pub(crate) fn run(self) -> Result<()> {
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
