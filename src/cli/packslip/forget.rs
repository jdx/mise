use eyre::Result;

use crate::backend::packslip::project_name;
use crate::packslip_pins;

/// Forget a project's pinned signer, so the next release accepted sets it again
///
/// Do this when the vendor has announced a new signing identity or key. The
/// project is named as the packslip backend names it: `github.com/owner/repo`,
/// `owner/repo`, or a host such as `tool.example.com`, with or without the
/// `packslip:` prefix.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, effect = "write")]
pub(super) struct PackslipForget {
    /// The project whose pin to drop
    project: String,
}

impl PackslipForget {
    pub(super) fn run(self) -> Result<()> {
        let name = self
            .project
            .strip_prefix("packslip:")
            .unwrap_or(&self.project);
        let project = project_name(name)?;
        if packslip_pins::forget(&project)? {
            miseprintln!("forgot the pinned signer of packslip:{project}");
        } else {
            miseprintln!("packslip:{project} had no pinned signer");
        }
        Ok(())
    }
}
