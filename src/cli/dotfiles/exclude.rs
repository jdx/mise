use eyre::Result;

/// Never capture paths matching a glob
///
/// Adds the glob to `[history] exclude` in the global config, which
/// applies to every tracked path. To choose what one tracked directory
/// saves, give that `[dotfiles]` entry its own `exclude` or `include`
/// list instead. Use it for
/// logs, caches, databases, and constantly rewritten application state; a
/// file that genuinely holds configuration but changes constantly is
/// better tracked with `--no-autosave` and saved explicitly.
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
/// Removes the glob from `[history] exclude` in the global config, the
/// list `mise dot exclude` writes: this is the inverse of that command.
///
/// It is not the per-entry `include` allowlist. That one lives on a
/// `[dotfiles]` entry, names what a tracked directory saves rather than
/// what it skips, and is edited in the configuration file:
///
///     mise dot exclude '~/.codex/sessions/**'   # global skip list
///     mise dot include '~/.codex/sessions/**'   # take that back
///
///     [dotfiles]                                             # per-entry
///     "~/.codex" = { mode = "track", include = ["config.toml"] }
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
