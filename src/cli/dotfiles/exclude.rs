use eyre::Result;

/// Never save paths matching a glob
///
/// Adds a glob to `[history] exclude` in the global config. The rule applies
/// to every tracked path. Quote the glob so your shell does not expand it.
///
/// Use exclusions for logs, caches, databases, and session state. For files
/// you want to save only on demand, track them with
/// `mise dot track --no-autosave` instead. To limit what one tracked directory
/// saves, edit the `exclude` or `include` list of its `[dotfiles]` entry.
#[derive(Debug, usage_rs::Args)]
#[usage(example(
    "mise dot exclude '~/.codex/sessions/**'",
    help = "Never save Codex session logs"
))]
pub(crate) struct DotfilesExclude {
    /// A glob such as `~/.config/hypr/plugins/**`
    glob: String,
}

impl DotfilesExclude {
    pub(crate) async fn run(self) -> Result<()> {
        let _declarations = super::track::declaration_lock()?;
        super::paths::edit_exclude(&self.glob, true)
    }
}

/// Stop excluding paths matching a glob
///
/// Removes a glob that `mise dot exclude` added to `[history] exclude` in the
/// global config. Pass the pattern exactly as you excluded it. Other matching
/// exclusion rules still apply. This command does not edit a tracked
/// directory's `include` list; change that list in its `[dotfiles]` entry.
#[derive(Debug, usage_rs::Args)]
#[usage(example(
    "mise dot include '~/.codex/sessions/**'",
    help = "Save Codex session logs again"
))]
pub(crate) struct DotfilesInclude {
    /// The glob as written by `mise dot exclude`
    glob: String,
}

impl DotfilesInclude {
    pub(crate) async fn run(self) -> Result<()> {
        let _declarations = super::track::declaration_lock()?;
        super::paths::edit_exclude(&self.glob, false)
    }
}
