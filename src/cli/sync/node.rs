use std::path::PathBuf;

use eyre::Result;
use itertools::sorted;

use crate::{backend, config, file};
use crate::{config::Config, config::Settings};

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

    /// Get tool versions from nodenv
    #[clap(long)]
    nodenv: bool,

    /// Get tool versions from nvm
    #[clap(long)]
    nvm: bool,
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
        let node = backend::get(&"node".into()).unwrap();

        let brew_prefix = PathBuf::from(cmd!("brew", "--prefix").read()?).join("opt");

        let subdirs = file::dir_subdirs(&brew_prefix)?;
        let mut links = vec![];
        for entry in sorted(subdirs) {
            if entry.starts_with(".") {
                continue;
            }
            if !entry.starts_with("node@") {
                continue;
            }
            let v = entry.trim_start_matches("node@");
            links.push((v.to_string(), brew_prefix.join(&entry)));
        }
        for v in node.sync_symlinks(&brew_prefix, links)? {
            miseprintln!("Synced node@{} from Homebrew", v);
        }
        Ok(())
    }

    async fn run_nvm(&self) -> Result<()> {
        let node = backend::get(&"node".into()).unwrap();
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
        for v in node.sync_symlinks(&nvm_versions_path, links)? {
            miseprintln!("Synced node@{} from nvm", v);
        }
        Ok(())
    }

    async fn run_nodenv(&self) -> Result<()> {
        let node = backend::get(&"node".into()).unwrap();
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
        for v in node.sync_symlinks(&nodenv_versions_path, links)? {
            miseprintln!("Synced node@{} from nodenv", v);
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
