use eyre::Result;

use crate::backend::packslip::project_name;
use crate::packslip_pins;

/// Forget a project's pinned signer so the next accepted release sets it
///
/// Do this after the vendor announces a new signing identity or key. Name the
/// project as in its tool identifier: `github.com/owner/repo`, `owner/repo`, or
/// a domain such as `tool.example.com`, with or without `packslip:`. This also
/// resets the project's remembered release-list state.
///
/// It does not change `mise.lock`. If the lockfile records the old signer,
/// delete the tool's entries and run `mise lock`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    effect = "write",
    example(
        r###"mise packslip forget github.com/jdx/hk"###,
        help = "Accept whichever signer the next hk release uses"
    )
)]
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
