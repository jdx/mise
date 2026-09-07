use eyre::Result;

/// Never capture paths matching a glob
///
/// Adds the glob to `[history] exclude` in the global config. Use it for
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
        super::paths::edit_exclude(&self.glob, true)
    }
}

/// Capture paths matching a glob again
///
/// Removes the glob from `[history] exclude` in the global config.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct DotfilesInclude {
    /// The glob as written by `exclude`
    glob: String,
}

impl DotfilesInclude {
    pub(crate) async fn run(self) -> Result<()> {
        super::paths::edit_exclude(&self.glob, false)
    }
}
