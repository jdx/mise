use eyre::Result;

/// Never capture paths matching a glob
///
/// Adds a glob to `[history] exclude` in the global configuration. The
/// rule applies to all tracked paths. Quote the glob to prevent your shell
/// from expanding it.
///
/// Use exclusions for logs, caches, databases, and session state. For
/// configuration you want to save manually, use `--no-autosave` instead.
/// To scope selection to one directory, edit that `[dotfiles]` entry's
/// `exclude` or `include` list.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
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
/// Removes the specified glob from `[history] exclude` in the global
/// configuration. Pass the same pattern used with `mise dot exclude`:
///
///     mise dot exclude '~/.codex/sessions/**'
///     mise dot include '~/.codex/sessions/**'
///
/// Other matching exclusion rules still apply. This command does not edit
/// a tracked directory's `include` list; change that field in `[dotfiles]`
/// to select which files the directory saves.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
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
