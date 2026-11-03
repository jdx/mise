use std::path::PathBuf;

use eyre::Result;
use itertools::sorted;

use crate::{
    backend,
    config::{self, Config},
    file,
};

use super::reconcile;

/// Symlink ruby versions installed by Homebrew into mise
///
/// Use this to make versions installed by another version manager available to
/// mise. It does not overwrite managed installs, runtime aliases, or links from
/// other providers. Homebrew is the only source, so --brew is required. mise
/// links each ruby@X.Y directory under Homebrew's opt directory, not opt/ruby.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"brew install ruby@3.4
mise sync ruby --brew
mise ls ruby --installed"###,
        help = "Link the Homebrew ruby@3.4, then list it with the other installed versions"
    )
)]
pub(super) struct SyncRuby {
    #[usage(flatten)]
    _type: SyncRubyType,
}

#[derive(Debug, usage_rs::Args)]
pub(super) struct SyncRubyType {
    /// Get tool versions from Homebrew
    #[usage(long, required = true)]
    brew: bool,
}

impl SyncRuby {
    pub(super) async fn run(self) -> Result<()> {
        if self._type.brew {
            self.run_brew().await?;
        }
        let config = Config::reset().await?;
        let ts = config.get_toolset().await?;
        config::rebuild_shims_and_runtime_symlinks(
            &config,
            ts,
            &[],
            crate::lockfile::LockfileUpdateMode::Normal,
        )
        .await?;
        Ok(())
    }

    async fn run_brew(&self) -> Result<()> {
        let ruby = backend::get(&"ruby".into()).unwrap();

        let brew_opt = PathBuf::from(cmd!("brew", "--prefix").read()?).join("opt");

        let subdirs = file::dir_subdirs(&brew_opt)?;
        let mut links = vec![];
        for entry in sorted(subdirs) {
            if entry.starts_with(".") {
                continue;
            }
            if !entry.starts_with("ruby@") {
                continue;
            }
            let v = entry.trim_start_matches("ruby@");
            links.push((v.to_string(), brew_opt.join(&entry)));
        }
        let ownership = reconcile::LinkOwnership::in_namespace(&brew_opt);
        for v in reconcile::reconcile(ruby.ba(), ownership, links)? {
            miseprintln!("Synced ruby@{} from Homebrew", v);
        }
        Ok(())
    }
}
