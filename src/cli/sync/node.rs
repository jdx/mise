use std::path::PathBuf;

use eyre::Result;
use itertools::sorted;

use crate::{backend, cmd, config, dirs, file};
use crate::{
    config::Config,
    env::{NODENV_ROOT, NVM_DIR},
};

/// Symlinks all tool versions from an external tool into mise
///
/// For example, use this to import all Homebrew node installs into mise
///
/// This won't overwrite any existing installs but will overwrite any existing symlinks
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

    /// Get tool versions from nvm
    #[clap(long)]
    nvm: bool,

    /// Get tool versions from nodenv
    #[clap(long)]
    nodenv: bool,
}

impl SyncNode {
    pub async fn run(self) -> Result<()> {
        if self._type.brew {
            self.run_brew().await?;
        }
        if self._type.nvm {
            self.run_nvm().await?;
        }
        if self._type.nodenv {
            self.run_nodenv().await?;
        }
        let config = Config::reset().await?;
        let ts = config.get_toolset().await?;
        config::rebuild_shims_and_runtime_symlinks(&config, ts, &[]).await?;
        Ok(())
    }

    async fn run_brew(&self) -> Result<()> {
        let node = backend::get(&"node".into()).unwrap();

        let brew_prefix = PathBuf::from(cmd!("brew", "--prefix").read()?).join("opt");
        let installed_versions_path = dirs::INSTALLS.join("node");

        file::remove_symlinks_with_target_prefix(&installed_versions_path, &brew_prefix)?;

        let subdirs = file::dir_subdirs(&brew_prefix)?;
        for entry in sorted(subdirs) {
            if entry.starts_with(".") {
                continue;
            }
            if !entry.starts_with("node@") {
                continue;
            }
            let v = entry.trim_start_matches("node@");
            if node.create_symlink(v, &brew_prefix.join(&entry))?.is_some() {
                miseprintln!("Synced node@{} from Homebrew", v);
            }
        }
        Ok(())
    }

    async fn run_nvm(&self) -> Result<()> {
        let node = backend::get(&"node".into()).unwrap();

        let nvm_versions_path = NVM_DIR.join("versions").join("node");
        let installed_versions_path = dirs::INSTALLS.join("node");

        let removed =
            file::remove_symlinks_with_target_prefix(&installed_versions_path, &nvm_versions_path)?;
        if !removed.is_empty() {
            debug!("Removed symlinks: {removed:?}");
        }

        let mut created = vec![];
        let subdirs = file::dir_subdirs(&nvm_versions_path)?;
        for entry in sorted(subdirs) {
            if entry.starts_with(".") {
                continue;
            }
            let v = entry.trim_start_matches('v');
            let symlink = node.create_symlink(v, &nvm_versions_path.join(&entry))?;
            if let Some(symlink) = symlink {
                created.push(symlink);
                miseprintln!("Synced node@{} from nvm", v);
            } else {
                info!("Skipping node@{v} from nvm because it already exists in mise");
            }
        }
        if !created.is_empty() {
            debug!("Created symlinks: {created:?}");
        }
        Ok(())
    }

    async fn run_nodenv(&self) -> Result<()> {
        let node = backend::get(&"node".into()).unwrap();

        let nodenv_versions_path = NODENV_ROOT.join("versions");
        let installed_versions_path = dirs::INSTALLS.join("node");

        file::remove_symlinks_with_target_prefix(&installed_versions_path, &nodenv_versions_path)?;

        let subdirs = file::dir_subdirs(&nodenv_versions_path)?;
        for v in sorted(subdirs) {
            if v.starts_with(".") {
                continue;
            }
            if node
                .create_symlink(&v, &nodenv_versions_path.join(&v))?
                .is_some()
            {
                miseprintln!("Synced node@{} from nodenv", v);
            }
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>brew install node@18 node@20</bold>
    $ <bold>mise sync node --brew</bold>
    $ <bold>mise use -g node@18</bold> - uses Homebrew-provided node
"#
);
