use eyre::Result;

use crate::file::display_path;
use crate::install_layout::resolver;

/// Choose which installation mise uses when no lockfile pins one
///
/// Several installations can match the same request, such as a refreshed copy next
/// to one a lockfile pinned, or installations made for different lockfiles. For a
/// request with no lockfile entry, mise uses the installation selected for that
/// tool, version, platform, and options. If none is selected and several match,
/// mise stops and tells you to run this command.
///
/// The selection is shared by every project on this machine that makes the same
/// request without a lockfile. Projects whose lockfile pins an artifact keep the
/// installation of that artifact. The version link (`installs/<tool>/<version>`)
/// is pointed at the selected installation.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example("mise installs ls jq", help = "Find jq's installations"),
    example(
        "mise installs select jq-hm3qa4vb",
        help = "Use this one for requests with no lockfile entry"
    ),
    verbatim_doc_comment
)]
pub(super) struct InstallsSelect {
    /// The installation to select: its directory name, or its path
    #[usage(value_name = "INSTALLATION")]
    installation: String,
}

impl InstallsSelect {
    pub(super) fn run(self) -> Result<()> {
        let selected = resolver::select(&self.installation)?;
        let tool = selected
            .requested_as
            .as_deref()
            .unwrap_or(&selected.backend);
        miseprintln!(
            "selected {} for {tool}@{} ({}); every project that asks for it without a lockfile now uses it",
            display_path(&selected.dir),
            selected.version,
            selected.platform
        );
        Ok(())
    }
}
