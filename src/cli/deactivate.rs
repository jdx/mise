use eyre::Result;

use crate::env;
use crate::shell::{EXAMPLE_SHELL, build_deactivation_script, require_shell};

/// Print the script to disable mise in the current shell session
///
/// In a shell where mise is activated, run `mise deactivate`: the activation
/// function evaluates the script for you. Calling the executable directly
/// (`command mise deactivate`) only prints the script, so evaluate it yourself as
/// in the examples below. New shells still activate mise; remove the line from your
/// shell startup file to stop that.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example("mise deactivate", help = "Turn mise off in an activated shell"),
    example(
        r###"eval "$(command mise deactivate)""###,
        help = r###"Bash or Zsh, calling the executable rather than the activation function"###
    ),
    example(
        r###"command mise deactivate | source"###,
        help = r###"Fish, calling the executable rather than the activation function"###
    )
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
