use std::path::PathBuf;

use eyre::Result;
use itertools::sorted;

use crate::{backend, config, file};
use crate::{config::Config, config::Settings};

use super::reconcile;

/// Symlinks all tool versions from an external tool into mise
///
/// For example, use this to import all Homebrew node installs into mise
///
/// This won't overwrite managed installs, runtime aliases, or links from other providers.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SyncNode {
    #[clap(flatten)]
    _type: SyncNodeType,
}

#[derive(Debug, clap::Args)]
#[group(required = true, multiple = true)]
pub struct SyncNodeType {
    /// Get tool versions from Homebrew
    #[clap(long)]
    brew: bool,

    /// Get tool versions from nodenv
    #[clap(long)]
    nodenv: bool,

    /// Get tool versions from nvm
    #[clap(long)]
    nvm: bool,
}

impl SyncNode {
    pub async fn run(self) -> Result<()> {
        let node = backend::get(&"node".into()).unwrap();
        let mut providers = vec![];
        if self._type.brew {
            providers.push(self.brew_links()?);
        }
        if self._type.nvm {
            providers.push(self.nvm_links()?);
        }
        if self._type.nodenv {
            providers.push(self.nodenv_links()?);
        }
        let mut changed = reconcile::reconcile_all(node.ba(), providers)?.into_iter();
        if self._type.brew {
            for v in changed.next().unwrap_or_default() {
                miseprintln!("Synced node@{} from Homebrew", v);
            }
        }
        if self._type.nvm {
            for v in changed.next().unwrap_or_default() {
                miseprintln!("Synced node@{} from nvm", v);
            }
        }
        if self._type.nodenv {
            for v in changed.next().unwrap_or_default() {
                miseprintln!("Synced node@{} from nodenv", v);
            }
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

    fn brew_links(&self) -> Result<reconcile::ProviderLinks> {
        let brew_opt = PathBuf::from(cmd!("brew", "--prefix").read()?).join("opt");

        let subdirs = file::dir_subdirs(&brew_opt)?;
        let mut links = vec![];
        for entry in sorted(subdirs) {
            if entry.starts_with(".") {
                continue;
            }
            if !entry.starts_with("node@") {
                continue;
            }
            let v = entry.trim_start_matches("node@");
            links.push((v.to_string(), brew_opt.join(&entry)));
        }
        let ownership = reconcile::LinkOwnership::in_namespace(&brew_opt);
        Ok(reconcile::ProviderLinks::new(ownership, links))
    }

    fn nvm_links(&self) -> Result<reconcile::ProviderLinks> {
        let settings = Settings::get();

        let nvm_versions_path = file::replace_path(&settings.node.nvm_dir)
            .join("versions")
            .join("node");

        let subdirs = file::dir_subdirs(&nvm_versions_path)?;
        let mut links = vec![];
        for entry in sorted(subdirs) {
            if entry.starts_with(".") {
                continue;
            }
            let v = entry.trim_start_matches('v');
            links.push((v.to_string(), nvm_versions_path.join(&entry)));
        }
        let ownership = reconcile::LinkOwnership::in_namespace(&nvm_versions_path);
        Ok(reconcile::ProviderLinks::new(ownership, links))
    }

    fn nodenv_links(&self) -> Result<reconcile::ProviderLinks> {
        let settings = Settings::get();

        let nodenv_versions_path = file::replace_path(&settings.node.nodenv_root).join("versions");

        let subdirs = file::dir_subdirs(&nodenv_versions_path)?;
        let mut links = vec![];
        for v in sorted(subdirs) {
            if v.starts_with(".") {
                continue;
            }
            links.push((v.clone(), nodenv_versions_path.join(&v)));
        }
        let ownership = reconcile::LinkOwnership::in_namespace(&nodenv_versions_path);
        Ok(reconcile::ProviderLinks::new(ownership, links))
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>brew install node@18 node@20</bold>
    $ <bold>mise sync node --brew</bold>
    $ <bold>mise use -g node@18</bold> - uses Homebrew-provided node
"#
);
