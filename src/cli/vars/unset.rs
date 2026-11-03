use eyre::{Result, bail};

use crate::config::env_directive::prompt;

/// Forget saved answers so they are asked for again
///
/// Edits `$MISE_STATE_DIR/vars.toml`; no config is loaded. The next
/// `mise vars prompt` or `mise bootstrap --prompt-vars` asks again.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_aliases = ["rm", "remove"],
    verbatim_doc_comment,
    example(r###"mise vars unset git_name"###, help = "Be asked for git_name again")
)]
pub(super) struct VarsUnset {
    /// The vars whose answers to forget
    #[usage(value_name = "NAME")]
    names: Vec<String>,
}

impl VarsUnset {
    pub(super) fn run(self) -> Result<()> {
        if self.names.is_empty() {
            bail!("name at least one var to unset");
        }
        for name in &self.names {
            if !prompt::remove(name)? {
                warn!("no saved answer for '{name}'");
            }
        }
        Ok(())
    }
}
