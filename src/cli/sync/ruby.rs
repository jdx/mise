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
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"brew install ruby
mise sync ruby --brew
mise ls ruby --installed # inspect linked versions, then select one with mise use"###
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
