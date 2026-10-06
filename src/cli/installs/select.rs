use eyre::Result;

use crate::file::display_path;
use crate::install_layout::resolver;

/// Choose the installation that requests without a lockfile use
///
/// Several installations can answer the same request: a refresh that could not
/// replace a lockfile's installation in place, or installations made for
/// different lockfiles. Requests without a lockfile entry use the one selected
/// for their tool, version, platform and options, and when none is selected and
/// several match, mise stops and asks for this command.
///
/// The selection is shared by every project on this machine that makes the same
/// request without a lockfile. Projects whose lockfile pins an artifact keep the
/// installation of that artifact. The version link (`installs/<tool>/<version>`)
/// is pointed at the selected installation.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise installs ls jq
mise installs select jq-hm3qa4vb"###,
        help = r###"Find jq's installations, then choose one"###
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
